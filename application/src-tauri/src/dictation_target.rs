//! Targeted Windows dictation. UI Automation checks the actual editable control,
//! rather than treating every foreground window as a text field. Provisional
//! composition reads only bounded UIA ranges to verify ownership; no clipboard,
//! whole-field SetValue or blind backspace sequence is used.

#[cfg(windows)]
mod native {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, OnceLock};
    use std::time::{Duration, Instant};

    use ::windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use ::windows::Win32::System::Variant::VARIANT;
    use ::windows::Win32::UI::Accessibility::{CUIAutomation8, IUIAutomation2, IUIAutomationElement,
        IUIAutomationTextPattern, IUIAutomationTextRange, IUIAutomationValuePattern, UIA_ComboBoxControlTypeId,
        UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId,
        UIA_TextPatternId, UIA_ValuePatternId, TextPatternRangeEndpoint_Start,
        TextPatternRangeEndpoint_End, TextUnit_Character};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0,
        INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_CONTROL, VK_MENU,
        VK_SHIFT, VK_LWIN, VK_RWIN, VK_DELETE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow,
        GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow};

    const POLL: Duration = Duration::from_millis(150);
    const REQUEST_LIMIT: Duration = Duration::from_millis(1400);
    const WAIT_LIMIT: Duration = Duration::from_millis(1600);
    const TARGET_TTL: Duration = Duration::from_secs(5 * 3600);
    const MAX_TARGETS: usize = 32;
    const MAX_COMPOSITION_BATCH: usize = 1_024;
    static TX: OnceLock<mpsc::SyncSender<Request>> = OnceLock::new();
    static CANCEL_EPOCH: AtomicU64 = AtomicU64::new(0);

    struct Budget {
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        epoch: u64,
    }
    impl Budget {
        fn check(&self) -> Result<(), String> {
            if self.cancelled.load(Ordering::Acquire)
                || self.epoch != CANCEL_EPOCH.load(Ordering::Acquire)
                || Instant::now() >= self.deadline
            {
                Err("Insertion annulée ou expirée. Le texte reste dans Écho.".into())
            } else { Ok(()) }
        }
    }
    enum Request {
        Freeze { reply: mpsc::Sender<Result<String, String>>, deadline: Instant },
        Insert { token: String, text: String, budget: Budget, reply: mpsc::Sender<Result<(), String>> },
        Preview { token: String, text: String, budget: Budget, reply: mpsc::Sender<Result<bool, String>> },
        Discard { token: String, budget: Budget, reply: mpsc::Sender<Result<bool, String>> },
    }
    #[derive(Clone)]
    struct Observed {
        window: isize,
        window_pid: u32,
        element_pid: i32,
        element: IUIAutomationElement,
        external_epoch: u64,
        seen: Instant,
    }
    #[derive(Clone)]
    struct Composition { text: String, range: IUIAutomationTextRange }
    struct Target { token: String, observed: Observed, created: Instant, tainted: bool,
                    composition: Option<Composition> }
    struct Worker {
        automation: IUIAutomation2,
        last: Option<Observed>,
        last_external: Option<(isize,u32)>,
        external_epoch: u64,
        targets: VecDeque<Target>,
    }

    fn foreground() -> Option<(isize,u32)> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() || unsafe { IsWindowVisible(window) } == 0 { return None; }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid); }
        if pid == 0 { None } else { Some((window as isize,pid)) }
    }
    fn still_exists(observed: &Observed) -> bool {
        let window = observed.window as _;
        if observed.window == 0 || unsafe { IsWindow(window) } == 0 { return false; }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid); }
        pid == observed.window_pid
    }
    fn uia_error(_: ::windows::core::Error) -> String {
        "Le champ ciblé ne répond plus à Windows. Le texte reste dans Écho ; utilise Copier.".into()
    }

    /// ValuePattern covers plain text fields. Rich editors may expose only
    /// TextPattern: require an explicit false IsReadOnly attribute, never infer
    /// editability just because a document or a page has keyboard focus.
    fn editable(element: &IUIAutomationElement) -> Result<i32, String> {
        unsafe {
            if !element.CurrentIsEnabled().map_err(uia_error)?.as_bool()
                || !element.CurrentIsKeyboardFocusable().map_err(uia_error)?.as_bool()
                || element.CurrentIsPassword().map_err(uia_error)?.as_bool()
                || element.CurrentIsOffscreen().map_err(uia_error)?.as_bool()
            {
                return Err("Clique dans un champ de texte modifiable, puis relance la dictée.".into());
            }
            let kind = element.CurrentControlType().map_err(uia_error)?;
            if kind != UIA_EditControlTypeId && kind != UIA_DocumentControlTypeId && kind != UIA_ComboBoxControlTypeId {
                return Err("La zone sélectionnée n’est pas un champ de texte. Le texte reste disponible dans Écho.".into());
            }
            let writable = if let Ok(value) = element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                !value.CurrentIsReadOnly().map_err(uia_error)?.as_bool()
            } else if let Ok(text) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                let range = text.DocumentRange().map_err(uia_error)?;
                let attribute = range.GetAttributeValue(UIA_IsReadOnlyAttributeId).map_err(uia_error)?;
                attribute == VARIANT::from(false)
            } else { false };
            if !writable {
                return Err("Windows ne confirme pas que ce champ accepte du texte. Utilise Copier ou Note Écho.".into());
            }
            element.CurrentProcessId().map_err(uia_error)
        }
    }

    impl Worker {
        // Rich editors can move focus between editable document fragments as
        // lines are created. Compare their actual enclosing text box, never
        // arbitrary fields in the same window. No text content is inspected.
        fn focused_field(&self) -> Result<(IUIAutomationElement,i32),String> {
            let focus=unsafe{self.automation.GetFocusedElement()}.map_err(uia_error)?;
            if !unsafe{focus.CurrentHasKeyboardFocus()}.map_err(uia_error)?.as_bool(){return Err("Aucun champ n’a le focus.".into());}
            let pid=editable(&focus)?;
            if unsafe{focus.CurrentControlType()}.map_err(uia_error)?!=UIA_DocumentControlTypeId{return Ok((focus,pid));}
            let walker=unsafe{self.automation.ControlViewWalker()}.map_err(uia_error)?;
            let mut current=focus.clone();let until=Instant::now()+Duration::from_millis(400);
            for _ in 0..6 {
                if Instant::now()>=until{break}
                let Ok(parent)=(unsafe{walker.GetParentElement(&current)})else{break};
                if unsafe{parent.CurrentProcessId()}.ok()!=Some(pid){break}
                let kind=unsafe{parent.CurrentControlType()}.map_err(uia_error)?;
                if kind==UIA_EditControlTypeId && editable(&parent).ok()==Some(pid){return Ok((parent,pid));}
                current=parent;
            }
            Ok((focus,pid))
        }
        fn update_external(&mut self, current: (isize,u32)) {
            if current.1 == std::process::id() { return; }
            if self.last_external != Some(current) {
                self.external_epoch = self.external_epoch.wrapping_add(1);
                self.last_external = Some(current);
                self.last = None;
            }
        }
        fn observe(&mut self) {
            let Some(current) = foreground() else { self.last = None; return; };
            if current.1 == std::process::id() {
                // A real editable control inside Écho is also a valid cursor destination.
                // Toolbar buttons preserve the last observed editable field.
                if let Ok((element,element_pid))=self.focused_field() {
                        if foreground()==Some(current) {
                            self.last=Some(Observed{window:current.0,window_pid:current.1,element_pid,element,external_epoch:self.external_epoch,seen:Instant::now()});
                        }
                }
                return;
            }
            self.update_external(current);
            let candidate = (|| {
                let (element,element_pid) = self.focused_field()?;
                // A provider can answer after focus moved. Its result then is
                // discarded, rather than attached to the new foreground HWND.
                if foreground() != Some(current) { return Err("Le focus a changé.".into()); }
                Ok::<_,String>(Observed { window:current.0, window_pid:current.1, element_pid,
                    element, external_epoch:self.external_epoch, seen:Instant::now() })
            })();
            self.last = candidate.ok();
        }
        fn freeze(&mut self, deadline: Instant) -> Result<String,String> {
            if Instant::now() >= deadline { return Err("Le champ n’a pas répondu. Clique dedans et réessaie.".into()); }
            self.observe();
            if Instant::now() >= deadline { return Err("Le champ n’a pas répondu. Clique dedans et réessaie.".into()); }
            let observed = self.last.as_ref().ok_or("Clique d’abord dans le champ de ta conversation, puis utilise la dictée. Si l’application ne l’expose pas à Windows, utilise Copier.")?.clone();
            if !still_exists(&observed) || observed.seen.elapsed() > Duration::from_secs(120) {
                return Err("Clique à nouveau dans le champ où tu veux écrire.".into());
            }
            self.targets.retain(|target| target.created.elapsed() < TARGET_TTL);
            while self.targets.len() >= MAX_TARGETS { self.targets.pop_front(); }
            let token = uuid::Uuid::new_v4().to_string();
            self.targets.push_back(Target { token:token.clone(), observed, created:Instant::now(), tainted:false, composition:None });
            Ok(token)
        }
        fn validate_focused_field(&self, observed:&Observed, budget:&Budget) -> Result<(),String> {
            budget.check()?;
            let (focus,focus_pid) = self.focused_field()?;
            if !unsafe { self.automation.CompareElements(&observed.element, &focus) }.map_err(uia_error)?.as_bool()
            {
                return Err("Le champ actif a changé depuis le début de la dictée. Le texte reste dans Écho.".into());
            }
            if focus_pid != observed.element_pid {
                return Err("Le champ ciblé a été remplacé. Le texte reste dans Écho.".into());
            }
            budget.check()
        }
        fn insert(&mut self, token:&str, text:&str, budget:&Budget) -> Result<(),String> {
            budget.check()?;
            let Some(index) = self.targets.iter().position(|t| t.token == token) else {
                return Err("Cette cible appartient à une ancienne session. Le texte est conservé : utilise Copier ou Note Écho.".into());
            };
            if self.targets[index].composition.is_some() {
                // Even an identical final must finalize the owned span. A
                // failed composition never falls through to a second append.
                return self.compose(token,text,true,budget).and_then(|done| if done {Ok(())}
                    else {Err("La composition ne peut plus être finalisée. Le texte reste dans Écho.".into())});
            }
            let target = &self.targets[index];
            if target.tainted || target.created.elapsed() >= TARGET_TTL {
                return Err("Vérifie le champ avant de recopier ce résultat. Écho refuse une seconde insertion automatique incertaine.".into());
            }
            let observed = target.observed.clone();
            if !still_exists(&observed) { return Err("La fenêtre ciblée a été fermée ou remplacée. Le texte reste dans Écho.".into()); }
            let current = foreground().ok_or("Aucune fenêtre active ; le texte reste dans Écho.")?;
            self.update_external(current);
            if observed.external_epoch != self.external_epoch
                || !super::foreground_allowed(current.0, current.1, observed.window, observed.window_pid, std::process::id())
            {
                return Err("Tu as changé de fenêtre pendant la dictée. Le texte est conservé ; utilise Copier ou Note Écho.".into());
            }
            if current.0 != observed.window {
                budget.check()?;
                if unsafe { SetForegroundWindow(observed.window as _) } == 0 {
                    return Err("Windows n’a pas rendu le focus au champ. Le texte reste dans Écho.".into());
                }
                std::thread::sleep(Duration::from_millis(75));
            }
            self.validate_focused_field(&observed,budget)?;
            let units = super::safe_units(text)?;
            let mut input = Vec::<INPUT>::with_capacity(units.len()*2);
            for ch in units {
                for flags in [KEYEVENTF_UNICODE,KEYEVENTF_UNICODE|KEYEVENTF_KEYUP] {
                    input.push(INPUT { r#type:INPUT_KEYBOARD, Anonymous:INPUT_0 { ki:KEYBDINPUT {
                        wVk:0,wScan:ch,dwFlags:flags,time:0,dwExtraInfo:0,
                    } } });
                }
            }
            let count = super::with_revalidated_focus(
                || {
                    // Do not inject while Ctrl/Alt/Shift/Win is held. The user
                    // can change fields while releasing the PTT chord.
                    while [VK_CONTROL,VK_MENU,VK_SHIFT,VK_LWIN,VK_RWIN].iter().any(|key| unsafe { GetAsyncKeyState(*key as i32) } < 0) {
                        budget.check()?;
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    Ok(())
                },
                || self.validate_focused_field(&observed,budget),
                || {
                    budget.check()?;
                    if foreground() != Some((observed.window,observed.window_pid)) || !still_exists(&observed) {
                        return Err("Le focus a changé avant l’insertion. Le texte reste dans Écho.".into());
                    }
                    // One batch, no Enter key and no clipboard replacement.
                    // SendInput confirms queueing, not application storage.
                    Ok(unsafe { SendInput(input.len() as u32,input.as_ptr(),std::mem::size_of::<INPUT>() as i32) })
                },
            )?;
            if count != input.len() as u32 {
                self.targets[index].tainted = count > 0;
                return Err(if count == 0 {
                    "Windows a refusé l’insertion (application éventuellement lancée en administrateur). Le texte reste dans Écho.".into()
                } else {
                    "Insertion partielle possible : vérifie le champ avant de copier le résultat. Le texte reste dans Écho.".into()
                });
            }
            Ok(())
        }

        fn target_index(&self, token:&str)->Result<usize,String> {
            let index=self.targets.iter().position(|target|target.token==token)
                .ok_or("Cette cible appartient à une ancienne session. Le texte reste dans Écho.")?;
            let target=&self.targets[index];
            if target.tainted || target.created.elapsed()>=TARGET_TTL {
                return Err("La composition est incertaine. Vérifie le champ ; aucune seconde insertion automatique n’est autorisée.".into());
            }
            Ok(index)
        }
        fn prepare_composition(&mut self,index:usize,budget:&Budget)->Result<Observed,String> {
            budget.check()?;
            let observed=self.targets[index].observed.clone();
            if !still_exists(&observed){return Err("La fenêtre ciblée a changé. Le texte reste dans Écho.".into());}
            let current=foreground().ok_or("Aucune fenêtre active.")?;
            self.update_external(current);
            if observed.external_epoch!=self.external_epoch
                || !super::foreground_allowed(current.0,current.1,observed.window,observed.window_pid,std::process::id()) {
                return Err("La fenêtre active a changé pendant la dictée. Le brouillon reste dans le champ.".into());
            }
            if current.0!=observed.window {
                budget.check()?;
                if unsafe{SetForegroundWindow(observed.window as _)}==0{return Err("Windows ne rend pas le focus au champ.".into());}
                std::thread::sleep(Duration::from_millis(75));
            }
            self.check_composition_focus(&observed,budget)?;
            Ok(observed)
        }
        fn check_composition_focus(&self,observed:&Observed,budget:&Budget)->Result<(),String> {
            budget.check()?;
            if observed.external_epoch!=self.external_epoch || !still_exists(observed)
                || foreground()!=Some((observed.window,observed.window_pid)) {
                return Err("Le focus a changé. Le brouillon n’est pas remplacé.".into());
            }
            self.validate_focused_field(observed,budget)
        }
        fn select_owned_range(&self,observed:&Observed,pattern:&IUIAutomationTextPattern,owned:&Composition,
            previous:&IUIAutomationTextRange,requested:&IUIAutomationTextRange,budget:&Budget)->Result<(),String>{
            // Computing a suffix may take several provider calls. Recheck the
            // actual caret and owned text before changing the selection.
            self.check_composition_focus(observed,budget)?;
            let before=single_selection(pattern)?;
            verify_owned(owned,&before)?;
            if !same_range(&before,previous)? {
                return Err("Le curseur a bougé avant la sélection du brouillon.".into());
            }
            self.check_composition_focus(observed,budget)?;
            // Appending a changed suffix already uses this caret. Do not queue
            // a redundant asynchronous Select which could reset it after input.
            if same_range(&before,requested)? {return Ok(())}
            unsafe{requested.Select()}.map_err(uia_error)?;
            let expected:Vec<u16>=owned.text.encode_utf16().collect();
            // Chromium can acknowledge Select after the COM call returns.
            // While pending, only the exact previously verified end caret is
            // allowed. A third selection or changed text fails immediately.
            super::wait_for_selection_acknowledgement(
                ||self.check_composition_focus(observed,budget),
                ||{
                    let selected=single_selection(pattern)?;
                    super::selection_acknowledged(
                        range_units(&owned.range,expected.len()+1)?==expected,
                        same_range(&selected,requested)?,same_range(&selected,previous)?)
                },
                ||std::thread::sleep(Duration::from_millis(15)),
            )
        }
        fn compose(&mut self,token:&str,text:&str,finalize:bool,budget:&Budget)->Result<bool,String> {
            let units=super::composition_units(text)?;
            let index=self.target_index(token)?;
            let old=self.targets[index].composition.clone();
            let result=(|| {
                let observed=self.prepare_composition(index,budget)?;
                let pattern=match unsafe{observed.element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)} {
                    Ok(pattern)=>pattern,
                    Err(_) if old.is_none()=>{budget.check()?;return Ok(false)},
                    Err(error)=>return Err(uia_error(error)),
                };
                let initial=single_selection(&pattern)?;
                if old.is_none() && !collapsed(&initial)? {return Ok(false)}
                if let Some(owned)=old.as_ref(){
                    verify_owned(owned,&initial).map_err(|error|{self.targets[index].tainted=true;error})?;
                }
                wait_modifiers(budget)?;
                self.check_composition_focus(&observed,budget)?;
                let current=single_selection(&pattern)?;
                if let Some(owned)=old.as_ref(){
                    verify_owned(owned,&current).map_err(|error|{self.targets[index].tainted=true;error})?;
                }
                else if !collapsed(&current)? || !same_range(&initial,&current)? {
                    return Err("Le curseur a bougé avant le brouillon. Aucun texte n’est envoyé.".into());
                }
                if !super::composition_needs_write(old.as_ref().map(|c|c.text.as_str()),text,finalize) {
                    return Ok(true);
                }
                if units.is_empty() && old.is_none(){return Ok(true)}
                // The saved start anchor is mandatory for re-anchoring. Merely
                // finding an identical suffix at the caret could claim text
                // that was already present before SendInput was processed.
                let anchor=if let Some(owned)=old.as_ref(){
                    let start=unsafe{owned.range.Clone()}.map_err(uia_error)?;
                    unsafe{start.MoveEndpointByRange(TextPatternRangeEndpoint_End,&owned.range,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?;
                    start
                }else{unsafe{current.Clone()}.map_err(uia_error)?};
                let (replacement,input_units)=if let Some(owned)=old.as_ref(){
                    changed_tail(owned,&current,text,&units,budget)?
                }else{(None,units.clone())};
                if input_units.len()>MAX_COMPOSITION_BATCH {
                    if old.is_none(){return Ok(false)}
                    return Err("Cette révision dépasse la limite sûre du direct. Le brouillon reste visible ; le texte complet reste dans Écho.".into());
                }
                if let Some(owned)=old.as_ref(){
                    let selected_range=replacement.as_ref().unwrap_or(&owned.range);
                    self.select_owned_range(&observed,&pattern,owned,&current,selected_range,budget)
                        .map_err(|error|{self.targets[index].tainted=true;error})?;
                }
                self.check_composition_focus(&observed,budget)?;
                // Last selection check immediately before the one input batch.
                let selected=single_selection(&pattern)?;
                if let Some(owned)=old.as_ref(){
                    if !same_range(&selected,replacement.as_ref().unwrap_or(&owned.range))?
                        || range_units(&owned.range,owned.text.encode_utf16().count()+1)?!=owned.text.encode_utf16().collect::<Vec<_>>() {
                        return Err("La sélection ou le brouillon a changé avant la correction.".into());
                    }
                }else if !collapsed(&selected)? || !same_range(&selected,&current)? {
                    return Err("Le curseur a changé avant l’insertion du brouillon.".into());
                }
                budget.check()?;
                let input=composition_input(&input_units);
                // From this point any failure is uncertain, even on the first
                // preview, and must prohibit fallback append or blind cleanup.
                self.targets[index].tainted=true;
                let sent=unsafe{SendInput(input.len() as u32,input.as_ptr(),std::mem::size_of::<INPUT>() as i32)};
                if sent!=input.len() as u32 {
                    return Err("Windows n’a pas confirmé toute la composition. Vérifie le champ ; le résultat reste dans Écho.".into());
                }
                loop {
                    self.check_composition_focus(&observed,budget)?;
                    let caret=single_selection(&pattern)?;
                    if collapsed(&caret)? {
                        if units.is_empty() {
                            let anchor_same=unsafe{caret.CompareEndpoints(TextPatternRangeEndpoint_Start,&anchor,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?==0;
                            // The prior selection was the nonempty owned span;
                            // deletion is acknowledged only at its saved start.
                            // Some providers leave obsolete range endpoints at
                            // numeric offsets, so do not read somebody's suffix
                            // through that obsolete range after deletion.
                            if anchor_same {
                                self.targets[index].composition=None;
                                self.targets[index].tainted=false;
                                return Ok(true);
                            }
                        }else if let Some(range)=owned_range_at_caret(&caret,&anchor,&units,text.chars().count(),budget)? {
                            self.check_composition_focus(&observed,budget)?;
                            let verified_caret=single_selection(&pattern)?;
                            let composed=Composition{text:text.into(),range};
                            verify_owned(&composed,&verified_caret)?;
                            self.targets[index].composition=if finalize{None}else{Some(composed)};
                            self.targets[index].tainted=false;
                            return Ok(true);
                        }
                    }
                    budget.check()?;
                    std::thread::sleep(Duration::from_millis(15));
                }
            })();
            // The owned span stays recorded on every error. SendInput attempts
            // stay tainted until positively re-anchored; cancellation before an
            // input batch can still discard the previously verified span.
            result
        }
        fn discard(&mut self,token:&str,budget:&Budget)->Result<bool,String>{
            budget.check()?;
            let index=self.target_index(token)?;
            if self.targets[index].composition.is_none(){return Ok(false)}
            self.compose(token,"",false,budget)
        }
    }

    fn wait_modifiers(budget:&Budget)->Result<(),String>{
        while [VK_CONTROL,VK_MENU,VK_SHIFT,VK_LWIN,VK_RWIN].iter().any(|key|unsafe{GetAsyncKeyState(*key as i32)}<0){
            budget.check()?;std::thread::sleep(Duration::from_millis(15));
        }
        budget.check()
    }
    fn single_selection(pattern:&IUIAutomationTextPattern)->Result<IUIAutomationTextRange,String>{
        let ranges=unsafe{pattern.GetSelection()}.map_err(uia_error)?;
        if unsafe{ranges.Length()}.map_err(uia_error)?!=1{return Err("Windows ne fournit pas une sélection unique sûre.".into());}
        unsafe{ranges.GetElement(0)}.map_err(uia_error)
    }
    fn collapsed(range:&IUIAutomationTextRange)->Result<bool,String>{
        Ok(unsafe{range.CompareEndpoints(TextPatternRangeEndpoint_Start,range,TextPatternRangeEndpoint_End)}.map_err(uia_error)?==0)
    }
    fn same_range(a:&IUIAutomationTextRange,b:&IUIAutomationTextRange)->Result<bool,String>{
        Ok(unsafe{a.CompareEndpoints(TextPatternRangeEndpoint_Start,b,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?==0
           && unsafe{a.CompareEndpoints(TextPatternRangeEndpoint_End,b,TextPatternRangeEndpoint_End)}.map_err(uia_error)?==0)
    }
    fn range_units(range:&IUIAutomationTextRange,limit:usize)->Result<Vec<u16>,String>{
        let value=unsafe{range.GetText(limit.min(32_001) as i32)}.map_err(uia_error)?;
        Ok(value.to_vec())
    }
    fn verify_owned(owned:&Composition,selection:&IUIAutomationTextRange)->Result<(),String>{
        let expected:Vec<u16>=owned.text.encode_utf16().collect();
        let observed=range_units(&owned.range,expected.len()+1)?;
        let at_end=unsafe{selection.CompareEndpoints(TextPatternRangeEndpoint_Start,&owned.range,TextPatternRangeEndpoint_End)}.map_err(uia_error)?==0;
        super::owned_composition_guard(&expected,&observed,collapsed(selection)?,at_end)
    }
    fn owned_range_at_caret(caret:&IUIAutomationTextRange,anchor:&IUIAutomationTextRange,units:&[u16],preferred:usize,budget:&Budget)->Result<Option<IUIAutomationTextRange>,String>{
        let Some(range)=range_at_caret(caret,units,preferred,budget)? else{return Ok(None)};
        // Reject providers that cannot preserve this insertion anchor rather
        // than claiming an unrelated matching word elsewhere in the field.
        if unsafe{range.CompareEndpoints(TextPatternRangeEndpoint_Start,anchor,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?!=0
            || unsafe{range.CompareEndpoints(TextPatternRangeEndpoint_End,caret,TextPatternRangeEndpoint_End)}.map_err(uia_error)?!=0 {
            return Ok(None);
        }
        Ok(Some(range))
    }
    fn range_at_caret(caret:&IUIAutomationTextRange,units:&[u16],preferred:usize,budget:&Budget)->Result<Option<IUIAutomationTextRange>,String>{
        let mut found=None;
        let count=super::find_character_extent(units,preferred,|count|{
            budget.check()?;
            let candidate=unsafe{caret.Clone()}.map_err(uia_error)?;
            unsafe{candidate.MoveEndpointByUnit(TextPatternRangeEndpoint_Start,TextUnit_Character,-count)}.map_err(uia_error)?;
            let text=range_units(&candidate,units.len()+1)?;
            if text==units {found=Some(candidate);}
            Ok(text)
        })?;
        if count.is_none(){return Ok(None)}
        let range=found.ok_or("Plage de composition introuvable.")?;
        Ok(Some(range))
    }
    fn changed_tail(owned:&Composition,caret:&IUIAutomationTextRange,next:&str,units:&[u16],budget:&Budget)->Result<(Option<IUIAutomationTextRange>,Vec<u16>),String>{
        // An identical final still performs a verified replacement; an empty
        // replacement selection must never become Delete on somebody's suffix.
        let prefix=if owned.text==next{0}else{super::stable_prefix_bytes(&owned.text,next)};
        if prefix==0{return Ok((None,units.to_vec()))}
        let suffix=&owned.text[prefix..];
        let candidate=if suffix.is_empty(){Some(unsafe{caret.Clone()}.map_err(uia_error)?)}
            else{range_at_caret(caret,&suffix.encode_utf16().collect::<Vec<_>>(),suffix.chars().count(),budget)?};
        if let Some(candidate)=candidate {
            let inside=unsafe{candidate.CompareEndpoints(TextPatternRangeEndpoint_Start,&owned.range,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?>=0
                && unsafe{candidate.CompareEndpoints(TextPatternRangeEndpoint_End,&owned.range,TextPatternRangeEndpoint_End)}.map_err(uia_error)?==0;
            if inside {
                let prefix_range=unsafe{owned.range.Clone()}.map_err(uia_error)?;
                unsafe{prefix_range.MoveEndpointByRange(TextPatternRangeEndpoint_End,&candidate,TextPatternRangeEndpoint_Start)}.map_err(uia_error)?;
                let expected:Vec<_>=owned.text[..prefix].encode_utf16().collect();
                if range_units(&prefix_range,expected.len()+1)?==expected {
                    return Ok((Some(candidate),next[prefix..].encode_utf16().collect()));
                }
            }
        }
        // A provider can promote Character to a larger unit. Replacing the
        // verified full owned span is the only fallback, still batch bounded.
        Ok((None,units.to_vec()))
    }
    fn composition_input(units:&[u16])->Vec<INPUT>{
        let mut input=Vec::with_capacity(units.len().max(1)*2);
        if units.is_empty(){
            // Delete is legal only after selecting the verified owned span.
            for flags in [0,KEYEVENTF_KEYUP]{input.push(INPUT{r#type:INPUT_KEYBOARD,Anonymous:INPUT_0{ki:KEYBDINPUT{wVk:VK_DELETE,wScan:0,dwFlags:flags,time:0,dwExtraInfo:0}}});}
        }else{
            for ch in units {for flags in [KEYEVENTF_UNICODE,KEYEVENTF_UNICODE|KEYEVENTF_KEYUP]{
                input.push(INPUT{r#type:INPUT_KEYBOARD,Anonymous:INPUT_0{ki:KEYBDINPUT{wVk:0,wScan:*ch,dwFlags:flags,time:0,dwExtraInfo:0}}});
            }}
        }
        input
    }

    struct ComApartment;
    impl Drop for ComApartment { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
    fn run(rx:mpsc::Receiver<Request>) {
        let initialized = unsafe { CoInitializeEx(None,COINIT_MULTITHREADED) }.ok();
        if initialized.is_err() { reject_all(rx,"L’accès aux champs Windows est indisponible. Utilise Note Écho."); return; }
        let _apartment = ComApartment;
        let automation: ::windows::core::Result<IUIAutomation2> = unsafe { CoCreateInstance(&CUIAutomation8,None,CLSCTX_INPROC_SERVER) };
        let Ok(automation) = automation else { reject_all(rx,"UI Automation est indisponible. Utilise Note Écho."); return; };
        unsafe {
            let _ = automation.SetConnectionTimeout(400);
            let _ = automation.SetTransactionTimeout(400);
            let _ = automation.SetAutoSetFocus(false);
        }
        let mut worker = Worker {automation,last:None,last_external:None,external_epoch:0,targets:VecDeque::new()};
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Request::Freeze {reply,deadline}) => { let result=worker.freeze(deadline);let _=reply.send(result); }
                Ok(Request::Insert {token,text,budget,reply}) => { let result=worker.insert(&token,&text,&budget);let _=reply.send(result); }
                Ok(Request::Preview {token,text,budget,reply}) => { let result=worker.compose(&token,&text,false,&budget);let _=reply.send(result); }
                Ok(Request::Discard {token,budget,reply}) => { let result=worker.discard(&token,&budget);let _=reply.send(result); }
                Err(mpsc::RecvTimeoutError::Timeout) => worker.observe(),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
    fn reject_all(rx:mpsc::Receiver<Request>,message:&str) {
        while let Ok(request)=rx.recv() {
            match request { Request::Freeze {reply,..}=>{let _=reply.send(Err(message.into()));}, Request::Insert {reply,..}=>{let _=reply.send(Err(message.into()));},
                Request::Preview {reply,..}|Request::Discard {reply,..}=>{let _=reply.send(Err(message.into()));} }
        }
    }
    fn sender() -> &'static mpsc::SyncSender<Request> {
        TX.get_or_init(|| { let (tx,rx)=mpsc::sync_channel(16);std::thread::spawn(move||run(rx));tx })
    }
    pub fn init() { let _=sender(); }
    pub fn cancel_pending() { CANCEL_EPOCH.fetch_add(1,Ordering::AcqRel); }
    pub fn freeze() -> Result<String,String> {
        let (tx,rx)=mpsc::channel();
        sender().try_send(Request::Freeze {reply:tx,deadline:Instant::now()+REQUEST_LIMIT}).map_err(|_|"La vérification du champ est occupée. Réessaie ou utilise Copier.".to_string())?;
        rx.recv_timeout(WAIT_LIMIT).map_err(|_|"Le champ Windows n’a pas répondu à temps. Utilise Copier ou Note Écho.".to_string())?
    }
    pub fn insert(token:String,text:String)->Result<(),String> {
        super::safe_units(&text)?;
        let (tx,rx)=mpsc::channel();let cancelled=Arc::new(AtomicBool::new(false));
        let budget=Budget {deadline:Instant::now()+REQUEST_LIMIT,cancelled:cancelled.clone(),epoch:CANCEL_EPOCH.load(Ordering::Acquire)};
        sender().try_send(Request::Insert {token,text,budget,reply:tx}).map_err(|_|"L’insertion Windows est occupée. Le texte est conservé.".to_string())?;
        match rx.recv_timeout(WAIT_LIMIT) { Ok(result)=>result,Err(_)=>{cancelled.store(true,Ordering::Release);Err("L’insertion a expiré. Le texte est conservé dans Écho.".into())} }
    }
    pub fn preview(token:String,text:String)->Result<bool,String>{
        super::composition_units(&text)?;
        composition_request(token,Some(text))
    }
    pub fn discard(token:String)->Result<bool,String>{composition_request(token,None)}
    fn composition_request(token:String,text:Option<String>)->Result<bool,String>{
        let(tx,rx)=mpsc::channel();let cancelled=Arc::new(AtomicBool::new(false));
        let budget=Budget{deadline:Instant::now()+REQUEST_LIMIT,cancelled:cancelled.clone(),epoch:CANCEL_EPOCH.load(Ordering::Acquire)};
        let request=if let Some(text)=text{Request::Preview{token,text,budget,reply:tx}}else{Request::Discard{token,budget,reply:tx}};
        sender().try_send(request).map_err(|_|"La composition Windows est occupée. Le texte reste dans Écho.".to_string())?;
        match rx.recv_timeout(WAIT_LIMIT){Ok(result)=>result,Err(_)=>{cancelled.store(true,Ordering::Release);Err("La composition n’a pas été confirmée à temps. Vérifie le champ avant de recopier le texte.".into())}}
    }

    #[cfg(test)]
    mod integration_tests {
        use super::*;
        fn qa_identity(title:&str,pid:u32){
            let current=foreground().expect("Owned QA fixture must remain foreground; no input was permitted.");
            assert!(current.1==pid,"QA PID changed; stop without writing.");
            let mut buffer=[0u16;512];
            let count=unsafe{windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(current.0 as _,buffer.as_mut_ptr(),buffer.len() as i32)};
            let expected:Vec<_>=title.encode_utf16().collect();
            assert!(count>=0 && buffer[..count as usize]==expected,"QA window title changed; stop without writing.");
        }
        fn qa_budget()->Budget{Budget{deadline:Instant::now()+REQUEST_LIMIT,cancelled:Arc::new(AtomicBool::new(false)),epoch:CANCEL_EPOCH.load(Ordering::Acquire)}}
        fn qa_text(pattern:&IUIAutomationTextPattern)->String{
            let range=unsafe{pattern.DocumentRange()}.expect("Owned QA document unavailable.");
            let units=range_units(&range,1024).expect("Owned QA text unavailable.");
            String::from_utf16(&units).expect("Owned QA fixture has invalid UTF-16.")
                .trim_end_matches(&['\r','\n'][..]).to_string()
        }
        fn qa_select(worker:&Worker,observed:&Observed,pattern:&IUIAutomationTextPattern,
            requested:&IUIAutomationTextRange,expected_text:&str,title:&str,pid:u32){
            let budget=qa_budget();
            qa_identity(title,pid);
            worker.check_composition_focus(observed,&budget).expect("QA field changed before Select.");
            assert!(qa_text(pattern)==expected_text,"QA text changed before Select.");
            let previous=single_selection(pattern).expect("QA previous selection unavailable.");
            if same_range(&previous,requested).expect("QA selection comparison failed."){return}
            unsafe{requested.Select()}.expect("Owned QA Select failed.");
            super::super::wait_for_selection_acknowledgement(
                ||{qa_identity(title,pid);worker.check_composition_focus(observed,&budget)},
                ||{
                    let selected=single_selection(pattern)?;
                    super::super::selection_acknowledged(qa_text(pattern)==expected_text,
                        same_range(&selected,requested)?,same_range(&selected,&previous)?)
                },
                ||std::thread::sleep(Duration::from_millis(15)),
            ).expect("Owned QA selection was not acknowledged; no text input attempted.");
        }
        /// Run only against a launcher-owned RichTextBox/textarea fixture.
        /// The exact foreground title and process id are mandatory capabilities.
        #[test]
        #[ignore = "Requires an explicitly owned UIA fixture and ECHO_QA_COMPOSITION_TITLE/PID."]
        fn owned_fixture_provisional_composition_roundtrip(){
            let title=std::env::var("ECHO_QA_COMPOSITION_TITLE").expect("Set the exact owned QA window title.");
            let pid: u32=std::env::var("ECHO_QA_COMPOSITION_PID").expect("Set the owned fixture PID.").parse().expect("QA PID must be numeric.");
            let webview_fixture=std::env::var("ECHO_QA_COMPOSITION_WEBVIEW2").as_deref()==Ok("1");
            assert!((title.starts_with("ECHO COMPOSITION QA - ") || webview_fixture) && !title.is_empty()
                && title.encode_utf16().count()<512 && pid>0,"Invalid QA fixture capability.");
            qa_identity(&title,pid);
            unsafe{CoInitializeEx(None,COINIT_MULTITHREADED)}.ok().expect("QA COM initialization failed.");
            let _apartment=ComApartment;
            let automation:IUIAutomation2=unsafe{CoCreateInstance(&CUIAutomation8,None,CLSCTX_INPROC_SERVER)}.expect("QA UI Automation unavailable.");
            unsafe{let _=automation.SetConnectionTimeout(400);let _=automation.SetTransactionTimeout(400);let _=automation.SetAutoSetFocus(false);}
            let mut worker=Worker{automation,last:None,last_external:None,external_epoch:0,targets:VecDeque::new()};
            let (element,_)=worker.focused_field().expect("Focus the owned QA text field.");
            if webview_fixture {
                // The launcher separately verifies the exact QA executable
                // path and workspace. Production title permissions stay intact.
                let id=unsafe{element.CurrentAutomationId()}.map(|value|value.to_string()).unwrap_or_default();
                let name=unsafe{element.CurrentName()}.map(|value|value.to_string()).unwrap_or_default();
                assert!(id=="EchoQaInput" || name=="Echo QA input","The focused field is not the explicit WebView QA fixture; no writes performed.");
            }
            let pattern=unsafe{element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)}
                .expect("Owned fixture has no TextPattern; no text writes performed.");
            assert!(qa_text(&pattern)=="Avant :  : apres","QA initial text mismatch; no text writes performed.");
            let initial=single_selection(&pattern).expect("Owned QA caret unavailable.");
            assert!(collapsed(&initial).unwrap(),"QA initial selection must be empty.");
            let prefix=unsafe{pattern.DocumentRange()}.unwrap();
            unsafe{prefix.MoveEndpointByRange(TextPatternRangeEndpoint_End,&initial,TextPatternRangeEndpoint_Start)}.unwrap();
            assert!(range_units(&prefix,20).unwrap()=="Avant : ".encode_utf16().collect::<Vec<_>>(),"QA initial caret must be after the known prefix.");
            let token=worker.freeze(Instant::now()+REQUEST_LIMIT).expect("Could not freeze owned QA field.");
            let observed=worker.targets.iter().find(|target|target.token==token).unwrap().observed.clone();

            // An initial user selection must not be replaced provisionally.
            let selection=unsafe{initial.Clone()}.unwrap();
            unsafe{selection.MoveEndpointByUnit(TextPatternRangeEndpoint_End,TextUnit_Character,1)}.unwrap();
            qa_select(&worker,&observed,&pattern,&selection,"Avant :  : apres",&title,pid);
            qa_identity(&title,pid);
            assert!(!worker.compose(&token,"ne pas remplacer",false,&qa_budget()).expect("Initial selection guard failed."));
            assert!(qa_text(&pattern)=="Avant :  : apres","Initial user selection was modified.");
            qa_select(&worker,&observed,&pattern,&initial,"Avant :  : apres",&title,pid);
            qa_identity(&title,pid);
            assert!(!worker.compose(&token,&"x".repeat(MAX_COMPOSITION_BATCH+1),false,&qa_budget()).expect("Initial batch bound failed."));
            assert!(qa_text(&pattern)=="Avant :  : apres","An oversized initial preview modified the field.");

            qa_identity(&title,pid);
            assert!(worker.compose(&token,"Bonjour",false,&qa_budget()).expect("First owned preview failed."));
            assert!(qa_text(&pattern)=="Avant : Bonjour : apres","First draft escaped its insertion span.");
            qa_identity(&title,pid);
            assert!(worker.compose(&token,"Bonjour frérot 🐕",false,&qa_budget()).expect("Unicode preview replacement failed."));
            assert!(qa_text(&pattern)=="Avant : Bonjour frérot 🐕 : apres","Unicode revision did not preserve surrounding text.");
            qa_identity(&title,pid);
            worker.insert(&token,"Bonjour frérot 🐕. ",&qa_budget()).expect("Final replacement failed.");
            let finalized="Avant : Bonjour frérot 🐕.  : apres";
            assert!(qa_text(&pattern)==finalized,"Final was duplicated or surrounding text changed.");
            assert!(worker.targets.iter().find(|target|target.token==token).unwrap().composition.is_none());

            qa_identity(&title,pid);
            assert!(worker.compose(&token,"brouillon",false,&qa_budget()).unwrap());
            assert!(worker.discard(&token,&qa_budget()).expect("Verified owned discard failed."));
            assert!(qa_text(&pattern)==finalized,"Discard changed text outside its owned span.");
            assert!(!worker.discard(&token,&qa_budget()).unwrap());

            qa_identity(&title,pid);
            assert!(worker.compose(&token,"à garder",false,&qa_budget()).unwrap());
            let before=qa_text(&pattern);
            let caret=single_selection(&pattern).unwrap();
            unsafe{caret.MoveEndpointByUnit(TextPatternRangeEndpoint_Start,TextUnit_Character,-1)}.unwrap();
            unsafe{caret.MoveEndpointByRange(TextPatternRangeEndpoint_End,&caret,TextPatternRangeEndpoint_Start)}.unwrap();
            qa_select(&worker,&observed,&pattern,&caret,&before,&title,pid);
            qa_identity(&title,pid);
            assert!(worker.compose(&token,"ne pas écraser",false,&qa_budget()).is_err());
            assert!(worker.insert(&token,"ne pas ajouter",&qa_budget()).is_err());
            assert!(worker.discard(&token,&qa_budget()).is_err());
            assert!(qa_text(&pattern)==before,"A moved caret allowed a destructive retry.");
            println!("{{\"qa\":\"owned-provisional-composition\",\"passed\":true,\"checks\":9}}");
        }
    }
}

/// Pure policy seams also tested on non-Windows hosts.
fn with_revalidated_focus<T>(
    wait:impl FnOnce()->Result<(),String>,
    validate:impl FnOnce()->Result<(),String>,
    insert:impl FnOnce()->Result<T,String>,
)->Result<T,String>{
    wait()?;
    validate()?;
    insert()
}
fn foreground_allowed(current:isize,current_pid:u32,target:isize,target_pid:u32,own_pid:u32)->bool {
    current!=0 && target!=0 && target_pid!=0
        && ((current==target && current_pid==target_pid) || current_pid==own_pid)
}
fn safe_units(text:&str)->Result<Vec<u16>,String> {
    if text.trim().is_empty(){return Err("La transcription est vide.".into());}
    if text.chars().any(char::is_control){return Err("Ce texte contient des retours à la ligne ou des touches de contrôle. Utilise Copier pour le coller sans simuler Entrée.".into());}
    let units:Vec<_>=text.encode_utf16().collect();
    if units.len()>32_000 {return Err("Ce texte est trop long pour l’insertion directe. Utilise Copier.".into());}
    Ok(units)
}
fn composition_units(text:&str)->Result<Vec<u16>,String>{
    if text.is_empty(){Ok(Vec::new())}else{safe_units(text)}
}
fn composition_needs_write(previous:Option<&str>,next:&str,finalize:bool)->bool{
    finalize || previous!=Some(next)
}
fn selection_acknowledged(owned_text_matches:bool,selection_expected:bool,selection_previous:bool)->Result<bool,String>{
    if !owned_text_matches || (!selection_expected && !selection_previous) {
        return Err("La sélection ou le brouillon a changé. Aucune correction n’est envoyée.".into());
    }
    Ok(selection_expected)
}
/// Check the deadline and destination on each poll, then again after the
/// expected selection is observed. Only the probe may allow a prior selection.
fn wait_for_selection_acknowledgement(
    mut check:impl FnMut()->Result<(),String>,mut probe:impl FnMut()->Result<bool,String>,mut pause:impl FnMut(),
)->Result<(),String>{
    loop {
        check()?;
        if probe()? {check()?;return Ok(())}
        pause();
    }
}
fn stable_prefix_bytes(previous:&str,next:&str)->usize{
    let mut boundary=0;
    for ((index,a),b) in previous.char_indices().zip(next.chars()) {
        if a!=b{break}
        if a.is_whitespace(){boundary=index+a.len_utf8();}
    }
    boundary
}
fn owned_composition_guard(expected:&[u16],observed:&[u16],selection_collapsed:bool,at_end:bool)->Result<(),String>{
    if expected!=observed || !selection_collapsed || !at_end {
        Err("Le texte ou le curseur a changé dans le champ. Écho conserve le brouillon et n’efface rien.".into())
    }else{Ok(())}
}
/// UIA Character can mean UTF-16 units, scalar values or grapheme clusters.
/// Search by observed UTF-16 length, bounded to 17 probes and never past the
/// exact target length plus one. A provider promoting Character to Word fails
/// closed when it cannot expose the exact inserted span.
fn find_character_extent(expected:&[u16],preferred:usize,mut probe:impl FnMut(i32)->Result<Vec<u16>,String>)->Result<Option<i32>,String>{
    if expected.is_empty() || expected.len()>32_000{return Ok(None)}
    let(mut low,mut high)=(1usize,expected.len());
    let mut count=preferred.clamp(low,high);
    for _ in 0..17 {
        let observed=probe(count as i32)?;
        if observed==expected{return Ok(Some(count as i32))}
        match observed.len().cmp(&expected.len()) {
            std::cmp::Ordering::Less=>low=count+1,
            std::cmp::Ordering::Greater=>high=count-1,
            std::cmp::Ordering::Equal=>return Ok(None),
        }
        if low>high{return Ok(None)}
        count=low+(high-low)/2;
    }
    Ok(None)
}

pub fn freeze_cursor()->Result<String,String> {
    #[cfg(windows)]{native::freeze()}
    #[cfg(not(windows))]{Err("La dictée vers une autre application est disponible sur Windows.".into())}
}
#[tauri::command]
pub async fn get_dictation_target()->Result<String,String> {
    tauri::async_runtime::spawn_blocking(freeze_cursor).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn insert_dictation_text(target:String,text:String)->Result<(),String> {
    tauri::async_runtime::spawn_blocking(move||insert_at_cursor(target,text)).await.map_err(|e|e.to_string())?
}
pub fn insert_at_cursor(target:String,text:String)->Result<(),String> {
    #[cfg(windows)]{native::insert(target,text)}
    #[cfg(not(windows))]{let _=(target,text);Err("La dictée vers une autre application est disponible sur Windows.".into())}
}
/// False means provisional composition is unsupported before any text write
/// (including an initial nonempty user selection). Errors never permit append.
pub fn preview_at_cursor(target:String,text:String)->Result<bool,String>{
    #[cfg(windows)]{native::preview(target,text)}
    #[cfg(not(windows))]{let _=(target,text);Ok(false)}
}
/// Remove only the verified owned provisional span. False means none is owned;
/// tainted targets return an error even if their first span was not re-anchored.
pub fn discard_preview_at_cursor(target:String)->Result<bool,String>{
    #[cfg(windows)]{native::discard(target)}
    #[cfg(not(windows))]{let _=target;Ok(false)}
}
pub fn init(){#[cfg(windows)]native::init();}
/// Call on voice cancellation/shutdown before waiting for a delivery worker.
pub fn cancel_pending(){#[cfg(windows)]native::cancel_pending();}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn field_or_process_changed_during_modifier_wait_prevents_insertion(){
        use std::cell::Cell;
        let observed=(1,100);
        for (after_wait,allowed) in [((1,100),true),((2,100),false),((1,200),false)] {
            let focus=Cell::new(observed);let inserted=Cell::new(false);
            let result=with_revalidated_focus(
                || {focus.set(after_wait);Ok(())},
                || if focus.get()==observed {Ok(())} else {Err("Le champ ou son processus a changé.".into())},
                || {inserted.set(true);Ok(())},
            );
            assert_eq!(result.is_ok(),allowed);
            assert_eq!(inserted.get(),allowed,"A changed destination must not receive text");
        }
    }
    #[test]fn cancelled_modifier_wait_never_validates_or_inserts(){
        use std::cell::Cell;
        let validated=Cell::new(false);let inserted=Cell::new(false);
        let result=with_revalidated_focus(
            || Err("Insertion annulée.".into()),
            || {validated.set(true);Ok(())},
            || {inserted.set(true);Ok(())},
        );
        assert!(result.is_err());assert!(!validated.get());assert!(!inserted.get());
    }
    #[test]fn cannot_target_another_external_window_or_reused_handle(){
        assert!(foreground_allowed(10,100,10,100,999));
        assert!(foreground_allowed(20,999,10,100,999));
        assert!(!foreground_allowed(20,200,10,100,999));
        assert!(!foreground_allowed(10,200,10,100,999));
        assert!(foreground_allowed(10,999,10,999,999)); // editable own-app fields are allowed too
        assert!(!foreground_allowed(0,999,10,999,999));
    }
    #[test]fn no_synthesized_enter_tab_or_control_characters(){
        assert!(safe_units("Bonjour\r\nEnvoyer").is_err());assert!(safe_units("Bonjour\tEnvoyer").is_err());assert!(safe_units("\u{0}").is_err());
        assert_eq!(String::from_utf16(&safe_units("Écho reçoit les accents et 🐕.").unwrap()).unwrap(),"Écho reçoit les accents et 🐕.");
    }
    #[test]fn identical_preview_skips_but_identical_final_replaces_owned_span(){
        assert!(!composition_needs_write(Some("bonjour"),"bonjour",false));
        assert!(composition_needs_write(Some("bonjour"),"bonjour",true));
        assert!(composition_needs_write(Some("bonjou"),"bonjour",false));
        assert!(composition_needs_write(Some("bonjour"),"",false));
        assert!(composition_units("").unwrap().is_empty());
        assert!(composition_units("\r").is_err());
    }
    #[test]fn changed_text_selection_or_caret_cannot_authorize_replacement(){
        let owned:Vec<_>="Écho 🐕".encode_utf16().collect();
        assert!(owned_composition_guard(&owned,&owned,true,true).is_ok());
        assert!(owned_composition_guard(&owned,&"modifié".encode_utf16().collect::<Vec<_>>(),true,true).is_err());
        assert!(owned_composition_guard(&owned,&owned,false,true).is_err());
        assert!(owned_composition_guard(&owned,&owned,true,false).is_err());
    }
    #[test]fn delayed_select_accepts_only_previous_caret_then_exact_range(){
        use std::cell::Cell;
        let checks=Cell::new(0);let probes=Cell::new(0);let pauses=Cell::new(0);
        let mut states=[(true,false,true),(true,false,true),(true,true,false)].into_iter();
        wait_for_selection_acknowledgement(
            ||{checks.set(checks.get()+1);Ok(())},
            ||{
                probes.set(probes.get()+1);
                let(text,expected,previous)=states.next().expect("Must stop after the exact acknowledgement.");
                selection_acknowledged(text,expected,previous)
            },
            ||pauses.set(pauses.get()+1),
        ).unwrap();
        assert_eq!(probes.get(),3);assert_eq!(pauses.get(),2);
        assert_eq!(checks.get(),4,"The destination is checked again after the acknowledgement.");
    }
    #[test]fn selection_ack_rejects_changed_text_or_unexpected_selection(){
        for(text,expected,previous)in [(false,false,true),(false,true,false),(true,false,false)]{
            let result=wait_for_selection_acknowledgement(
                ||Ok(()),||selection_acknowledged(text,expected,previous),
                ||panic!("An ownership mismatch must not be retried."),
            );
            assert!(result.is_err());
        }
    }
    #[test]fn selection_ack_deadline_or_focus_loss_never_authorizes_input(){
        use std::cell::Cell;
        for acknowledged in [false,true]{
            let checks=Cell::new(0);let probes=Cell::new(0);let authorized=Cell::new(false);
            let limit=if acknowledged{2}else{3};
            let result=wait_for_selection_acknowledgement(
                ||{
                    checks.set(checks.get()+1);
                    if checks.get()>=limit{Err("Deadline or focus changed.".into())}else{Ok(())}
                },
                ||{probes.set(probes.get()+1);selection_acknowledged(true,acknowledged,!acknowledged)},
                ||{},
            ).map(|()|authorized.set(true));
            assert!(result.is_err());assert!(!authorized.get());
            assert_eq!(probes.get(),if acknowledged{1}else{2});
        }
    }
    #[test]fn changed_tail_preserves_only_complete_common_words(){
        let old="Bonjour frérot, je vais bien";let new="Bonjour frérot, je vais mieux";
        let boundary=stable_prefix_bytes(old,new);
        assert_eq!(&old[..boundary],"Bonjour frérot, je vais ");
        assert_eq!(&new[boundary..],"mieux");
        assert_eq!(stable_prefix_bytes("é\u{301}","é\u{300}"),0);
        let old="Salut 👨‍👩‍👧‍👦";let new="Salut 👨‍👩‍👦";
        assert_eq!(&old[..stable_prefix_bytes(old,new)],"Salut ");
    }
    #[test]fn character_extent_handles_utf16_scalars_and_graphemes(){
        let text="é e\u{301} 🐕";
        let expected:Vec<_>=text.encode_utf16().collect();
        for lengths in [vec![1usize;expected.len()],vec![1,1,1,1,1,2],vec![1,1,2,1,2]] {
            let mut calls=0;
            let found=find_character_extent(&expected,text.chars().count(),|count|{
                calls+=1;
                let size=lengths.iter().rev().take(count as usize).sum::<usize>();
                Ok(expected[expected.len().saturating_sub(size)..].to_vec())
            }).unwrap();
            assert!(found.is_some());assert!(calls<=17);
        }
    }
    #[test]fn oversized_provider_units_or_different_suffix_do_not_claim_ownership(){
        let expected:Vec<_>="mot".encode_utf16().collect();
        assert!(find_character_extent(&expected,3,|_|Ok("un autre mot".encode_utf16().collect())).unwrap().is_none());
        assert!(find_character_extent(&expected,3,|_|Ok("non".encode_utf16().collect())).unwrap().is_none());
        assert!(find_character_extent(&vec![1;32_001],1,|_|panic!("must not probe unbounded text")).unwrap().is_none());
    }
}
