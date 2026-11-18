// Écriture atomique adaptée à Windows (§3.5 du plan).
//
// Principe : on n'écrase jamais un fichier en place. On écrit un fichier
// temporaire sur le MÊME volume, on le synchronise sur disque, puis on le
// substitue à la cible avec ReplaceFileW (préserve les attributs et les liens).
// Une coupure de courant laisse donc soit l'ancien fichier, soit le nouveau,
// jamais un fichier tronqué.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Écrit `bytes` dans `path` de façon atomique (remplacement).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = temp_sibling(path);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    replace(&tmp, path)
}

/// Nom de fichier temporaire voisin de la cible (même volume, donc remplacement
/// atomique possible). Le PID évite qu'un rejeu concurrent écrase le temporaire
/// d'un autre processus.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "fichier".into());
    let tmp_name = format!(".{name}.{}.tmp", std::process::id());
    match path.parent() {
        Some(dir) => dir.join(tmp_name),
        None => PathBuf::from(tmp_name),
    }
}

#[cfg(windows)]
fn replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    if !dst.exists() {
        return fs::rename(src, dst);
    }

    let src_w: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst_w: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();

    let ok = unsafe {
        ReplaceFileW(
            dst_w.as_ptr(),
            src_w.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        let _ = fs::remove_file(src);
        return Err(err);
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::rename(src, dst)
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// Remplace un fichier par un autre, en conservant attributs et liens.
    fn ReplaceFileW(
        lpReplacedFileName: *const u16,
        lpReplacementFileName: *const u16,
        lpBackupFileName: *const u16,
        dwReplaceFlags: u32,
        lpExclude: *mut core::ffi::c_void,
        lpReserved: *mut core::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier_test(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-test-{nom}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ecrit_un_nouveau_fichier() {
        let dir = dossier_test("atomic-new");
        let f = dir.join("note.md");
        write_atomic(&f, b"premier").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "premier");
        assert!(!dir.join(format!(".note.md.{}.tmp", std::process::id())).exists());
    }

    #[test]
    fn remplace_un_fichier_existant() {
        let dir = dossier_test("atomic-replace");
        let f = dir.join("note.md");
        fs::write(&f, "ancien contenu plus long").unwrap();
        write_atomic(&f, b"nouveau").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "nouveau");
    }
}
