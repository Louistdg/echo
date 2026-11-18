//! State for previewing and finalizing live dictation.

#[derive(Default, Debug)]
pub struct TextPresentation {
    session: Option<String>,
    capture_open: bool,
    pub partial: String,
    pub finalizing_id: Option<String>,
    pub finalizing_text: String,
    pub last_sent_text: String,
    pub sent_count: usize,
}

impl TextPresentation {
    pub fn begin_session(&mut self, session: &str) {
        self.session = Some(session.into());
        self.capture_open = true;
        self.partial.clear();
        self.last_sent_text.clear();
        self.sent_count = 0;
        // An older accepted job can still be finalizing for its frozen target.
        // Its separate finalization row must not overwrite the new preview.
    }
    pub fn preview(&mut self, session: &str, text: String) -> bool {
        if !self.capture_open || self.session.as_deref() != Some(session) { return false; }
        self.partial = text;
        true
    }
    pub fn clear_preview(&mut self) { self.partial.clear(); }
    pub fn end_capture(&mut self) { self.capture_open = false; }
    pub fn begin_final(&mut self, id: &str) {
        self.finalizing_id = Some(id.into());
        self.finalizing_text.clear();
    }
    pub fn final_progress(&mut self, id: &str, text: &str) -> bool {
        if self.finalizing_id.as_deref() != Some(id) { return false; }
        self.finalizing_text = text.into();
        true
    }
    pub fn end_final(&mut self, id: &str) {
        if self.finalizing_id.as_deref() == Some(id) {
            self.finalizing_id = None;
            self.finalizing_text.clear();
        }
    }
    pub fn sent(&mut self, session: &str, action: &str, text: &str) -> bool {
        if !matches!(action, "auto" | "insert") { return false; }
        // A manually recovered insertion is also an actual delivery.
        if self.session.is_none() { self.session = Some(session.into()); }
        if self.session.as_deref() != Some(session) { return false; }
        self.last_sent_text = text.into();
        self.sent_count = self.sent_count.saturating_add(1);
        true
    }
    pub fn cancel(&mut self) {
        self.capture_open = false;
        self.partial.clear();
        self.finalizing_id = None;
        self.finalizing_text.clear();
        // Cancellation cannot undo phrases already delivered successfully.
    }
}

/// A candidate belongs to one VAD phrase, not merely to an open microphone.
/// A native mutation is serialized by the caller and rechecked with this ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineTicket {
    pub session: String,
    pub epoch: u64,
    pub generation: u64,
    pub phrase: String,
    pub text: String,
}

#[derive(Default, Debug)]
pub struct InlinePreview {
    session: Option<String>,
    epoch: u64,
    generation: u64,
    capture_open: bool,
    speech_phrase: Option<String>,
    disabled: bool,
    pub supported: bool,
    draft: Option<InlineTicket>,
    verified: bool,
    guard_failed: bool,
}

