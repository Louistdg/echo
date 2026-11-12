// Moteur de transcription — v4.
//
// L'interface `TranscriptionEngine` reste l'abstraction remplaçable (§2.2) :
// `WhisperCli` lance whisper.cpp en processus isolé, une exécution par
// transcription. Le reste de l'app ne voit que le trait.
//
// Ce qui change en v4, et pourquoi la dictée sonnait mal :
//
//  1. **Découverte automatique** du binaire et du modèle autour de l'exécutable.
//     Avant, un chemin vide = « moteur non configuré » et l'utilisateur ne
//     pouvait rien dicter.
//  2. **Le modèle est choisi par qualité** : `small` > `base` > `tiny`. Le
//     `ggml-tiny` embarqué par défaut produisait des phrases approximatives en
//     français ; c'est la première cause de mauvaise qualité.
//  3. **Recherche par faisceau** (`-bs`) et **contexte coupé** (`-mc 0`) : sans
//     ça, whisper répète la phrase précédente et invente sur les silences.
//  4. **Filtre d'hallucinations** : sur un silence, whisper.cpp sort des
//     génériques (« Sous-titres réalisés par la communauté d'Amara.org »,
//     « Merci d'avoir regardé cette vidéo »). On les reconnaît et on rend une
//     transcription vide, que l'app traite déjà comme « rien entendu ».
//  5. **Garde-fous audio** : un WAV trop court (< 0,25 s) ou vide ne part pas
//     au moteur.
//  6. **Aucune fenêtre console** : le processus est lancé avec
//     CREATE_NO_WINDOW, la sortie est capturée au lieu de s'afficher.
//
// Aucune donnée ne quitte la machine : le CLI lit un WAV local et écrit un
// fichier texte local.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub trait TranscriptionEngine: Send + Sync {
    fn name(&self) -> &'static str;
    /// Transcrit un WAV 16 kHz mono en texte. Erreur explicite en cas d'échec.
    fn transcribe(&self, wav_path: &Path) -> Result<String, String>;
}

/// Nettoyage local, sans IA : suppression des hésitations, majuscules en début
/// de phrase, espaces normalisés. Aucune reformulation — le sens est intact.
/// C'est le « nettoyage local d'abord » du plan ; la reformulation par modèle
/// reste un choix explicite, jamais appliquée ici.
pub fn clean_text(raw: &str) -> String {
    const FILLERS: &[&str] = &["euh", "hein", "ben", "hum", "genre", "bah"];

    let words: Vec<&str> = raw.split_whitespace().collect();
    let mut gardees: Vec<&str> = Vec::with_capacity(words.len());
    for w in words {
        let noyau = w.trim_matches(|c: char| !c.is_alphanumeric());
        if FILLERS.contains(&noyau.to_lowercase().as_str()) {
            continue;
        }
        gardees.push(w);
    }

    let s = gardees.join(" ");
    let mut chars: Vec<char> = s.chars().collect();
    let mut majuscule = true;
    for c in chars.iter_mut() {
        if majuscule && c.is_alphabetic() {
            if let Some(up) = c.to_uppercase().next() {
                *c = up;
            }
            majuscule = false;
        }
        if matches!(*c, '.' | '!' | '?') {
            majuscule = true;
        }
    }
    chars.into_iter().collect()
}

// ---------------------------------------------------------------- options

/// Réglages du moteur. Les valeurs par défaut sont pensées pour de la dictée
/// en français : précision d'abord, débit ensuite.
#[derive(Clone, Debug)]
pub struct WhisperOptions {
    /// `fr` par défaut. `auto` laisse whisper détecter (plus lent, moins sûr).
    pub language: String,
    /// Recherche par faisceau. 5 est le bon compromis ; 1 = glouton (rapide, moins juste).
    pub beam_size: u32,
    /// 0 = laisser whisper.cpp choisir (nombre de cœurs disponibles).
    pub threads: u32,
    /// Seuil d'entropie : au-delà, le segment est considéré comme douteux.
    pub entropy_thold: f32,
    /// Contexte glissant entre segments. Coupé, il n'y a plus de répétition en boucle.
    pub use_context: bool,
    /// Lexique passé au modèle (noms propres, sigles) pour améliorer la reconnaissance.
    pub prompt: Option<String>,
}

