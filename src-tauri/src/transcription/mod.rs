// Moteur de transcription (§5 du plan).
//
// L'interface `TranscriptionEngine` est l'abstraction remplaçable (§2.2) :
// aujourd'hui une implémentation `WhisperCli` qui lance whisper.cpp en
// processus isolé (une exécution par transcription), demain un `whisper-server`
// résident ou un moteur cloud opt-in. Le reste de l'app ne voit que le trait.
//
// Aucune donnée ne quitte la machine : le CLI lit un WAV local et écrit un
// fichier texte local.

use std::path::{Path, PathBuf};
use std::process::Command;

pub trait TranscriptionEngine: Send + Sync {
    fn name(&self) -> &'static str;
    /// Transcrit un WAV 16 kHz mono en texte. Erreur explicite en cas d'échec.
    fn transcribe(&self, wav_path: &Path) -> Result<String, String>;
}

/// Implémentation locale via `whisper-cli` (whisper.cpp), un processus par
/// transcription. Les chemins du binaire et du modèle viennent des réglages.
pub struct WhisperCli {
    pub binary: PathBuf,
    pub model: PathBuf,
}

impl WhisperCli {
    pub fn new(binary: PathBuf, model: PathBuf) -> Self {
        Self { binary, model }
    }
}

impl TranscriptionEngine for WhisperCli {
    fn name(&self) -> &'static str {
        "whisper.cpp (CLI)"
    }

    fn transcribe(&self, wav_path: &Path) -> Result<String, String> {
        if self.binary.as_os_str().is_empty() || !self.binary.exists() {
            return Err(format!(
                "whisper-cli introuvable : {} — configure le binaire dans les réglages",
                self.binary.display()
            ));
        }
        if self.model.as_os_str().is_empty() || !self.model.exists() {
            return Err(format!(
                "modèle Whisper introuvable : {} — configure-le dans les réglages",
                self.model.display()
            ));
        }
        if !wav_path.exists() {
            return Err(format!("audio absent : {}", wav_path.display()));
        }

        let out_dir = std::env::temp_dir().join(format!("echo-trans-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| format!("dossier de transcription inaccessible : {e}"))?;
        let out_base = out_dir.join("out");

        let status = Command::new(&self.binary)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(wav_path)
            .arg("-otxt")
            .arg("-of")
            .arg(&out_base)
            .arg("-l")
            .arg("fr")
            .arg("-nt")
            .status()
            .map_err(|e| format!("impossible de lancer whisper-cli : {e}"))?;

        if !status.success() {
            return Err(format!(
                "whisper-cli a échoué (code {})",
                status.code().map(|c| c.to_string()).unwrap_or_default()
            ));
        }

        let texte = std::fs::read_to_string(out_base.with_extension("txt"))
            .map_err(|e| format!("sortie de whisper-cli illisible : {e}"))?;
        let _ = std::fs::remove_dir_all(&out_dir);
        Ok(texte.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MoteurFixe(String);
    impl TranscriptionEngine for MoteurFixe {
        fn name(&self) -> &'static str {
            "moteur de test"
        }
        fn transcribe(&self, _wav: &Path) -> Result<String, String> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn le_trait_remplacable_fonctionne() {
        let e = MoteurFixe("phrase transcrite".into());
        assert_eq!(e.transcribe(Path::new("x.wav")).unwrap(), "phrase transcrite");
        assert_eq!(e.name(), "moteur de test");
    }

    #[test]
    fn un_binaire_absent_donne_une_erreur_explicite() {
        let e = WhisperCli::new(
            PathBuf::from("Z:\\introuvable\\whisper-cli.exe"),
            PathBuf::from("Z:\\introuvable\\model.bin"),
        );
        let err = e.transcribe(Path::new("x.wav")).unwrap_err();
        assert!(err.contains("introuvable"), "message : {err}");
    }

    #[test]
    fn un_audio_absent_est_refuse_avant_le_moteur() {
        let dir = std::env::temp_dir().join(format!("echo-tr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fake.exe");
        std::fs::write(&bin, b"x").unwrap();
        let model = dir.join("m.bin");
        std::fs::write(&model, b"x").unwrap();
        let e = WhisperCli::new(bin, model);
        let err = e.transcribe(&dir.join("absent.wav")).unwrap_err();
        assert!(err.contains("audio absent"), "message : {err}");
    }
}