impl InlinePreview {
    pub fn begin_session(&mut self, session: &str, epoch: u64) {
        self.session = Some(session.into());
        self.epoch = epoch;
        self.capture_open = true;
        self.speech_phrase = None;
        self.disabled = false;
        self.supported = false;
        // A previous accepted finale still owns its draft until it commits.
    }
    pub fn next_phrase(&mut self, session: &str, epoch: u64, generation: u64) {
        if self.session.as_deref() != Some(session) || self.epoch != epoch { return; }
        self.generation = generation;
        self.speech_phrase = None;
    }
    pub fn confirm_speech(&mut self, session: &str, epoch: u64, phrase: &str) -> bool {
        if !self.capture_open || self.session.as_deref() != Some(session) || self.epoch != epoch { return false; }
        self.speech_phrase = Some(phrase.into());
        true
    }
    pub fn propose(&self, session: &str, epoch: u64, generation: u64, text: &str, final_pending: bool, speech_snapshot: Option<&str>) -> Option<InlineTicket> {
        let phrase = self.speech_phrase.as_ref()?;
        if speech_snapshot != Some(phrase.as_str()) { return None; }
        let ticket = InlineTicket { session: session.into(), epoch, generation, phrase: phrase.clone(), text: text.trim().into() };
        self.accepts(&ticket, epoch, final_pending).then_some(ticket)
    }
    pub fn accepts(&self, ticket: &InlineTicket, current_epoch: u64, final_pending: bool) -> bool {
        self.capture_open && !self.disabled && !final_pending && !ticket.text.is_empty()
            && self.session.as_deref() == Some(ticket.session.as_str())
            && self.epoch == ticket.epoch && ticket.epoch == current_epoch
            && self.generation == ticket.generation
            && self.speech_phrase.as_deref() == Some(ticket.phrase.as_str())
            && self.draft.as_ref().is_none_or(|draft| {
                draft.session == ticket.session && draft.phrase == ticket.phrase && draft.text != ticket.text
            })
    }
    pub fn applied(&mut self, ticket: InlineTicket) {
        // Stop/cancel may have been requested during the native transaction.
        // Keep ownership even then so its completion can be removed or replaced.
        if self.session.as_deref() == Some(ticket.session.as_str()) { self.supported = true; }
        self.draft = Some(ticket);
        self.verified = true;
        self.guard_failed = false;
    }
    pub fn unsupported(&mut self) { self.disabled = true; self.supported = false; }
    pub fn rejected(&mut self, ticket: InlineTicket) {
        self.disabled = true;
        self.draft = Some(ticket);
        self.verified = false;
        self.guard_failed = true;
    }
    pub fn end_capture(&mut self) { self.capture_open = false; }
    pub fn active(&self) -> bool { self.draft.is_some() && self.verified }
    pub fn draft(&self) -> Option<&InlineTicket> { self.draft.as_ref() }
    pub fn matches(&self, session: &str, phrase: &str) -> bool {
        self.draft.as_ref().is_some_and(|draft| draft.session == session && draft.phrase == phrase)
    }
    pub fn can_commit(&self, session: &str, phrase: &str) -> bool {
        self.draft.as_ref().is_none_or(|draft| draft.session != session || (draft.phrase == phrase && !self.guard_failed))
    }
    pub fn resolved(&mut self, session: &str, phrase: &str) {
        if self.matches(session, phrase) {
            self.draft = None;
            self.verified = false;
            self.guard_failed = false;
        }
    }
}