impl Default for WhisperOptions {
    fn default() -> Self {
        Self {
            language: "fr".to_string(),
            beam_size: 5,
            threads: 0,
            entropy_thold: 2.8,
            use_context: false,
            prompt: None,
        }
    }
}

/// Qualité connue d'un modèle, déduite de son nom de fichier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelQuality {
    Unknown,
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

impl ModelQuality {
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if name.contains("large") || name.contains("turbo") {
            Self::Large
        } else if name.contains("medium") {
            Self::Medium
        } else if name.contains("small") {
            Self::Small
        } else if name.contains("base") {
            Self::Base
        } else if name.contains("tiny") {
            Self::Tiny
        } else {
            Self::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Unknown => "inconnu",
        }
    }

    /// Message à montrer quand le modèle choisi plafonne la qualité.
    pub fn quality_warning(self) -> Option<String> {
        match self {
            Self::Tiny => Some(
                "Le modèle « tiny » est trop léger pour du français dicté : mots avalés et \
                 phrases approximatives. Prends « small » pour retrouver une dictée exploitable."
                    .to_string(),
            ),
            Self::Base => Some(
                "Le modèle « base » reste juste en dessous du seuil confortable en français. \
                 « small » est nettement plus sûr, au prix d'un peu de temps de calcul."
                    .to_string(),
            ),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- moteur

/// Implémentation locale via `whisper-cli` (whisper.cpp), un processus par
/// transcription.
pub struct WhisperCli {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub options: WhisperOptions,
}

impl WhisperCli {
    /// Construction historique : garde les chemins configurés par l'utilisateur.
    pub fn new(binary: PathBuf, model: PathBuf) -> Self {
        Self {
            binary,
            model,
            options: WhisperOptions::default(),
        }
    }

    pub fn with_options(binary: PathBuf, model: PathBuf, options: WhisperOptions) -> Self {
        Self {
            binary,
            model,
            options,
        }
    }

    pub fn quality(&self) -> ModelQuality {
        ModelQuality::from_path(&self.model)
    }

    /// Cherche `whisper-cli` et un modèle autour de l'exécutable, puis dans
    /// quelques emplacements classiques. Retient le meilleur modèle trouvé.
    /// Renvoie `None` si l'un des deux manque.
    pub fn discover() -> Option<Self> {
        let binary = find_binary()?;
        let model = find_model()?;
        Some(Self::new(binary, model))
    }

    /// Vérifie que le couple binaire/modèle est utilisable, avec un message
    /// qui dit quoi faire plutôt que « erreur ».
    pub fn check(&self) -> Result<(), String> {
        if self.binary.as_os_str().is_empty() || !self.binary.exists() {
            return Err(format!(
                "whisper-cli introuvable ({}) — place whisper-cli.exe à côté d'Écho \
                 ou indique son chemin dans les réglages",
                self.binary.display()
            ));
        }
        if self.model.as_os_str().is_empty() || !self.model.exists() {
            return Err(format!(
                "modèle Whisper introuvable ({}) — place un fichier ggml-*.bin à côté d'Écho \
                 (ggml-small.bin recommandé) ou indique son chemin dans les réglages",
                self.model.display()
            ));
        }
        Ok(())
    }

    /// Arguments passés à whisper-cli. Séparé pour être testable sans exécuter.
    pub fn arguments(&self, wav_path: &Path, out_base: &Path) -> Vec<OsString> {
        let o = &self.options;
        let mut a: Vec<OsString> = Vec::with_capacity(24);
        // Macro locale : la closure équivalente garderait un emprunt mutable sur `a`
        // et empêcherait les push directs de chemins (E0499).
        macro_rules! arg {
            ($s:expr) => {
                a.push(OsString::from($s))
            };
        }

        arg!("-m");
        a.push(self.model.clone().into_os_string());
        arg!("-f");
        a.push(wav_path.as_os_str().to_owned());
        arg!("-otxt");
        arg!("-of");
        a.push(out_base.as_os_str().to_owned());
        arg!("-l");
        arg!(o.language.as_str());
        // Pas d'horodatage : on veut du texte suivi, pas des segments.
        arg!("-nt");
        // Recherche par faisceau : nettement plus juste que le décodage glouton.
        arg!("-bs");
        arg!(o.beam_size.to_string());
        // Entropie haute = segment inventé : whisper le retente au lieu de l'accepter.
        arg!("-et");
        arg!(o.entropy_thold.to_string());
        // Contexte coupé : supprime les boucles de répétition.
        if !o.use_context {
            arg!("-mc");
            arg!("0");
        }
        if o.threads > 0 {
            arg!("-t");
            arg!(o.threads.to_string());
        }
        if let Some(p) = &o.prompt {
            if !p.trim().is_empty() {
                arg!("--prompt");
                arg!(p.as_str());
            }
        }
        a
    }

    fn run(&self, wav_path: &Path) -> Result<String, String> {
        let out_dir = std::env::temp_dir().join(format!("echo-trans-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| format!("dossier de transcription inaccessible : {e}"))?;
        let out_base = out_dir.join("out");

        let mut cmd = Command::new(&self.binary);
        cmd.args(self.arguments(wav_path, &out_base));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let sortie = cmd
            .output()
            .map_err(|e| format!("impossible de lancer whisper-cli : {e}"))?;

        if !sortie.status.success() {
            let _ = std::fs::remove_dir_all(&out_dir);
            let err = String::from_utf8_lossy(&sortie.stderr);
            let extrait: String = err.chars().rev().take(400).collect::<Vec<_>>()
                .into_iter().rev().collect();
            return Err(format!(
                "whisper-cli a échoué (code {}){}",
                sortie.status.code().map(|c| c.to_string()).unwrap_or_default(),
                if extrait.trim().is_empty() { String::new() } else { format!(" : {extrait}") }
            ));
        }

        let texte = std::fs::read_to_string(out_base.with_extension("txt"))
            .map_err(|e| format!("sortie de whisper-cli illisible : {e}"))?;
        let _ = std::fs::remove_dir_all(&out_dir);
        Ok(texte.trim().to_string())
    }
}

impl TranscriptionEngine for WhisperCli {
    fn name(&self) -> &'static str {
        "whisper.cpp (CLI)"
    }

    fn transcribe(&self, wav_path: &Path) -> Result<String, String> {
        self.check()?;
        if !wav_path.exists() {
            return Err(format!("audio absent : {}", wav_path.display()));
        }

        // Un enregistrement trop court ne contient pas de parole : inutile de
        // réveiller le modèle, qui n'y verrait qu'un silence à inventer.
        if let Some(d) = wav_duration_seconds(wav_path) {
            if d < 0.25 {
                return Ok(String::new());
            }
        }

        let debut = Instant::now();
        let brut = self.run(wav_path)?;
        let texte = clean_text(&brut);

        if is_hallucination(&texte) {
            eprintln!(
                "[echo] hallucination écartée ({} ms, modèle {}) : {:?}",
                debut.elapsed().as_millis(),
                self.quality().label(),
                texte
            );
            return Ok(String::new());
        }

        eprintln!(
            "[echo] transcription {} ms · modèle {} · {} caractères",
            debut.elapsed().as_millis(),
            self.quality().label(),
            texte.chars().count()
        );
        Ok(texte)
    }
}

// ---------------------------------------------------------------- helpers

/// Durée d'un WAV PCM, lue dans l'en-tête RIFF (pas besoin de décoder l'audio).
pub fn wav_duration_seconds(path: &Path) -> Option<f32> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut byte_rate: Option<u32> = None;
    let mut data_len: Option<u32> = None;

    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let corps = pos + 8;
        if id == b"fmt " && corps + 12 <= data.len() {
            byte_rate = Some(u32::from_le_bytes([
                data[corps + 8],
                data[corps + 9],
                data[corps + 10],
                data[corps + 11],
            ]));
        } else if id == b"data" {
            data_len = Some(size.min(data.len().saturating_sub(corps)) as u32);
            break;
        }
        pos = corps + size + (size % 2);
    }

    let rate = byte_rate? as f32;
    if rate <= 0.0 {
        return None;
    }
    Some(data_len? as f32 / rate)
}

/// Phrases que whisper.cpp produit sur du silence ou du bruit. Ce ne sont pas
    /// des transcriptions : les garder pollue la note de l'utilisateur.
    ///
    /// Trois règles, volontairement étroites — un faux positif supprime une
    /// vraie dictée en silence, ce qui est pire que laisser passer un générique :
    ///
    ///  1. tout le texte est une annotation entre crochets/parenthèses
    ///     (`[BLANK_AUDIO]`, `[Musique]`, `(applaudissements)`) ;
    ///  2. le texte ne contient aucun caractère alphanumérique (`♪`) ;
    ///  3. le texte commence par un générique connu, sans longue traîne derrière
    ///     (pour ne pas jeter « …on se rappelle demain et à la prochaine »).
    pub fn is_hallucination(text: &str) -> bool {
        const PHRASES: &[&str] = &[
            "sous-titres réalisés par la communauté",
            "sous-titrage société radio-canada",
            "sous-titrage : société radio-canada",
            "sous-titres par",
            "amara.org",
            "merci d'avoir regardé cette vidéo",
            "merci d'avoir regardé cette video",
            "merci d'avoir regardé la vidéo",
            "n'hésitez pas à vous abonner",
            "abonnez-vous à ma chaîne",
            "abonnez-vous pour",
            "pensez à vous abonner",
        ];
        const EXACTES: &[&str] = &["à la prochaine", "à bientôt", "sous-titres", "sous-titrage"];

        let t = text.trim();
        if t.is_empty() {
            return true;
        }
        let bas = t.to_lowercase();

        // 1) annotation complète : ce que le modèle met entre crochets n'est pas
        //    de la parole dictée.
        let entre = (bas.starts_with('[') && bas.ends_with(']'))
            || (bas.starts_with('(') && bas.ends_with(')'));
        if entre && bas.chars().count() > 2 {
            return true;
        }

        // 2) que des symboles (« ♪ », « ... »).
        if !bas.chars().any(|c| c.is_alphanumeric()) {
            return true;
        }

        // 3) génériques. Une phrase entière qui les résume, ou qui les dépasse
        //    de quelques mots seulement.
        if EXACTES.contains(&bas.as_str()) {
            return true;
        }
        PHRASES
            .iter()
            .any(|m| bas == *m || (bas.starts_with(m) && bas.chars().count() <= m.chars().count() + 24))
    }

fn find_binary() -> Option<PathBuf> {
    let mut candidats: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in ["whisper-cli.exe", "vendor/whisper-cli.exe", "moteur/whisper-cli.exe", "moteur/vendor/whisper-cli.exe"] {
                candidats.push(dir.join(rel));
            }
        }
    }
    for c in ["whisper-cli.exe", "main.exe"] {
        candidats.push(PathBuf::from(c));
    }
    candidats.into_iter().find(|p| p.exists())
}

