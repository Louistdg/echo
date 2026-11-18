// Journal de récupération append-only (§3.5 du plan).
//
// Règle : une entrée est écrite dans ce journal, synchronisée sur disque
// (`sync_data`), AVANT d'être ajoutée à la note. Si le processus meurt entre
// les deux, le démarrage suivant rejoue l'entrée `pending`. C'est le journal
// qui garantit la survie, pas la note.
//
// Une entrée porte la destination (`dir`) en plus de la date : sans elle, un
// changement de dossier de notes ferait atterrir une entrée en attente dans le
// mauvais dossier. Le rejeu utilise donc la destination journalisée.
//
// Une entrée est marquée `done` après écriture réussie dans la note. Le rejeu
// est idempotent : la note porte un marqueur `<!--echo:ID-->` et l'entrée n'est
// réécrite que si ce marqueur est absent — jamais deux fois.
//
// Lecture honnête : une fin de ligne tronquée ou une ligne illisible n'est
// jamais « réparée » en silence. Le journal est sauvegardé à part, la ligne
// tronquée est écartée, et la compaction est refusée tant qu'il reste des
// lignes invalides.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Dossier de notes visé, figé au moment de l'écriture.
    #[serde(default)]
    pub dir: String,
    /// Date de la note visée, `AAAA-MM-JJ`.
    pub note: String,
    /// Heure de l'entrée, `HH:MM`.
    pub time: String,
    pub text: String,
    /// `pending` tant que la note ne contient pas l'entrée, `done` ensuite.
    pub state: String,
}

