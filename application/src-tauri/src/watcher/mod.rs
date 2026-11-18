// Observateur du dossier de notes (§3.5 du plan).
// SQUELETTE — aucune implémentation pour l'instant.
// TODO v2 : watcher (crate notify) + rescan au démarrage et toutes les 60 s,
// détection des modifications externes via content_hash, resynchronisation
// de l'index SQLite (qui reste un cache reconstructible).

pub struct FolderWatcher;

impl FolderWatcher {
    /// TODO v2 : initialiser notify::Watcher sur le dossier racine.
    pub fn new() -> Result<Self, String> {
        Err("FolderWatcher non implémenté (TODO v2 — crate notify)".into())
    }
}
