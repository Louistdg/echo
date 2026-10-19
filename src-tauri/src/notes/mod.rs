// Gestion des notes (§3 du plan d'architecture).
// Le dossier de notes en .md est la source de vérité. Toute écriture passe par
// `atomic::write_atomic` (fichier temporaire du même volume, puis ReplaceFileW).
// Chaque entrée porte un marqueur invisible `<!--echo:ID-->` qui rend le rejeu
// du journal de récupération idempotent.

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic::write_atomic;

/// Nom de fichier de la note du jour : AAAA-MM-JJ.md (§3.2).
pub fn daily_note_path(root: &Path, date_str: &str) -> PathBuf {
    root.join(format!("{date_str}.md"))
}

/// Marqueur d'idempotence, invisible au rendu Markdown.
pub fn marker(id: &str) -> String {
    format!("<!--echo:{id}-->")
}

/// Crée la note du jour si elle n'existe pas, avec son frontmatter minimal.
pub fn ensure_daily_note(root: &Path, date_str: &str) -> std::io::Result<PathBuf> {
    let path = daily_note_path(root, date_str);
    if !path.exists() {
        fs::create_dir_all(root)?;
        let content = format!(
            "---\ncreated: {date_str}\ntype: journal\nsource: echo\n---\n\n# {date_str}\n\n"
        );
        write_atomic(&path, content.as_bytes())?;
    }
    Ok(path)
}

/// Ajoute une entrée horodatée à la note du jour.
///
/// Idempotent : si la note contient déjà le marqueur `id`, rien n'est réécrit.
/// C'est ce qui permet de rejouer le journal après un arrêt brutal sans
/// dupliquer l'entrée.
pub fn append_entry(
    root: &Path,
    date_str: &str,
    time_str: &str,
    text: &str,
    id: &str,
) -> std::io::Result<PathBuf> {
    let path = ensure_daily_note(root, date_str)?;
    let current = fs::read_to_string(&path).unwrap_or_default();

    if current.contains(&marker(id)) {
        return Ok(path);
    }

    let mut content = current;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!(
        "## {time_str}\n{text}\n\n{}\n\n",
        marker(id)
    ));
    write_atomic(&path, content.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier_test(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-notes-{nom}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cree_la_note_avec_frontmatter_puis_ajoute() {
        let dir = dossier_test("creation");
        let p = append_entry(&dir, "2026-10-19", "19:07", "Bonjour", "id-1").unwrap();
        let contenu = fs::read_to_string(&p).unwrap();
        assert!(contenu.starts_with("---\ncreated: 2026-10-19\n"));
        assert!(contenu.contains("# 2026-10-19"));
        assert!(contenu.contains("## 19:07\nBonjour"));
        assert!(contenu.contains("<!--echo:id-1-->"));
    }

    #[test]
    fn rejoue_sans_dupliquer_la_meme_entree() {
        let dir = dossier_test("idempotence");
        append_entry(&dir, "2026-10-19", "19:07", "Une seule fois", "id-1").unwrap();
        append_entry(&dir, "2026-10-19", "19:07", "Une seule fois", "id-1").unwrap();
        append_entry(&dir, "2026-10-19", "19:07", "Une seule fois", "id-1").unwrap();
        let contenu = fs::read_to_string(daily_note_path(&dir, "2026-10-19")).unwrap();
        assert_eq!(contenu.matches("## 19:07").count(), 1);
        assert_eq!(contenu.matches("<!--echo:id-1-->").count(), 1);
    }

    #[test]
    fn deux_entrees_identiques_avec_ids_differents_sont_conservees() {
        let dir = dossier_test("texte-identique");
        append_entry(&dir, "2026-10-19", "19:07", "même texte", "id-1").unwrap();
        append_entry(&dir, "2026-10-19", "19:07", "même texte", "id-2").unwrap();
        let contenu = fs::read_to_string(daily_note_path(&dir, "2026-10-19")).unwrap();
        assert_eq!(contenu.matches("même texte").count(), 2);
    }

    #[test]
    fn recree_la_note_supprimee_de_l_exterieur() {
        let dir = dossier_test("suppression");
        let p = append_entry(&dir, "2026-10-19", "19:07", "avant", "id-1").unwrap();
        fs::remove_file(&p).unwrap();
        append_entry(&dir, "2026-10-19", "19:07", "après", "id-2").unwrap();
        let contenu = fs::read_to_string(&p).unwrap();
        assert!(contenu.contains("# 2026-10-19"));
        assert!(contenu.contains("après"));
        assert!(!contenu.contains("avant"), "le contenu perdu ne peut pas revenir seul");
    }

    #[test]
    fn les_accents_traversent_intacts() {
        let dir = dossier_test("accents");
        let p = append_entry(&dir, "2026-10-19", "19:07", "àéèù — çà", "id-1").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap().contains("àéèù — çà"), true);
    }
}
