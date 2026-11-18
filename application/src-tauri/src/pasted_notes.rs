//! Sauvegarde d'une réponse collée, sans transformer son Markdown.
//!
//! Un UUID conserve le même chemin pendant les sauvegardes et les réessais.
//! La création refuse tout écrasement ; les mises à jour comparent le dernier
//! contenu confirmé. Ce contrôle reste optimiste face à un éditeur externe :
//! le mutex sérialise les appels de cette application, pas ceux d'autres outils.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Sauvegarde `text` exactement, sans titre ajouté ni nettoyage du contenu.
///
/// `expected = None` crée une note ; `Some(previous)` met à jour uniquement la
/// version précédemment confirmée. Un contenu déjà identique à `text` confirme
/// un réessai dont la réponse a pu être perdue, sans créer une seconde note.
pub fn save_pasted_note(
    root: &Path,
    id: &str,
    text: &str,
    expected: Option<&str>,
) -> Result<PathBuf, String> {
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| "identifiant de note invalide — la réponse n'a pas été enregistrée".to_string())?;
    if text.trim().is_empty() {
        return Err("la réponse est vide — aucun fichier n'a été écrit".into());
    }

    let _lock = SAVE_LOCK
        .lock()
        .map_err(|_| "la sauvegarde est indisponible — conserve ta réponse et réessaie".to_string())?;
    fs::create_dir_all(root)
        .map_err(|e| format!("dossier de notes inaccessible — réponse non enregistrée : {e}"))?;
    let path = root.join(format!("reponse-{id}.md"));

    match read_existing(&path)? {
        Some(current) => {
            // Même après une réponse perdue, un réessai confirme le fichier
            // existant au lieu de le réécrire ou de créer un doublon.
            if current == text.as_bytes() {
                return confirm_saved(root, path);
            }
            match expected {
                Some(previous) if current == previous.as_bytes() => {
                    // Le verrou couvre le temporaire utilisé par write_atomic.
                    crate::atomic::write_atomic(&path, text.as_bytes())
                        .map_err(|e| format!("mise à jour non enregistrée — conserve ta réponse : {e}"))?;
                    confirm_saved(root, path)
                }
                _ => Err(conflict_message(&path)),
            }
        }
        None if expected.is_some() => Err(format!(
            "la note a été supprimée ou déplacée — aucun fichier recréé : {}",
            path.display()
        )),
        None => {
            let temp_path = root.join(format!(".reponse-{id}-{}.tmp", uuid::Uuid::new_v4()));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|e| format!("création du fichier temporaire impossible : {e}"))?;
            // Ne prendre possession du temporaire qu'après create_new réussi.
            let temp = TemporaryFile(temp_path);
            {
                let mut file = file;
                file.write_all(text.as_bytes())
                    .and_then(|_| file.sync_all())
                    .map_err(|e| format!("réponse non enregistrée — conserve le texte : {e}"))?;
            }
            match publish_new(&temp.0, &path) {
                Ok(()) => confirm_saved(root, path),
                Err(error) => {
                    // Une autre instance a pu créer la destination depuis la
                    // lecture initiale. La publication n'écrase jamais ce fichier.
                    match read_existing(&path)? {
                        Some(current) if current == text.as_bytes() => confirm_saved(root, path),
                        Some(_) => Err(conflict_message(&path)),
                        None => Err(format!(
                            "réponse non enregistrée — création du fichier impossible : {error}"
                        )),
                    }
                }
            }
        }
    }
}

fn confirm_saved(_root: &Path, path: PathBuf) -> Result<PathBuf, String> {
    // Sous Unix, synchroniser aussi la publication du nom dans le répertoire.
    // Windows utilise MOVEFILE_WRITE_THROUGH lors de la création.
    #[cfg(not(windows))]
    fs::File::open(_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| format!("confirmation de sauvegarde impossible — conserve ta réponse : {e}"))?;
    Ok(path)
}

fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("lecture de la note impossible — aucun écrasement : {e}")),
        Ok(meta) if !meta.file_type().is_file() => Err(format!(
            "la destination n'est pas un fichier ordinaire — aucun écrasement : {}",
            path.display()
        )),
        Ok(_) => fs::read(path)
            .map(Some)
            .map_err(|e| format!("lecture de la note impossible — aucun écrasement : {e}")),
    }
}

fn conflict_message(path: &Path) -> String {
    format!(
        "ce fichier contient une autre version — aucun écrasement, conserve ta réponse : {}",
        path.display()
    )
}

/// Supprime uniquement notre propre temporaire, y compris en cas d'erreur.
struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn publish_new(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    let source: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_WRITE_THROUGH, sans MOVEFILE_REPLACE_EXISTING : une cible
    // apparue entre-temps est refusée. Fonctionne aussi sur FAT/exFAT.
    let result = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x8) };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

