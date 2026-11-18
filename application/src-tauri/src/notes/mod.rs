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

/// Lit une note. L'absence de fichier vaut contenu vide (création normale) ;
/// toute autre erreur est propagée. Une note illisible (octets non UTF-8,
/// accès refusé) n'est **pas** une note vide : la traiter comme vide revient à
/// l'effacer à l'écriture suivante.
pub fn read_note(path: &Path) -> std::io::Result<String> {
    match fs::read(path) {
        Ok(octets) => String::from_utf8(octets).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "le contenu de la note n'est pas de l'UTF-8 valide",
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Nom du fichier de conflit écrit quand la note a changé entre la lecture et
/// le remplacement (§3.5 : « on n'écrase jamais silencieusement »).
fn conflict_path(path: &Path) -> PathBuf {
    let nom = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "note".into());
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    path.with_file_name(format!("{nom}.conflit-{stamp}.md"))
}

/// Remplace `path` par `content` **seulement** si son contenu est encore
/// `attendu`. Sinon, écrit la version proposée dans un fichier de conflit et
/// retourne une erreur explicite : la modification externe est préservée.
pub fn replace_if_unchanged(path: &Path, attendu: &str, content: &str) -> std::io::Result<()> {
    let actuel = read_note(path)?;
    if actuel != attendu {
        let conflit = conflict_path(path);
        write_atomic(&conflit, content.as_bytes())?;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "la note a été modifiée entre-temps ; votre entrée est écrite dans {}",
                conflit.display()
            ),
        ));
    }
    write_atomic(path, content.as_bytes())
}

/// Ajoute une entrée horodatée à la note du jour.
///
/// Idempotent : si la note contient déjà le marqueur `id`, rien n'est réécrit.
/// C'est ce qui permet de rejouer le journal après un arrêt brutal sans
/// dupliquer l'entrée.
///
/// Refuse d'écrire si la note est illisible, et n'écrase pas une note modifiée
/// entre la lecture et le remplacement.
pub fn append_entry(
    root: &Path,
    date_str: &str,
    time_str: &str,
    text: &str,
    id: &str,
) -> std::io::Result<PathBuf> {
    let path = ensure_daily_note(root, date_str)?;
    let current = read_note(&path)?;

    if current.contains(&marker(id)) {
        return Ok(path);
    }

    let mut content = current.clone();
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!(
        "## {time_str}\n{text}\n\n{}\n\n",
        marker(id)
    ));
    replace_if_unchanged(&path, &current, &content)?;
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

    #[test]
    fn une_note_illisible_ne_doit_pas_etre_ecrasee() {
        let dir = dossier_test("illisible");
        let p = daily_note_path(&dir, "2026-10-19");
        let mut octets = b"# Note existante\n\ntexte d'origine\n".to_vec();
        octets.push(0xFF); // octet non UTF-8 : lecture en texte impossible
        fs::write(&p, &octets).unwrap();
        let avant = fs::read(&p).unwrap();

        let resultat = append_entry(&dir, "2026-10-19", "19:07", "nouvelle entrée", "id-1");

        assert!(
            resultat.is_err(),
            "une note illisible doit refuser l'écriture, pas être remplacée"
        );
        assert_eq!(
            fs::read(&p).unwrap(),
            avant,
            "le contenu existant ne doit pas être écrasé"
        );
    }

    #[test]
    fn une_modification_externe_part_dans_un_fichier_de_conflit() {
        let dir = dossier_test("conflit");
        let p = append_entry(&dir, "2026-10-19", "19:07", "première", "id-1").unwrap();
        let lu = read_note(&p).unwrap();

        // L'utilisateur modifie la note entre la lecture et le remplacement.
        let modifie = lu.replace("première", "première corrigée");
        fs::write(&p, &modifie).unwrap();

        let resultat = replace_if_unchanged(&p, &lu, "contenu proposé");

        assert!(resultat.is_err(), "un changement externe doit bloquer le remplacement");
        assert_eq!(
            read_note(&p).unwrap(),
            modifie,
            "la version de l'utilisateur ne doit pas être écrasée"
        );
        let conflits: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("conflit"))
            .collect();
        assert_eq!(conflits.len(), 1, "un fichier de conflit doit être écrit");
        assert_eq!(
            fs::read_to_string(conflits[0].path()).unwrap(),
            "contenu proposé"
        );
    }
}
