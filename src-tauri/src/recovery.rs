// Journal de récupération append-only (§3.5 du plan).
//
// Règle : une entrée est écrite dans ce journal, synchronisée sur disque
// (`sync_data`), AVANT d'être ajoutée à la note. Si le processus meurt entre
// les deux, le démarrage suivant rejoue l'entrée `pending`. C'est le journal
// qui garantit la survie, pas la note.
//
// Une entrée est marquée `done` après écriture réussie dans la note. Le rejeu
// est idempotent : la note porte un marqueur `<!--echo:ID-->` et l'entrée n'est
// réécrite que si ce marqueur est absent — jamais deux fois.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Date de la note visée, `AAAA-MM-JJ`.
    pub note: String,
    /// Heure de l'entrée, `HH:MM`.
    pub time: String,
    pub text: String,
    /// `pending` tant que la note ne contient pas l'entrée, `done` ensuite.
    pub state: String,
}

impl Entry {
    pub fn new(id: &str, note: &str, time: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            note: note.to_string(),
            time: time.to_string(),
            text: text.to_string(),
            state: "pending".to_string(),
        }
    }

    pub fn done(&self) -> Self {
        Self {
            state: "done".to_string(),
            ..self.clone()
        }
    }
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ajoute une entrée et la synchronise sur disque avant de rendre la main.
    /// C'est l'appel qui doit précéder toute écriture dans la note.
    pub fn append(&self, entry: &Entry) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        // fsync : sans cela, la ligne peut rester dans le cache du système et
        // disparaître avec la coupure qu'on cherche justement à couvrir.
        file.sync_data()
    }

    /// Lit toutes les lignes valides. Une ligne corrompue est ignorée : une
    /// ligne illisible ne doit pas empêcher le rejeu des autres.
    pub fn read_all(&self) -> Vec<Entry> {
        let Ok(file) = File::open(&self.path) else {
            return Vec::new();
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Entry>(&l).ok())
            .collect()
    }

    /// Entrées `pending` qui n'ont pas reçu de `done` correspondant, dans
    /// l'ordre du journal.
    pub fn pending(&self) -> Vec<Entry> {
        let all = self.read_all();
        let done: HashSet<&str> = all
            .iter()
            .filter(|e| e.state == "done")
            .map(|e| e.id.as_str())
            .collect();

        let mut vus = HashSet::new();
        all.iter()
            .filter(|e| e.state == "pending")
            .filter(|e| !done.contains(e.id.as_str()))
            .filter(|e| vus.insert(e.id.clone()))
            .cloned()
            .collect()
    }

    /// Marque une entrée comme appliquée. Un échec ici n'est pas grave : la note
    /// porte déjà le marqueur, le prochain rejeu la reconnaîtra et ne dupliquera
    /// rien.
    pub fn mark_done(&self, entry: &Entry) -> std::io::Result<()> {
        self.append(&entry.done())
    }

    /// Compacte le journal : garde toutes les entrées `pending`, et les `done`
    /// dont la note n'est pas antérieure à `min_date` (`AAAA-MM-JJ`).
    /// Réécriture atomique : une coupure pendant la compaction laisse l'ancien
    /// journal intact.
    pub fn compact(&self, min_date: &str) -> std::io::Result<usize> {
        let all = self.read_all();
        let done_ids: HashSet<&str> = all
            .iter()
            .filter(|e| e.state == "done")
            .map(|e| e.id.as_str())
            .collect();

        let keep: Vec<&Entry> = all
            .iter()
            .filter(|e| {
                let pending = e.state == "pending" && !done_ids.contains(e.id.as_str());
                pending || e.note.as_str() >= min_date
            })
            .collect();

        let mut out = String::new();
        for e in &keep {
            out.push_str(&serde_json::to_string(e).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err)
            })?);
            out.push('\n');
        }
        crate::atomic::write_atomic(&self.path, out.as_bytes())?;
        Ok(all.len() - keep.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal_test(nom: &str) -> Journal {
        let dir = std::env::temp_dir().join(format!("echo-journal-{nom}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Journal::new(dir.join("recovery.log"))
    }

    #[test]
    fn append_puis_lecture_conserve_les_champs() {
        let j = journal_test("lecture");
        let e = Entry::new("id-1", "2026-10-19", "19:07", "Bonjour à toi");
        j.append(&e).unwrap();
        assert_eq!(j.read_all(), vec![e]);
    }

    #[test]
    fn pending_ignore_les_entrees_terminees() {
        let j = journal_test("pending");
        let a = Entry::new("a", "2026-10-19", "10:00", "un");
        let b = Entry::new("b", "2026-10-19", "10:01", "deux");
        j.append(&a).unwrap();
        j.append(&b).unwrap();
        j.mark_done(&a).unwrap();
        assert_eq!(j.pending(), vec![b]);
    }

    #[test]
    fn une_ligne_corrompue_n_empeche_pas_le_rejeu() {
        let j = journal_test("corrompue");
        let a = Entry::new("a", "2026-10-19", "10:00", "un");
        j.append(&a).unwrap();
        {
            let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
            f.write_all(b"{ceci n'est pas du json\n").unwrap();
        }
        assert_eq!(j.pending(), vec![a]);
    }

    #[test]
    fn compaction_garde_les_pending_et_jette_les_vieux_done() {
        let j = journal_test("compaction");
        let vieux = Entry::new("vieux", "2026-01-01", "10:00", "vieux");
        let recent = Entry::new("recent", "2026-10-19", "10:00", "recent");
        let attente = Entry::new("attente", "2026-10-19", "10:05", "attente");
        j.append(&vieux).unwrap();
        j.mark_done(&vieux).unwrap();
        j.append(&recent).unwrap();
        j.mark_done(&recent).unwrap();
        j.append(&attente).unwrap();

        let jetees = j.compact("2026-09-19").unwrap();
        assert_eq!(jetees, 2, "vieux pending + vieux done doivent partir");
        let restants: Vec<String> = j.read_all().into_iter().map(|e| e.id).collect();
        assert!(restants.contains(&"recent".to_string()));
        assert!(restants.contains(&"attente".to_string()));
        assert!(!restants.contains(&"vieux".to_string()));
        assert_eq!(j.pending(), vec![attente]);
    }
}