#[cfg(not(windows))]
fn publish_new(src: &Path, dst: &Path) -> std::io::Result<()> {
    // Le temporaire est voisin : le lien ne traverse jamais un volume et sa
    // création refuse une destination déjà présente. Drop retire le temporaire.
    fs::hard_link(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("echo-pasted-{}", uuid::Uuid::new_v4())))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[test]
    fn conserve_exactement_un_long_markdown_et_cree_le_dossier() {
        let dir = TestDirectory::new();
        let text = "# Réponse 🐕\r\n\r\n- élément\r\n\t```rust\r\nlet x = 1;\r\n```  \r\n".repeat(12_000);
        let path = save_pasted_note(&dir.0, &id(), &text, None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
        assert!(path.starts_with(&dir.0));
        assert_eq!(path.extension().unwrap(), "md");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn un_reessai_confirme_le_meme_fichier_sans_doublon() {
        let dir = TestDirectory::new();
        let id = id();
        let first = save_pasted_note(&dir.0, &id, "texte exact", None).unwrap();
        let retry = save_pasted_note(&dir.0, &id, "texte exact", None).unwrap();
        assert_eq!(first, retry);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn une_creation_ne_remplace_pas_un_fichier_different() {
        let dir = TestDirectory::new();
        let id = id();
        let path = save_pasted_note(&dir.0, &id, "original", None).unwrap();
        assert!(save_pasted_note(&dir.0, &id, "autre réponse", None).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn une_mise_a_jour_et_son_reessai_conservent_le_chemin() {
        let dir = TestDirectory::new();
        let id = id();
        let first = save_pasted_note(&dir.0, &id, "version 1", None).unwrap();
        let next = save_pasted_note(&dir.0, &id, "version 2", Some("version 1")).unwrap();
        let retry = save_pasted_note(&dir.0, &id, "version 2", Some("version 1")).unwrap();
        assert_eq!(first, next);
        assert_eq!(next, retry);
        assert_eq!(fs::read_to_string(next).unwrap(), "version 2");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn une_modification_externe_est_preservee() {
        let dir = TestDirectory::new();
        let id = id();
        let path = save_pasted_note(&dir.0, &id, "version 1", None).unwrap();
        fs::write(&path, "modification externe").unwrap();
        assert!(save_pasted_note(&dir.0, &id, "version 2", Some("version 1")).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "modification externe");
    }

    #[test]
    fn une_suppression_externe_ne_recree_pas_la_note() {
        let dir = TestDirectory::new();
        let id = id();
        let path = save_pasted_note(&dir.0, &id, "version 1", None).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(save_pasted_note(&dir.0, &id, "version 2", Some("version 1")).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn refuse_texte_vide_et_identifiant_de_traversee() {
        let dir = TestDirectory::new();
        for text in ["", " \n\r\t "] {
            assert!(save_pasted_note(&dir.0, &id(), text, None).is_err());
        }
        for id in ["../note", "..\\note", "", "pas-un-uuid"] {
            assert!(save_pasted_note(&dir.0, id, "réponse", None).is_err());
        }
        assert!(!dir.0.exists());
    }

    #[test]
    fn refuse_un_parent_qui_est_un_fichier() {
        let dir = TestDirectory::new();
        fs::create_dir_all(&dir.0).unwrap();
        let parent = dir.0.join("parent.txt");
        fs::write(&parent, "à préserver").unwrap();
        assert!(save_pasted_note(&parent, &id(), "réponse", None).is_err());
        assert_eq!(fs::read_to_string(parent).unwrap(), "à préserver");
    }

    #[test]
    fn refuse_une_note_illisible_sans_en_changer_les_octets() {
        let dir = TestDirectory::new();
        let id = id();
        let path = save_pasted_note(&dir.0, &id, "initial", None).unwrap();
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(save_pasted_note(&dir.0, &id, "réponse", Some("initial")).is_err());
        assert_eq!(fs::read(path).unwrap(), [0xff, 0xfe, 0x00]);
    }

    #[test]
    fn publier_ne_remplace_jamais_une_destination_deja_presente() {
        let dir = TestDirectory::new();
        fs::create_dir_all(&dir.0).unwrap();
        let src = dir.0.join("source.tmp");
        let dst = dir.0.join("note.md");
        fs::write(&src, "nouveau").unwrap();
        fs::write(&dst, "existant").unwrap();
        assert!(publish_new(&src, &dst).is_err());
        assert_eq!(fs::read_to_string(dst).unwrap(), "existant");
        assert_eq!(fs::read_to_string(src).unwrap(), "nouveau");
    }
}
