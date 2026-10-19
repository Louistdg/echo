// Commandes IPC exposées à l'UI (§8.2 du plan).
// Principe d'honnêteté : ce qui n'est pas implémenté retourne une erreur
// explicite plutôt qu'un succès factice.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::notes;
use crate::recovery::{Entry, Journal};
use crate::transcription::TranscriptionEngine;

/// Statut des modules — affiché dans l'UI pour être transparent sur l'avancement.
#[derive(Serialize)]
pub struct ModuleStatus {
    pub capture_audio: &'static str,
    pub transcription: &'static str,
    pub notes: &'static str,
    /// Résumé de la dernière récupération de journal (vide si tout va bien).
    pub recovery: String,
}

/// Dernier état de récupération, lisible par l'UI.
static RECOVERY_STATUS: std::sync::LazyLock<std::sync::Mutex<String>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(String::new()));

#[tauri::command]
pub fn get_status() -> ModuleStatus {
    ModuleStatus {
        capture_audio: "capture OK (M2) — WAV 16 kHz mono",
        transcription: "moteur whisper.cpp prêt si binaire + modèle configurés dans les réglages",
        notes: "note du jour + journal de récupération (rejeu au démarrage)",
        recovery: RECOVERY_STATUS
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default(),
    }
}

/// Paramètres persistés dans le répertoire applicatif (settings.json).
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub notes_dir: Option<PathBuf>,
    pub whisper_bin: Option<PathBuf>,
    pub whisper_model: Option<PathBuf>,
}

/// Résultat d'un ajout : l'UI n'affiche « Enregistré » que si cette valeur est
/// renvoyée, donc seulement après journal + écriture dans la note.
#[derive(Serialize)]
pub struct AppendResult {
    pub id: String,
    pub path: String,
    pub state: String,
    pub text: String,
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

/// Lit un fichier de réglages en tolérant un BOM UTF-8 en tête.
///
/// Défaut corrigé le 19/10/2026 : un `settings.json` écrit par un éditeur
/// Windows (BOM) faisait échouer `serde_json` ; l'erreur était avalée et
/// l'utilisateur voyait ses réglages disparaître sans message.
pub fn read_settings_file(path: &Path) -> Settings {
    let Ok(bytes) = fs::read(path) else {
        return Settings::default();
    };
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).unwrap_or_default()
}

fn load_settings(app: &AppHandle) -> Settings {
    match settings_path(app) {
        Some(path) => read_settings_file(&path),
        None => Settings::default(),
    }
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app).ok_or("répertoire de configuration indisponible")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Jamais de BOM à l'écriture.
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

fn journal(app: &AppHandle) -> Result<Journal, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("répertoire applicatif indisponible : {e}"))?;
    Ok(Journal::new(dir.join("recovery.log")))
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    load_settings(&app)
}

/// Ouvre un dialogue natif de choix du dossier de notes et persiste le choix.
/// Le dialogue bloquant tourne sur un thread dédié (spawn_blocking) pour ne
/// jamais bloquer le thread principal.
#[tauri::command]
pub async fn select_notes_folder(app: AppHandle) -> Result<String, String> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("erreur interne : {e}"))?;
    let Some(picked) = picked else {
        return Err("aucun dossier sélectionné".into());
    };
    let dir = picked.simplified().into_path().map_err(|e| e.to_string())?;
    let mut settings = load_settings(&app);
    settings.notes_dir = Some(dir.clone());
    save_settings(&app, &settings)?;
    Ok(dir.to_string_lossy().to_string())
}

/// Ajoute une entrée horodatée à la note du jour, avec journal de récupération.
///
/// Ordre imposé : (1) journal + fsync, (2) note en écriture atomique,
/// (3) marquage `done`. Une coupure entre (1) et (2) laisse un `pending` que le
/// démarrage suivant rejoue ; entre (2) et (3), le marqueur d'idempotence dans la
/// note empêche tout doublon.
/// Cœur d'ajout, partagé par la frappe et la dictée : journal → note → done.
/// L'identifiant est stable : rejouer ou retranscrire ne duplique pas.
pub fn append_text(app: &AppHandle, text: &str, id: &str) -> Result<AppendResult, String> {
    let settings = load_settings(app);
    let root = settings.notes_dir.ok_or(
        "aucun dossier de notes configuré — utilise « Choisir le dossier » d'abord",
    )?;

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();

    let journal = journal(app)?;
    let entry = Entry::new(
        id,
        &root.to_string_lossy(),
        &date_str,
        &time_str,
        text,
    );

    journal
        .append(&entry)
        .map_err(|e| format!("journal non écrit, rien n'a été ajouté : {e}"))?;

    let path = notes::append_entry(&root, &date_str, &time_str, text, id).map_err(|e| {
        format!(
            "note non écrite, entrée conservée au journal et réessayée au prochain démarrage : {e}"
        )
    })?;

    let state = match journal.mark_done(&entry) {
        Ok(()) => "enregistré".to_string(),
        Err(_) => "enregistré (journal en attente de rejeu)".to_string(),
    };

    Ok(AppendResult {
        id: id.to_string(),
        path: path.to_string_lossy().into_owned(),
        state,
        text: text.to_string(),
    })
}