fn find_model() -> Option<PathBuf> {
    let mut dossiers: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in ["", "models", "modeles", "vendor", "moteur", "moteur/vendor"] {
                dossiers.push(if rel.is_empty() { dir.to_path_buf() } else { dir.join(rel) });
            }
        }
    }
    dossiers.push(std::env::current_dir().unwrap_or_default());

    let mut meilleurs: Vec<(ModelQuality, PathBuf)> = Vec::new();
    for d in dossiers {
        let Ok(entrees) = std::fs::read_dir(&d) else { continue };
        for e in entrees.flatten() {
            let p = e.path();
            let nom = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if nom.starts_with("ggml-") && nom.ends_with(".bin") {
                meilleurs.push((ModelQuality::from_path(&p), p));
            }
        }
    }
    meilleurs.sort_by(|a, b| b.0.cmp(&a.0));
    meilleurs.into_iter().next().map(|(_, p)| p)
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
    fn le_nettoyage_retire_les_hesitations_et_met_des_majuscules() {
        assert_eq!(
            clean_text("euh bonjour. hein ceci est un test"),
            "Bonjour. Ceci est un test"
        );
        assert_eq!(
            clean_text("  du   texte   genre  melange  "),
            "Du texte melange"
        );
        assert_eq!(clean_text(""), "");
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

    #[test]
    fn la_qualite_du_modele_est_lue_dans_le_nom() {
        assert_eq!(ModelQuality::from_path(Path::new("ggml-tiny.bin")), ModelQuality::Tiny);
        assert_eq!(ModelQuality::from_path(Path::new("ggml-small.bin")), ModelQuality::Small);
        assert_eq!(
            ModelQuality::from_path(Path::new("C:/m/ggml-large-v3-turbo.bin")),
            ModelQuality::Large
        );
        assert_eq!(ModelQuality::from_path(Path::new("autre.bin")), ModelQuality::Unknown);
        assert!(ModelQuality::Tiny.quality_warning().is_some());
        assert!(ModelQuality::Small.quality_warning().is_none());
    }

    #[test]
    fn les_arguments_portent_les_reglages_de_dictee() {
        let opts = WhisperOptions { language: "fr".into(), ..Default::default() };
        let e = WhisperCli::with_options(
            PathBuf::from("w.exe"),
            PathBuf::from("ggml-small.bin"),
            opts,
        );
        let args: Vec<String> = e
            .arguments(Path::new("a.wav"), Path::new("out"))
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let s = args.join(" ");

        assert!(s.contains("-l fr"), "langue absente : {s}");
        assert!(s.contains("-bs 5"), "faisceau absent : {s}");
        assert!(s.contains("-mc 0"), "contexte non coupé : {s}");
        assert!(s.contains("--prompt") == false, "prompt vide ne doit pas partir : {s}");
    }

    #[test]
    fn un_lexique_est_transmis_au_modele() {
        let opts = WhisperOptions {
            prompt: Some("TARVOS, BFB, Hostinger".into()),
            ..Default::default()
        };
        let e = WhisperCli::with_options(PathBuf::from("w.exe"), PathBuf::from("m.bin"), opts);
        let args: Vec<String> = e
            .arguments(Path::new("a.wav"), Path::new("out"))
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.join(" ").contains("--prompt TARVOS"));
    }

    #[test]
    fn les_hallucinations_de_silence_sont_reconnues() {
        // Génériques de sous-titrage.
        assert!(is_hallucination("Sous-titres réalisés par la communauté d'Amara.org"));
        assert!(is_hallucination("Merci d'avoir regardé cette vidéo"));
        // Annotations complètes, quelle que soit la casse.
        assert!(is_hallucination("[BLANK_AUDIO]"));
        assert!(is_hallucination("[Musique]"));
        assert!(is_hallucination("(applaudissements)"));
        // Que des symboles.
        assert!(is_hallucination("♪♪"));
        // Vide.
        assert!(is_hallucination("   "));

        // Faux positifs à éviter absolument : une vraie dictée ne doit jamais partir.
        assert!(!is_hallucination(
            "J'ai fini la réunion, on se rappelle demain et à la prochaine"
        ));
        assert!(!is_hallucination("Le moteur de transcription est prêt."));
        assert!(!is_hallucination("J'ai noté ça entre crochets [urgent] dans la liste"));
        assert!(!is_hallucination("Merci beaucoup pour ton aide hier soir"));
    }

    #[test]
    fn la_duree_d_un_wav_est_lue_sans_decoder_l_audio() {
        // WAV mono 16 kHz, 1 seconde de silence.
        let dir = std::env::temp_dir().join(format!("echo-wav-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        let data_len: u32 = 32_000; // 16000 Hz * 2 octets * 1 s
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&16_000u32.to_le_bytes());
        w.extend_from_slice(&32_000u32.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        w.extend(std::iter::repeat(0u8).take(data_len as usize));
        std::fs::write(&p, &w).unwrap();

        let d = wav_duration_seconds(&p).expect("durée lisible");
        assert!((d - 1.0).abs() < 0.05, "durée lue : {d}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}