// Moteur de transcription (§5 du plan).
// SQUELETTE — le trait TranscriptionEngine est l'abstraction remplaçable ;
// l'implémentation locale whisper.cpp (whisper-server en boucle locale)
// arrive en v1, l'implémentation cloud reste opt-in (§6.1).

/// Texte + segments horodatés produits par un moteur.
pub struct TranscriptionResult {
    pub raw_text: String,
    pub segments: Vec<Segment>,
    pub language: String,
}

pub struct Segment {
    pub t_start: f64,
    pub t_end: f64,
    pub text: String,
}

/// Abstraction moteur (§2.2) : local whisper.cpp d'abord, cloud opt-in ensuite.
pub trait TranscriptionEngine {
    fn name(&self) -> &'static str;
    /// Transcrit un WAV 16 kHz mono. TODO v1 : requête HTTP vers
    /// whisper-server sur 127.0.0.1:<port aléatoire>, sérialisée par le cœur.
    fn transcribe(&self, wav_path: &std::path::Path) -> Result<TranscriptionResult, String>;
}

/// Implémentation locale whisper.cpp — NON IMPLÉMENTÉE.
pub struct WhisperCpp;

impl TranscriptionEngine for WhisperCpp {
    fn name(&self) -> &'static str {
        "whisper.cpp"
    }

    fn transcribe(&self, _wav_path: &std::path::Path) -> Result<TranscriptionResult, String> {
        Err("WhisperCpp::transcribe non implémenté (TODO v1 — whisper-server)".into())
    }
}

/// Implémentation cloud opt-in — NON IMPLÉMENTÉE, restera désactivée par
/// défaut (principe de confidentialité §6.1 : local par défaut).
pub struct Cloud;

impl TranscriptionEngine for Cloud {
    fn name(&self) -> &'static str {
        "cloud (opt-in)"
    }

    fn transcribe(&self, _wav_path: &std::path::Path) -> Result<TranscriptionResult, String> {
        Err("Cloud::transcribe non implémenté (opt-in explicite requis, §6.1)".into())
    }
}