#[tauri::command]
pub fn append_to_today_note(
    app: AppHandle,
    text: String,
    entry_id: Option<String>,
) -> Result<AppendResult, String> {
    // Identifiant d'opération : l'interface le conserve entre deux tentatives
    // pour qu'un réessai après échec ne crée pas une seconde entrée.
    let id = entry_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    append_text(&app, &text, &id)
}

/// Transcrit un enregistrement puis l'ajoute à la note du jour.
/// Le texte suit le même chemin sûr que la frappe (journal → note → done),
/// avec un identifiant dérivé du nom du fichier : retranscrire ne duplique pas.
#[tauri::command]
pub fn transcribe_and_append(app: AppHandle, path: String) -> Result<AppendResult, String> {
    let settings = load_settings(&app);
    let root = settings.notes_dir.ok_or(
        "aucun dossier de notes configuré — utilise « Choisir le dossier » d'abord",
    )?;
    let rec_dir = crate::capture::recordings_dir(&root);
    let wav = PathBuf::from(&path);
    if !wav.starts_with(&rec_dir) {
        return Err("ce fichier n'est pas un enregistrement d'Écho".into());
    }

    let engine = crate::transcription::WhisperCli::new(
        settings.whisper_bin.unwrap_or_default(),
        settings.whisper_model.unwrap_or_default(),
    );
    let texte = crate::transcription::clean_text(&engine.transcribe(&wav)?);
    if texte.is_empty() {
        return Err("transcription vide — le moteur n'a rien entendu".into());
    }

    let id = format!(
        "trans-{}",
        wav.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    append_text(&app, &texte, &id)
}

/// Résultat d'un rejeu : combien d'entrées, et ce qui n'a pas pu être lu.
#[derive(Debug, Default, Serialize)]
pub struct ReplayReport {
    pub rejouees: usize,
    pub lignes_invalides: usize,
    pub fin_reparee: bool,
    pub compaction_refusee: bool,
    pub erreur: Option<String>,
}

impl ReplayReport {
    pub fn resume(&self) -> String {
        if let Some(e) = &self.erreur {
            return format!("journal illisible — récupération à faire : {e}");
        }
        if self.lignes_invalides > 0 {
            return format!(
                "{} ligne(s) de journal illisible(s) — compaction bloquée, récupération à faire",
                self.lignes_invalides
            );
        }
        if self.compaction_refusee {
            return "compaction du journal refusée — récupération à faire".into();
        }
        if self.fin_reparee {
            return "journal réparé (écriture interrompue) — sauvegarde conservée".into();
        }
        String::new()
    }
}

/// Rejoue au démarrage les entrées `pending`, puis compacte le journal.
pub fn replay_pending(app: &AppHandle) -> Result<ReplayReport, String> {
    let Some(root) = load_settings(app).notes_dir else {
        return Ok(ReplayReport::default());
    };
    let rapport = replay_into(&root, &journal(app)?)?;
    let resume = rapport.resume();
    if let Ok(mut slot) = RECOVERY_STATUS.lock() {
        *slot = resume;
    }
    Ok(rapport)
}

/// Cœur du rejeu, sans dépendance à Tauri (donc testable).
///
/// Chaque entrée est rejouée **dans le dossier qu'elle a journalisé**, pas dans
/// le dossier configuré au moment du redémarrage. Idempotent :
/// `notes::append_entry` ne réécrit pas une entrée dont le marqueur est présent.
pub fn replay_into(root: &Path, journal: &Journal) -> Result<ReplayReport, String> {
    let mut rapport = ReplayReport::default();

    if journal.reparer_fin_tronquee().unwrap_or(false) {
        rapport.fin_reparee = true;
    }

    let lecture = journal.read_report();
    if let Some(e) = lecture.erreur.clone() {
        rapport.erreur = Some(e);
        return Ok(rapport);
    }
    rapport.lignes_invalides = lecture.lignes_invalides;

    for entry in Journal::pending_of(&lecture) {
        let destination = if entry.dir.trim().is_empty() {
            root.to_path_buf()
        } else {
            PathBuf::from(&entry.dir)
        };
        notes::append_entry(
            &destination,
            &entry.note,
            &entry.time,
            &entry.text,
            &entry.id,
        )
        .map_err(|e| format!("rejeu impossible pour {} : {e}", entry.id))?;
        let _ = journal.mark_done(&entry);
        rapport.rejouees += 1;
    }

    // Compaction : 30 jours de `done`, tous les `pending` conservés. Refusée si
    // le journal est incomplet — l'erreur est remontée, pas avalée.
    let cutoff = (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    if let Err(e) = journal.compact(&cutoff) {
        rapport.compaction_refusee = true;
        eprintln!("compaction refusée : {e}");
    }

    Ok(rapport)
}

/// Niveau sonore poussé vers l'UI (~30 Hz pendant l'enregistrement).
#[derive(Serialize, Clone)]
pub struct LevelPayload {
    pub rms: f32,
}

/// Démarre la capture micro. L'audio est rangé dans
/// `<dossier de notes>/.echo/audio/`, jamais dans un temporaire système.
#[tauri::command]
pub fn start_capture(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let root = load_settings(&app).notes_dir.ok_or(
        "aucun dossier de notes configuré — choisis-le d'abord, l'audio est rangé juste à côté",
    )?;
    let dir = crate::capture::recordings_dir(&root);
    let handle = app.clone();
    crate::capture::start(dir, move |rms| {
        let _ = handle.emit("capture:level", LevelPayload { rms });
    })
}

/// Un enregistrement, tel que listé dans l'interface.
#[derive(Serialize)]
pub struct RecordingInfo {
    pub name: String,
    pub path: String,
    pub duration_secs: f64,
    pub modified: String,
}

/// Liste les 20 enregistrements les plus récents.
#[tauri::command]
pub fn list_recordings(app: AppHandle) -> Result<Vec<RecordingInfo>, String> {
    let Some(root) = load_settings(&app).notes_dir else {
        return Ok(Vec::new());
    };
    let dir = crate::capture::recordings_dir(&root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut out: Vec<RecordingInfo> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
        })
        .map(|e| {
            let path = e.path();
            let meta = e.metadata().ok();
            let taille = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    chrono::DateTime::<chrono::Local>::from(t)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            RecordingInfo {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
                duration_secs: taille.saturating_sub(44) as f64 / 32_000.0,
                modified,
            }
        })
        .collect();

    out.sort_by(|a, b| b.name.cmp(&a.name));
    out.truncate(20);
    Ok(out)
}

/// Ouvre le dossier des enregistrements dans l'Explorateur.
#[tauri::command]
pub fn open_recordings_folder(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    let dir = crate::capture::recordings_dir(&root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let chemin = dir.to_string_lossy().into_owned();
    app.opener()
        .open_path(chemin.clone(), None::<&str>)
        .map_err(|e| format!("ouverture du dossier impossible : {e}"))?;
    Ok(chemin)
}

/// Ouvre le dossier des notes dans l'Explorateur.
#[tauri::command]
pub fn open_notes_folder(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    let chemin = root.to_string_lossy().into_owned();
    app.opener()
        .open_path(chemin.clone(), None::<&str>)
        .map_err(|e| format!("ouverture du dossier impossible : {e}"))?;
    Ok(chemin)
}

/// Ouvre un enregistrement (lecteur par défaut), uniquement s'il vient du
/// dossier d'enregistrements — un chemin arbitraire est refusé.
#[tauri::command]
pub fn open_recording(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    let dir = crate::capture::recordings_dir(&root);
    let cible = PathBuf::from(&path);
    if !cible.starts_with(&dir) {
        return Err("ce fichier n'est pas dans le dossier d'enregistrements".into());
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("ouverture impossible : {e}"))
}

/// Purge les `.part` laissés par un arrêt brutal. Ne touche jamais aux WAV
/// finalisés : ce sont des données utilisateur.
pub fn purge_recording_parts(app: &AppHandle) -> usize {
    let Some(root) = load_settings(app).notes_dir else {
        return 0;
    };
    crate::capture::purge_parts(&crate::capture::recordings_dir(&root))
}

/// Arrête la capture, finalise le WAV et retourne son emplacement et sa durée.
#[tauri::command]
pub fn stop_capture() -> Result<crate::capture::CaptureInfo, String> {
    crate::capture::stop()
}

#[tauri::command]
pub fn pause_capture() -> Result<(), String> {
    crate::capture::pause()
}

#[tauri::command]
pub fn resume_capture() -> Result<(), String> {
    crate::capture::resume()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier_test(nom: &str, octets: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-settings-{nom}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        fs::write(&f, octets).unwrap();
        f
    }

    #[test]
    fn lit_un_settings_sans_bom() {
        let f = fichier_test("sans-bom", br#"{"notes_dir":"D:\\Notes"}"#);
        let s = read_settings_file(&f);
        assert_eq!(s.notes_dir, Some(PathBuf::from("D:\\Notes")));
    }

    #[test]
    fn lit_un_settings_avec_bom() {
        let mut octets = vec![0xEF, 0xBB, 0xBF];
        octets.extend_from_slice(br#"{"notes_dir":"D:\\Notes"}"#);
        let f = fichier_test("avec-bom", &octets);
        let s = read_settings_file(&f);
        assert_eq!(
            s.notes_dir,
            Some(PathBuf::from("D:\\Notes")),
            "un BOM ne doit plus faire perdre le dossier configuré"
        );
    }

    #[test]
    fn settings_absent_donne_des_valeurs_vides() {
        let s = read_settings_file(Path::new("D:\\chemin\\qui\\n'existe\\pas.json"));
        assert_eq!(s.notes_dir, None);
    }

    #[test]
    fn settings_corrompu_ne_panique_pas() {
        let f = fichier_test("corrompu", b"{pas du json");
        assert_eq!(read_settings_file(&f).notes_dir, None);
    }

    #[test]
    fn rejeu_recree_une_note_supprimee_de_l_exterieur() {
        let dir = std::env::temp_dir().join(format!("echo-rejeu-{}", uuid::Uuid::new_v4()));
        let notes_dir = dir.join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let journal = Journal::new(dir.join("recovery.log"));

        let e = Entry::new(
            "id-unique",
            &notes_dir.to_string_lossy(),
            "2026-10-19",
            "19:07",
            "entrée en attente",
        );
        journal.append(&e).unwrap();

        // La note n'existe pas : c'est le cas « supprimée pendant la coupure ».
        assert_eq!(replay_into(&notes_dir, &journal).unwrap().rejouees, 1);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert!(note.contains("entrée en attente"));
        assert!(note.contains("<!--echo:id-unique-->"));
        assert!(journal.pending().is_empty(), "l'entrée doit être marquée done");

        // Deuxième rejeu : rien à faire, et surtout aucune duplication.
        assert_eq!(replay_into(&notes_dir, &journal).unwrap().rejouees, 0);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert_eq!(note.matches("entrée en attente").count(), 1);
    }

    #[test]
    fn le_rejeu_respecte_le_dossier_journalise_pas_le_dossier_courant() {
        let dir = std::env::temp_dir().join(format!("echo-dest-{}", uuid::Uuid::new_v4()));
        let dossier_a = dir.join("A");
        let dossier_b = dir.join("B");
        std::fs::create_dir_all(&dossier_a).unwrap();
        std::fs::create_dir_all(&dossier_b).unwrap();
        let journal = Journal::new(dir.join("recovery.log"));

        // Entrée destinée à A, alors que l'application est reconfigurée sur B.
        let e = Entry::new(
            "id-a",
            &dossier_a.to_string_lossy(),
            "2026-10-19",
            "19:07",
            "destinée à A",
        );
        journal.append(&e).unwrap();

        let rapport = replay_into(&dossier_b, &journal).unwrap();
        assert_eq!(rapport.rejouees, 1);
        assert!(
            dossier_a.join("2026-10-19.md").exists(),
            "l'entrée doit atterrir dans le dossier journalisé"
        );
        assert!(
            !dossier_b.join("2026-10-19.md").exists(),
            "elle ne doit pas atterrir dans le dossier courant"
        );
    }
}
