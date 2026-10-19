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

/// Statut des modules — affiché dans l'UI pour être transparent sur l'avancement.
#[derive(Serialize)]
pub struct ModuleStatus {
    pub capture_audio: &'static str,
    pub transcription: &'static str,
    pub notes: &'static str,
}

#[tauri::command]
pub fn get_status() -> ModuleStatus {
    ModuleStatus {
        capture_audio: "non implémenté (TODO v1 — module cpal)",
        transcription: "non implémenté (TODO v1 — whisper.cpp)",
        notes: "note du jour + journal de récupération (rejeu au démarrage)",
    }
}

/// Paramètres persistés dans le répertoire applicatif (settings.json).
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub notes_dir: Option<PathBuf>,
}

/// Résultat d'un ajout : l'UI n'affiche « Enregistré » que si cette valeur est
/// renvoyée, donc seulement après journal + écriture dans la note.
#[derive(Serialize)]
pub struct AppendResult {
    pub id: String,
    pub path: String,
    pub state: String,
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
#[tauri::command]
pub fn append_to_today_note(app: AppHandle, text: String) -> Result<AppendResult, String> {
    let settings = load_settings(&app);
    let root = settings.notes_dir.ok_or(
        "aucun dossier de notes configuré — utilise « Choisir le dossier » d'abord",
    )?;

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();
    let id = uuid::Uuid::new_v4().to_string();

    let journal = journal(&app)?;
    let entry = Entry::new(&id, &date_str, &time_str, &text);

    journal
        .append(&entry)
        .map_err(|e| format!("journal non écrit, rien n'a été ajouté : {e}"))?;

    let path = notes::append_entry(&root, &date_str, &time_str, &text, &id)
        .map_err(|e| {
            format!("note non écrite, entrée conservée au journal et réessayée au prochain démarrage : {e}")
        })?;

    let state = match journal.mark_done(&entry) {
        Ok(()) => "enregistré".to_string(),
        Err(_) => "enregistré (journal en attente de rejeu)".to_string(),
    };

    Ok(AppendResult {
        id,
        path: path.to_string_lossy().into_owned(),
        state,
    })
}

/// Rejoue au démarrage les entrées `pending`, puis compacte le journal.
/// Retourne le nombre d'entrées rejouées.
pub fn replay_pending(app: &AppHandle) -> Result<usize, String> {
    let Some(root) = load_settings(app).notes_dir else {
        return Ok(0);
    };
    replay_into(&root, &journal(app)?)
}

/// Cœur du rejeu, sans dépendance à Tauri (donc testable) : applique les
/// entrées `pending` au dossier `root`, marque chacune `done`, et compacte.
///
/// Idempotent : `notes::append_entry` ne réécrit pas une entrée dont le
/// marqueur est déjà présent. Rejouer deux fois ne duplique rien.
pub fn replay_into(root: &Path, journal: &Journal) -> Result<usize, String> {
    let pending = journal.pending();
    let mut n = 0;

    for entry in &pending {
        notes::append_entry(root, &entry.note, &entry.time, &entry.text, &entry.id)
            .map_err(|e| format!("rejeu impossible pour {} : {e}", entry.id))?;
        let _ = journal.mark_done(entry);
        n += 1;
    }

    // Compaction : 30 jours de `done`, tous les `pending` conservés.
    let cutoff = (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let _ = journal.compact(&cutoff);

    Ok(n)
}

/// Démarre une session de capture micro. NON IMPLÉMENTÉ — retourne une erreur
/// explicite (aucun faux succès).
#[tauri::command]
pub fn start_capture() -> Result<(), String> {
    Err("capture audio non implémentée (TODO v1 — module capture/cpal)".into())
}

/// Arrête la session de capture en cours. NON IMPLÉMENTÉ.
#[tauri::command]
pub fn stop_capture() -> Result<(), String> {
    Err("capture audio non implémentée (TODO v1 — module capture/cpal)".into())
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

        let e = Entry::new("id-unique", "2026-10-19", "19:07", "entrée en attente");
        journal.append(&e).unwrap();

        // La note n'existe pas : c'est le cas « supprimée pendant la coupure ».
        assert_eq!(replay_into(&notes_dir, &journal).unwrap(), 1);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert!(note.contains("entrée en attente"));
        assert!(note.contains("<!--echo:id-unique-->"));
        assert!(journal.pending().is_empty(), "l'entrée doit être marquée done");

        // Deuxième rejeu : rien à faire, et surtout aucune duplication.
        assert_eq!(replay_into(&notes_dir, &journal).unwrap(), 0);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert_eq!(note.matches("entrée en attente").count(), 1);
    }
}