impl Entry {
    pub fn new(id: &str, dir: &str, note: &str, time: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            dir: dir.to_string(),
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

/// Résultat d'une lecture de journal : ce qui a été lu, et ce qui a échoué.
#[derive(Debug, Default)]
pub struct ReadReport {
    pub entries: Vec<Entry>,
    pub lignes_invalides: usize,
    pub fin_tronquee: bool,
    pub erreur: Option<String>,
}

impl ReadReport {
    pub fn est_incomplet(&self) -> bool {
        self.erreur.is_some() || self.lignes_invalides > 0 || self.fin_tronquee
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

    /// Lit le journal en distinguant les trois cas : absent, lisible, abîmé.
    pub fn read_report(&self) -> ReadReport {
        let mut rapport = ReadReport::default();

        let octets = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return rapport,
            Err(e) => {
                rapport.erreur = Some(format!("journal illisible : {e}"));
                return rapport;
            }
        };

        if !octets.is_empty() && !octets.ends_with(b"\n") {
            rapport.fin_tronquee = true;
        }

        let texte = String::from_utf8_lossy(&octets);
        for ligne in texte.lines() {
            if ligne.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Entry>(ligne) {
                Ok(e) => rapport.entries.push(e),
                Err(_) => rapport.lignes_invalides += 1,
            }
        }

        rapport
    }

    /// Entrées `pending` sans `done` correspondant, dans l'ordre du journal.
    /// Un journal illisible ne rend aucune entrée (on ne « répare » pas au
    /// hasard) — l'appelant doit consulter `read_report` pour le dire.
    pub fn pending(&self) -> Vec<Entry> {
        let rapport = self.read_report();
        if rapport.erreur.is_some() {
            return Vec::new();
        }
        Self::pending_of(&rapport)
    }

    pub fn pending_of(rapport: &ReadReport) -> Vec<Entry> {
        let done: HashSet<&str> = rapport
            .entries
            .iter()
            .filter(|e| e.state == "done")
            .map(|e| e.id.as_str())
            .collect();

        let mut vus = HashSet::new();
        rapport
            .entries
            .iter()
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

    /// Copie le journal abîmé à côté, horodatée, avant toute réécriture.
    fn sauvegarder_abime(&self) -> Option<PathBuf> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let backup = self
            .path
            .with_file_name(format!("recovery.corrompu-{stamp}.log"));
        std::fs::copy(&self.path, &backup).ok().map(|_| backup)
    }

    /// Traite une fin de ligne tronquée (écriture interrompue) : sauvegarde le
    /// journal, puis réécrit uniquement les entrées complètes. Retourne `true`
    /// si une réparation a eu lieu.
    pub fn reparer_fin_tronquee(&self) -> std::io::Result<bool> {
        let rapport = self.read_report();
        if !rapport.fin_tronquee || rapport.erreur.is_some() {
            return Ok(false);
        }
        self.sauvegarder_abime();
        let mut out = String::new();
        for e in &rapport.entries {
            out.push_str(&serde_json::to_string(e).map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err)
            })?);
            out.push('\n');
        }
        crate::atomic::write_atomic(&self.path, out.as_bytes())?;
        Ok(true)
    }

    /// Compacte le journal : garde toutes les entrées `pending`, et les `done`
    /// dont la note n'est pas antérieure à `min_date` (`AAAA-MM-JJ`).
    ///
    /// Refusé si la lecture est incomplète (ligne illisible) : compacter un
    /// sous-ensemble lirait comme une perte définitive. Le journal est alors
    /// sauvegardé et l'erreur remontée.
    pub fn compact(&self, min_date: &str) -> std::io::Result<usize> {
        let rapport = self.read_report();
        if let Some(e) = &rapport.erreur {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.clone()));
        }
        if rapport.lignes_invalides > 0 || rapport.fin_tronquee {
            self.sauvegarder_abime();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "compaction refusée : {} ligne(s) illisible(s){} — journal sauvegardé à part",
                    rapport.lignes_invalides,
                    if rapport.fin_tronquee { ", fin tronquée" } else { "" }
                ),
            ));
        }

        let done_ids: HashSet<&str> = rapport
            .entries
            .iter()
            .filter(|e| e.state == "done")
            .map(|e| e.id.as_str())
            .collect();

        let keep: Vec<&Entry> = rapport
            .entries
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
        Ok(rapport.entries.len() - keep.len())
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
        let e = Entry::new("id-1", "D:\\Notes", "2026-10-19", "19:07", "Bonjour à toi");
        j.append(&e).unwrap();
        assert_eq!(j.read_report().entries, vec![e]);
    }

    #[test]
    fn pending_ignore_les_entrees_terminees() {
        let j = journal_test("pending");
        let a = Entry::new("a", "D:\\Notes", "2026-10-19", "10:00", "un");
        let b = Entry::new("b", "D:\\Notes", "2026-10-19", "10:01", "deux");
        j.append(&a).unwrap();
        j.append(&b).unwrap();
        j.mark_done(&a).unwrap();
        assert_eq!(j.pending(), vec![b]);
    }

    #[test]
    fn une_ligne_corrompue_est_comptee_et_n_empeche_pas_le_rejeu() {
        let j = journal_test("corrompue");
        let a = Entry::new("a", "D:\\Notes", "2026-10-19", "10:00", "un");
        j.append(&a).unwrap();
        {
            let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
            f.write_all(b"{ceci n'est pas du json\n").unwrap();
        }
        let rapport = j.read_report();
        assert_eq!(rapport.lignes_invalides, 1);
        assert!(rapport.est_incomplet());
        assert_eq!(Journal::pending_of(&rapport), vec![a]);
    }

    #[test]
    fn une_fin_tronquee_est_detectee_puis_reparee() {
        let j = journal_test("tronquee");
        let a = Entry::new("a", "D:\\Notes", "2026-10-19", "10:00", "un");
        j.append(&a).unwrap();
        {
            let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
            f.write_all(b"{\"id\":\"b\",\"note\":\"2026-10-1").unwrap(); // coupé net
        }
        let rapport = j.read_report();
        assert!(rapport.fin_tronquee, "une ligne sans saut final doit être signalée");

        assert!(j.reparer_fin_tronquee().unwrap());
        let apres = j.read_report();
        assert!(!apres.fin_tronquee);
        assert_eq!(apres.entries, vec![a]);

        let sauvegardes: Vec<_> = std::fs::read_dir(j.path().parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("corrompu"))
            .collect();
        assert_eq!(sauvegardes.len(), 1, "le journal abîmé doit être conservé à part");
    }

    #[test]
    fn la_compaction_est_refusee_si_une_ligne_est_illisible() {
        let j = journal_test("compaction-refusee");
        let a = Entry::new("a", "D:\\Notes", "2026-01-01", "10:00", "vieux");
        j.append(&a).unwrap();
        j.mark_done(&a).unwrap();
        {
            let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
            f.write_all(b"pas du json\n").unwrap();
        }
        assert!(j.compact("2026-09-19").is_err());
        assert!(
            j.read_report().entries.iter().any(|e| e.id == "a"),
            "aucune entrée ne doit disparaître"
        );
    }

    #[test]
    fn compaction_garde_les_pending_et_jette_les_vieux_done() {
        let j = journal_test("compaction");
        let vieux = Entry::new("vieux", "D:\\Notes", "2026-01-01", "10:00", "vieux");
        let recent = Entry::new("recent", "D:\\Notes", "2026-10-19", "10:00", "recent");
        let attente = Entry::new("attente", "D:\\Notes", "2026-10-19", "10:05", "attente");
        j.append(&vieux).unwrap();
        j.mark_done(&vieux).unwrap();
        j.append(&recent).unwrap();
        j.mark_done(&recent).unwrap();
        j.append(&attente).unwrap();

        let jetees = j.compact("2026-09-19").unwrap();
        assert_eq!(jetees, 2, "vieux pending + vieux done doivent partir");
        let restants: Vec<String> = j.read_report().entries.into_iter().map(|e| e.id).collect();
        assert!(restants.contains(&"recent".to_string()));
        assert!(restants.contains(&"attente".to_string()));
        assert!(!restants.contains(&"vieux".to_string()));
        assert_eq!(j.pending(), vec![attente]);
    }
}