/// On an engine failure a provisional draft may be the only remaining text.
/// Its durable recovery must succeed before removing it from the user's field.
pub fn preserve_before_removal<T, E>(preserve: impl FnOnce() -> Result<(), E>, remove: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    preserve()?;
    remove()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_recovery_save_never_removes_the_only_visible_draft() {
        let removed = std::cell::Cell::new(false);
        let result = preserve_before_removal(|| Err("disk full"), || { removed.set(true); Ok(()) });
        assert_eq!(result, Err("disk full"));
        assert!(!removed.get());
    }

    #[test]
    fn engine_failure_saves_recovery_before_removing_owned_text() {
        let order = std::cell::RefCell::new(vec![]);
        let result: Result<(), &str> = preserve_before_removal(
            || { order.borrow_mut().push("durable recovery"); Ok(()) },
            || { order.borrow_mut().push("remove owned draft"); Ok(()) },
        );
        assert!(result.is_ok());
        assert_eq!(*order.borrow(), ["durable recovery", "remove owned draft"]);
    }

    fn inline_phrase() -> InlinePreview {
        let mut inline = InlinePreview::default();
        inline.begin_session("session", 7);
        inline.next_phrase("session", 7, 10);
        assert!(inline.confirm_speech("session", 7, "phrase-1"));
        inline
    }

    #[test]
    fn inline_requires_vad_speech_and_rechecks_session_epoch_phrase_and_queue() {
        let mut inline = InlinePreview::default();
        inline.begin_session("session", 7);
        inline.next_phrase("session", 7, 10);
        assert!(inline.propose("session", 7, 10, "Bruit possible", false, Some("phrase-1")).is_none());
        assert!(!inline.confirm_speech("old", 7, "old-phrase"));
        assert!(inline.confirm_speech("session", 7, "phrase-1"));
        assert!(inline.propose("session", 7, 10, "Ancien bruit", false, None).is_none());
        let ticket = inline.propose("session", 7, 10, "Bonjour", false, Some("phrase-1")).unwrap();
        assert!(!inline.accepts(&ticket, 8, false));
        assert!(!inline.accepts(&ticket, 7, true));
        inline.next_phrase("session", 7, 11);
        assert!(inline.confirm_speech("session", 7, "phrase-2"));
        assert!(!inline.accepts(&ticket, 7, false));
        inline.begin_session("new-session", 7);
        assert!(!inline.accepts(&ticket, 7, false));
    }

    #[test]
    fn identical_progress_and_result_do_not_write_twice() {
        let mut inline = inline_phrase();
        let ticket = inline.propose("session", 7, 10, " Bonjour ", false, Some("phrase-1")).unwrap();
        inline.applied(ticket.clone());
        assert!(inline.active());
        assert!(inline.supported);
        assert!(!inline.accepts(&ticket, 7, false));
        assert!(inline.propose("session", 7, 10, "Bonjour", false, Some("phrase-1")).is_none());
        assert!(inline.propose("session", 7, 10, "Bonjour à tous", false, Some("phrase-1")).is_some());
    }

    #[test]
    fn unsupported_field_keeps_final_only_delivery_and_new_target_can_be_probed() {
        let mut inline = inline_phrase();
        inline.unsupported();
        assert!(!inline.supported);
        assert!(!inline.active());
        assert!(inline.can_commit("session", "phrase-1"));
        assert!(inline.propose("session", 7, 10, "Bonjour", false, Some("phrase-1")).is_none());
        inline.begin_session("next", 7);
        inline.next_phrase("next", 7, 12);
        inline.confirm_speech("next", 7, "next-phrase");
        assert!(inline.propose("next", 7, 12, "Bonjour", false, Some("next-phrase")).is_some());
    }

    #[test]
    fn phrase_boundary_keeps_owned_draft_and_later_final_cannot_clear_new_phrase() {
        let mut inline = inline_phrase();
        let first = inline.propose("session", 7, 10, "Un aperçu", false, Some("phrase-1")).unwrap();
        inline.applied(first);
        inline.next_phrase("session", 7, 11);
        inline.confirm_speech("session", 7, "phrase-2");
        assert!(inline.active());
        assert!(inline.can_commit("session", "phrase-1"));
        assert!(!inline.can_commit("session", "phrase-2"));
        assert!(inline.propose("session", 7, 11, "Deuxième", false, Some("phrase-2")).is_none());
        inline.resolved("session", "phrase-1");
        let second = inline.propose("session", 7, 11, "Deuxième", false, Some("phrase-2")).unwrap();
        inline.applied(second);
        inline.resolved("session", "phrase-1");
        assert!(inline.matches("session", "phrase-2"));
    }

    #[test]
    fn stop_or_cancel_during_native_write_keeps_ownership_for_cleanup() {
        let mut inline = inline_phrase();
        let ticket = inline.propose("session", 7, 10, "Brouillon", false, Some("phrase-1")).unwrap();
        inline.end_capture();
        assert!(!inline.accepts(&ticket, 7, false));
        inline.applied(ticket);
        assert!(inline.active());
        assert!(inline.can_commit("session", "phrase-1"));
        inline.resolved("session", "phrase-1");
        assert!(!inline.active());
        assert!(inline.draft().is_none());
    }

    #[test]
    fn guard_failure_retains_candidate_and_cannot_fall_back_to_append() {
        let mut inline = inline_phrase();
        let ticket = inline.propose("session", 7, 10, "Texte à récupérer", false, Some("phrase-1")).unwrap();
        inline.rejected(ticket);
        assert!(!inline.active());
        assert_eq!(inline.draft().unwrap().text, "Texte à récupérer");
        assert!(!inline.can_commit("session", "phrase-1"));
        assert!(!inline.can_commit("session", "phrase-2"));
        assert!(inline.propose("session", 7, 10, "Encore", false, Some("phrase-1")).is_none());
        inline.resolved("session", "phrase-1");
        assert!(inline.draft().is_none());
    }

    #[test]
    fn previous_session_final_keeps_its_draft_while_new_capture_waits() {
        let mut inline = inline_phrase();
        let ticket = inline.propose("session", 7, 10, "Ancienne phrase", false, Some("phrase-1")).unwrap();
        inline.applied(ticket);
        inline.end_capture();
        inline.begin_session("next", 7);
        inline.next_phrase("next", 7, 12);
        inline.confirm_speech("next", 7, "next-phrase");
        assert!(inline.propose("next", 7, 12, "Nouvelle phrase", true, Some("next-phrase")).is_none());
        inline.resolved("session", "phrase-1");
        assert!(inline.propose("next", 7, 12, "Nouvelle phrase", false, Some("next-phrase")).is_some());
    }

    #[test]
    fn final_progress_never_overwrites_the_current_preview() {
        let mut text = TextPresentation::default();
        text.begin_session("session");
        text.begin_final("phrase-1");
        assert!(text.preview("session", "La deuxième phrase commence".into()));
        assert!(text.final_progress("phrase-1", ""));
        assert_eq!(text.partial, "La deuxième phrase commence");
        assert!(text.final_progress("phrase-1", "La première phrase."));
        assert_eq!(text.finalizing_text, "La première phrase.");
        assert_eq!(text.partial, "La deuxième phrase commence");
        text.end_final("phrase-1");
        assert!(text.finalizing_id.is_none());
        assert_eq!(text.partial, "La deuxième phrase commence");
    }
    #[test]
    fn old_sessions_and_old_final_jobs_cannot_replace_current_text() {
        let mut text = TextPresentation::default();
        text.begin_session("old");
        text.begin_session("new");
        text.preview("new", "Texte actuel".into());
        assert!(!text.preview("old", "Texte périmé".into()));
        text.begin_final("new-job");
        assert!(!text.final_progress("old-job", "Ancienne finale"));
        text.end_final("old-job");
        assert_eq!(text.partial, "Texte actuel");
        assert_eq!(text.finalizing_id.as_deref(), Some("new-job"));
    }
    #[test]
    fn only_successful_insert_actions_are_counted_in_the_displayed_session() {
        let mut text = TextPresentation::default();
        text.begin_session("first");
        assert!(!text.sent("first", "copy", "Copié"));
        assert!(!text.sent("first", "note", "Rangé"));
        assert!(!text.sent("first", "discard", "Écarté"));
        assert!(text.sent("first", "auto", "Un."));
        assert!(text.sent("first", "insert", "Deux."));
        assert_eq!(text.sent_count, 2);
        assert_eq!(text.last_sent_text, "Deux.");
        text.begin_session("second");
        assert!(!text.sent("first", "auto", "Ancienne prise."));
        assert_eq!(text.sent_count, 0);
        assert!(text.last_sent_text.is_empty());
    }
    #[test]
    fn cancellation_rejects_late_preview_without_erasing_sent_phrases() {
        let mut text = TextPresentation::default();
        text.begin_session("one");
        text.sent("one", "auto", "Déjà transmis.");
        text.begin_final("pending");
        text.cancel();
        assert!(!text.preview("one", "Après annulation".into()));
        assert!(!text.final_progress("pending", "Après annulation"));
        assert_eq!(text.sent_count, 1);
        assert_eq!(text.last_sent_text, "Déjà transmis.");
        assert!(text.partial.is_empty());
    }
    #[test]
    fn recovered_insert_is_counted_but_clipboard_alone_is_not() {
        let mut text = TextPresentation::default();
        assert!(!text.sent("recovered", "copy", "Texte"));
        assert_eq!(text.sent_count, 0);
        assert!(text.sent("recovered", "insert", "Texte"));
        assert_eq!(text.sent_count, 1);
    }
}
