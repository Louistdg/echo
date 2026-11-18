//! Targeted Windows dictation. UI Automation checks the actual editable control,
//! rather than treating every foreground window as a text field. No field content
//! is read and the clipboard is never modified.

#[cfg(windows)]
mod native {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, OnceLock};
    use std::time::{Duration, Instant};

    use ::windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use ::windows::Win32::System::Variant::VARIANT;
    use ::windows::Win32::UI::Accessibility::{CUIAutomation8, IUIAutomation2, IUIAutomationElement,
        IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_ComboBoxControlTypeId,
        UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId,
        UIA_TextPatternId, UIA_ValuePatternId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0,
        INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_BACK, VK_CONTROL, VK_MENU,
        VK_SHIFT, VK_LWIN, VK_RWIN};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow,
        GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow};

    const POLL: Duration = Duration::from_millis(150);
    const REQUEST_LIMIT: Duration = Duration::from_millis(1400);
    const WAIT_LIMIT: Duration = Duration::from_millis(1600);
    const TARGET_TTL: Duration = Duration::from_secs(5 * 3600);
    const MAX_TARGETS: usize = 32;
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
        Live { token: String, text: String, restore: bool },
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
    struct Target { token: String, observed: Observed, created: Instant, tainted: bool, live: String }
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
            self.targets.push_back(Target { token:token.clone(), observed, created:Instant::now(), tainted:false, live:String::new() });
            Ok(token)
        }
        fn backspaces(count:usize,input:&mut Vec<INPUT>){
            for _ in 0..count {
                for flags in [0,KEYEVENTF_KEYUP] {
                    input.push(INPUT { r#type:INPUT_KEYBOARD, Anonymous:INPUT_0 { ki:KEYBDINPUT {
                        wVk:VK_BACK,wScan:0,dwFlags:flags,time:0,dwExtraInfo:0,
                    } } });
                }
            }
        }
        fn type_units(units:&[u16],input:&mut Vec<INPUT>){
            for ch in units {
                for flags in [KEYEVENTF_UNICODE,KEYEVENTF_UNICODE|KEYEVENTF_KEYUP] {
                    input.push(INPUT { r#type:INPUT_KEYBOARD, Anonymous:INPUT_0 { ki:KEYBDINPUT {
                        wVk:0,wScan:*ch,dwFlags:flags,time:0,dwExtraInfo:0,
                    } } });
                }
            }
        }
        /// Sends one batch (erase + type) to the aimed window, live or final.
        /// Returns false without changing state when nothing was accepted.
        fn send_batch(&mut self, index:usize, window:(isize,u32), backspace:usize, units:&[u16]) -> Result<(),String> {
            let mut input = Vec::<INPUT>::with_capacity(backspace*2 + units.len()*2);
            Self::backspaces(backspace,&mut input);
            Self::type_units(units,&mut input);
            if input.is_empty() { return Ok(()); }
            if foreground() != Some(window) || unsafe { IsWindow(window.0 as _) } == 0 {
                return Err("Le focus a changé avant l’insertion. Le texte reste dans Écho.".into());
            }
            // One batch, no Enter key and no clipboard replacement. SendInput
            // confirms queueing only; it cannot acknowledge application storage.
            let count = unsafe { SendInput(input.len() as u32,input.as_ptr(),std::mem::size_of::<INPUT>() as i32) };
            if count != input.len() as u32 {
                if count > 0 { self.targets[index].tainted = true; }
                return Err(if count == 0 {
                    "Windows a refusé l’insertion (application éventuellement lancée en administrateur). Le texte reste dans Écho.".into()
                } else {
                    "Insertion partielle possible : vérifie le champ avant de copier le résultat. Le texte reste dans Écho.".into()
                });
            }
            Ok(())
        }
        fn frozen_field_focused(&mut self, observed:&Observed) -> bool {
            let Ok((focus,_)) = self.focused_field() else { return false };
            unsafe { self.automation.CompareElements(&observed.element,&focus) }.map(|b|b.as_bool()).unwrap_or(false)
        }
        /// Live preview, Win+H style: the provisional text is typed into the
        /// frozen field while the user speaks. Strict on purpose: it writes
        /// only when the frozen field actually has the focus, and only the
        /// changed tail is erased and typed again. Dropped silently when the
        /// field is not reachable — the final delivery stays authoritative.
        fn live(&mut self, token:&str, text:&str, restore:bool) {
            let Some(index) = self.targets.iter().position(|t| t.token == token) else { return };
            let target = &self.targets[index];
            if target.tainted || target.created.elapsed() >= TARGET_TTL { return; }
            let observed = target.observed.clone();
            if !still_exists(&observed) { return; }
            if foreground() != Some((observed.window,observed.window_pid)) {
                if !restore { return; }
                if unsafe { SetForegroundWindow(observed.window as _) } == 0 { return; }
                std::thread::sleep(Duration::from_millis(75));
            }
            if !self.frozen_field_focused(&observed) { return; }
            if [VK_CONTROL,VK_MENU,VK_SHIFT,VK_LWIN,VK_RWIN].iter().any(|key| unsafe { GetAsyncKeyState(*key as i32) } < 0) { return; }
            let (erase,tail) = { let (e,t) = super::live_diff(&self.targets[index].live, text); (e, t.to_string()) };
            if erase == 0 && tail.is_empty() { return; }
            let units = if tail.is_empty() { Vec::new() } else {
                match super::safe_units(&tail) { Ok(units)=>units, Err(_)=>return }
            };
            if self.send_batch(index,(observed.window,observed.window_pid),erase,&units).is_ok() {
                self.targets[index].live = text.to_string();
            }
        }
        fn insert(&mut self, token:&str, text:&str, budget:&Budget) -> Result<(),String> {
            budget.check()?;
            let Some(index) = self.targets.iter().position(|t| t.token == token) else {
                return Err("Cette cible appartient à une ancienne session. Le texte est conservé : utilise Copier ou Note Écho.".into());
            };
            let target = &self.targets[index];
            if target.tainted || target.created.elapsed() >= TARGET_TTL {
                return Err("Vérifie le champ avant de recopier ce résultat. Écho refuse une seconde insertion automatique incertaine.".into());
            }
            let observed = target.observed.clone();
            let live = target.live.clone();
            let Some(current) = foreground() else {
                return Err("Aucune fenêtre active ; le texte reste dans Écho.".into());
            };
            self.update_external(current);
            // Comme la dictée Windows (Win+H), le texte suit le curseur : la
            // fenêtre gelée au départ reste préférée tant qu'elle existe et que
            // le focus n'a pas rejoint une autre application ; sinon la fenêtre
            // active actuelle devient la destination, sans demander de choix.
            // Si un texte provisoire a déjà été tapé (direct), il doit être
            // remplacé là où il se trouve : la fenêtre gelée prime alors.
            let prefer_frozen = still_exists(&observed)
                && (!live.is_empty()
                    || (observed.external_epoch == self.external_epoch
                        && super::foreground_allowed(current.0, current.1, observed.window, observed.window_pid, std::process::id())));
            let aim = if prefer_frozen { (observed.window, observed.window_pid) } else { current };
            if current != aim {
                budget.check()?;
                if unsafe { SetForegroundWindow(aim.0 as _) } == 0 {
                    return Err("Windows n’a pas rendu le focus au champ. Le texte reste dans Écho.".into());
                }
                std::thread::sleep(Duration::from_millis(75));
            }
            budget.check()?;
            // Sécurité conservée : jamais de texte vers une zone non modifiable,
            // un mot de passe ou un champ en lecture seule. Le champ réellement
            // actif dans la fenêtre visée décide, même si la ligne a changé.
            let (_focus, _focus_pid) = self.focused_field()?;
            if !live.is_empty() && !self.frozen_field_focused(&observed) {
                // Le provisoire à remplacer est dans le champ gelé : on s'y
                // replace explicitement plutôt que de dupliquer le texte.
                let _ = unsafe { observed.element.SetFocus() };
                std::thread::sleep(Duration::from_millis(40));
                if !self.frozen_field_focused(&observed) {
                    return Err("Le champ qui contient le texte provisoire n’est plus accessible. Le texte reste dans Écho.".into());
                }
            }
            // Do not inject while Ctrl/Alt/Shift/Win is held. Usually the speech
            // decoder already gives the user time to release the PTT chord.
            while [VK_CONTROL,VK_MENU,VK_SHIFT,VK_LWIN,VK_RWIN].iter().any(|key| unsafe { GetAsyncKeyState(*key as i32) } < 0) {
                budget.check()?;
                std::thread::sleep(Duration::from_millis(15));
            }
            let units = super::safe_units(text)?;
            let erase = live.chars().count();
            budget.check()?;
            self.send_batch(index, aim, erase, &units)?;
            self.targets[index].live.clear();
            Ok(())
        }
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
                Ok(Request::Live {token,text,restore}) => worker.live(&token,&text,restore),
                Err(mpsc::RecvTimeoutError::Timeout) => worker.observe(),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
    fn reject_all(rx:mpsc::Receiver<Request>,message:&str) {
        while let Ok(request)=rx.recv() {
            match request { Request::Freeze {reply,..}=>{let _=reply.send(Err(message.into()));}, Request::Insert {reply,..}=>{let _=reply.send(Err(message.into()));}, Request::Live {..}=>{} }
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
    /// Fire-and-forget live preview: a full channel simply drops the update,
    /// the next preview tick re-synchronizes from what was actually typed.
    pub fn live(token:String,text:String,restore:bool) {
        let _=sender().try_send(Request::Live {token,text,restore});
    }
}

/// Pure policy seams also tested on non-Windows hosts.
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
/// Diff for live replacement: the shared prefix stays typed, only the changed
/// tail is erased (one Backspace per character) and typed again.
fn live_diff<'a>(typed:&str,next:&'a str)->(usize,&'a str){
    let mut end=0;
    for ((offset,a),b) in typed.char_indices().zip(next.chars()){ if a!=b{break} end=offset+a.len_utf8(); }
    (typed[end..].chars().count(),&next[end..])
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
/// Aperçu en direct vers le champ gelé, façon Win+H. Sans réponse : un canal
/// occupé abandonne la mise à jour, le prochain aperçu resynchronise.
pub fn live_update(target:String,text:String,restore:bool) {
    #[cfg(windows)]{native::live(target,text,restore)}
    #[cfg(not(windows))]{let _=(target,text,restore);}
}
pub fn init(){#[cfg(windows)]native::init();}
/// Call on voice cancellation/shutdown before waiting for a delivery worker.
pub fn cancel_pending(){#[cfg(windows)]native::cancel_pending();}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]fn live_diff_keeps_the_shared_prefix_and_erases_only_the_tail(){
        assert_eq!(live_diff("","Bonjour"),(0,"Bonjour"));
        assert_eq!(live_diff("Bonjour","Bonjour"),(0,""));
        assert_eq!(live_diff("Bonjour le","Bonjour le monde"),(0," monde"));
        assert_eq!(live_diff("Bonjour le mondes","Bonjour le monde"),(1,""));
        assert_eq!(live_diff("Bonjour toute","Bonjour tout le monde"),(1," le monde"));
        assert_eq!(live_diff("allô alo","allô allô"),(1,"lô"));
        assert_eq!(live_diff("🐕 ab","🐈 ab"),(4,"🐈 ab"));
        assert_eq!(live_diff("C’est lundir soir","C’est lundi soir"),(6," soir"));
        assert_eq!(live_diff("abc",""),(3,""));
    }
}
