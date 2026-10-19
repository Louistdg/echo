// Capture audio (§4.2 du plan).
// SQUELETTE — aucune implémentation pour l'instant.
// TODO v1 : session micro via cpal (WAV 16 kHz mono), VU-mètre (événement
// `capture:level` ~30 Hz), pause/reprise, VAD Silero pour l'arrêt au silence.

/// Session de capture — un seul actif à la fois (géré par le cœur Rust).
pub struct CaptureSession {
    started_at: chrono::DateTime<chrono::Local>,
}

impl CaptureSession {
    /// TODO v1 : ouvrir le périphérique de capture par défaut via cpal,
    /// écrire dans un WAV temporaire (%TEMP%/Écho/), publier les niveaux.
    pub fn start() -> Result<Self, String> {
        Err("CaptureSession::start non implémenté (TODO v1 — cpal)".into())
    }

    /// TODO v1 : clôturer le WAV, retourner son chemin et sa durée.
    pub fn stop(self) -> Result<CaptureResult, String> {
        Err("CaptureSession::stop non implémenté (TODO v1)".into())
    }
}

pub struct CaptureResult {
    pub wav_path: std::path::PathBuf,
    pub duration_secs: f32,
}
