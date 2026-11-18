# Écho — Plan d'architecture complet

> Application de bureau de capture vocale locale : dictée hors ligne, bloc-notes intégré, notes enregistrées dans un dossier choisi.
> Document de conception — version 1.0 — 19 octobre 2026

---

## Table des matières

1. [Vision et périmètre](#1-vision-et-périmètre)
2. [Architecture générale](#2-architecture-générale)
3. [Stockage et modèle de données](#3-stockage-et-modèle-de-données)
4. [Parcours utilisateur de bout en bout](#4-parcours-utilisateur-de-bout-en-bout)
5. [Le moteur de transcription](#5-le-moteur-de-transcription)
6. [Confidentialité et sécurité](#6-confidentialité-et-sécurité)
7. [Catalogue des fonctionnalités](#7-catalogue-des-fonctionnalités)
8. [Spécification technique](#8-spécification-technique)
9. [Tests et critères d'acceptation](#9-tests-et-critères-dacceptation)
10. [Roadmap de livraison](#10-roadmap-de-livraison)

---

## 1. Vision et périmètre

### 1.1 La promesse en une phrase

> « J'appuie sur un raccourci, je parle, le texte apparaît dans ma note du jour, dans mon dossier, sur mon PC. Rien ne quitte ma machine. »

### 1.2 Besoins certains (exprimés par l'utilisateur)

- Une petite application résidant en bas de l'écran (zone de notification / fenêtre flottante).
- Un bloc-notes intégré avec sauvegarde automatique dans un **dossier choisi par l'utilisateur**.
- Un bouton micro (et idéalement un raccourci clavier) pour dicter.
- Une transcription **correcte en français**, fonctionnant même quand l'IA cible (ex. DeepSeek) n'offre pas de saisie vocale : l'app doit pouvoir *coller* le texte dans n'importe quelle application.
- Possibilité d'ajouter d'autres fonctionnalités.

### 1.3 Hypothèses et recommandations

| Sujet | Hypothèse | Statut |
|---|---|---|
| OS cible | Windows 10/11 en priorité | À confirmer avec la config de l'utilisateur |
| Matériel | PC moyen, GPU inconnu | L'app doit détecter les capacités (CPU AVX, GPU) et s'adapter |
| Langue principale | Français, avec détection FR/EN | Par défaut |
| Connexion | Internet nécessaire uniquement au premier téléchargement du modèle | Tout le reste est hors ligne |
| Portage | macOS/Linux possibles plus tard (Tauri est multiplateforme) | Phase ultérieure |

### 1.4 Ce que l'app n'est PAS (au début)

- Pas un assistant IA conversationnel.
- Pas une extension navigateur (limitée pour le micro, les dossiers et les raccourcis globaux ; l'app desktop est plus fiable).
- Pas un service cloud : aucun compte obligatoire, aucune donnée envoyée par défaut.

---

## 2. Architecture générale

### 2.1 Vue d'ensemble en couches

```
┌─────────────────────────────────────────────────────────────────┐
│  COUCHE PRÉSENTATION (TypeScript / framework léger)              │
│  ├─ Fenêtre principale (bloc-notes + historique)                │
│  ├─ Fenêtre flottante compacte (dictée rapide)                  │
│  ├─ Menu tray (zone de notification Windows)                    │
│  └─ Fenêtre Paramètres                                          │
└────────────────────────────┬────────────────────────────────────┘
                             │ IPC Tauri (invoke / events)
┌────────────────────────────┴────────────────────────────────────┐
│  COUCHE APPLICATION — Cœur Rust (processus principal)            │
│  ├─ Gestionnaire de raccourcis globaux                          │
│  ├─ Gestionnaire de fenêtres (position, focus, affichage)       │
│  ├─ Orchestrateur de capture (sessions audio)                   │
│  ├─ Gestionnaire de notes (fichiers, index, sauvegarde)         │
│  ├─ Gestionnaire de paramètres                                   │
│  └─ Gestionnaire de modèles (téléchargement, sélection)          │
└──────┬─────────────────┬──────────────────┬─────────────────────┘
       │ IPC dédié       │ fichiers locaux   │ HTTP (localhost)
┌──────────────────────┬─────────────────────────────────────────┐
│ STOCKAGE             │ MOTEUR DE TRANSCRIPTION                 │
│ ├─ dossier notes     │ whisper-server (whisper.cpp) :          │
│ │  (source de        │ processus de travail isolé, boucle       │
│ │  vérité .md)      │ locale uniquement                        │
│ ├─ SQLite (index     │ Le CŒUR RUST est propriétaire de la      │
│ │  reconstructible)  │ file de tâches : il lance/arrête et      │
│ ├─ .echo/ (métadonnées│ redémarre le moteur, sérialise les     │
│ │  sessions, audio)  │ requêtes, applique les quotas            │
│ └─ appdata (cfg)     │ Remplaçable via le trait                 │
│                      │ TranscriptionEngine (faster-whisper,     │
└──────────────────────┤ cloud opt-in)                            │
                       └─────────────────────────────────────────┘
```

### 2.2 Choix de stack justifié

| Élément | Choix | Justification |
|---|---|---|
| Framework | **Tauri 2** (stable depuis octobre 2024, actif en 2026) | Binaires petits (~quelques Mo), cœur en Rust, plugins officiels pour tout ce dont on a besoin : raccourcis globaux, tray, clipboard, autostart, updater, fichiers, dialogues ([blog Tauri 2.0](https://v2.tauri.app/blog/tauri-20/)) |
| UI | TypeScript + **Svelte** (recommandé) | Fenêtres rapides, très faible empreinte mémoire ; un seul framework recommandé, React reste possible si tu le préfères |
| Raccourcis globaux | Plugin officiel `global-shortcut` | Enregistre des raccourcis système, gère Pressed/Released (idéal pour le push-to-talk) ([doc](https://v2.tauri.app/plugin/global-shortcut/)) |
| Tray | Module `tray-icon` intégré à Tauri | Icône en zone de notification + menu contextuel |
| Transcription | **whisper.cpp** en processus serveur local | C/C++ sans dépendances, CPU AVX, GPU CUDA/Vulkan/OpenVINO, quantification, VAD Silero intégrée, et un `whisper-server` avec API compatible OpenAI ([GitHub whisper.cpp](https://github.com/ggml-org/whisper.cpp/)) |
| Alternative moteur | faster-whisper (CTranslate2) | Précision comparable à Whisper original avec des optimisations d'inférence ([benchmark université de Göttingen, 2026](https://hps.vi4io.org/_media/research/labs/2026/2026-02-deepak_budha-performance_benchmarking_and_evaluation_of_open_source_asr_frameworks_for_real_time_transcription-report.pdf)). Attention : les comparaisons publiées mélangent des configurations (modèles, quantifications, VAD) différentes, donc pas de conclusion ferme. Recommandation : si le besoin apparaît, comparer les deux moteurs **à modèle et paramètres identiques** sur des enregistrements français représentatifs |
| Remplacement opt-in | API cloud (Whisper API, etc.) | Uniquement si l'utilisateur l'active explicitement, avec affichage clair des données envoyées |

**Point clé :** l'interface du moteur de transcription est une abstraction (`TranscriptionEngine`) avec deux implémentations (local whisper.cpp, distant opt-in). On ne construit PAS un système de plugins généraliste au départ — juste une interface remplaçable.

### 2.3 Pourquoi le moteur tourne dans un processus séparé

- **Isolation des pannes** : si le moteur plante (OOM, modèle corrompu), le cœur Rust le redémarre et l'UI affiche « Moteur redémarré, réessayez » — l'app de notes ne plante jamais avec lui.
- **Annulation** : le cœur Rust est propriétaire de la file de tâches ; annuler = terminer le processus moteur (le `whisper-server` upstream n'offre pas d'annulation propre, c'est notre intégration qui la fournit).
- **Non-blocage** : l'utilisateur continue à écrire pendant qu'un long import est transcrit (ex. 1 h d'audio).

**Capacités réelles du serveur upstream vs. notre intégration** — à ne pas confondre : le `whisper-server` de whisper.cpp fournit l'inférence par requête HTTP, sans authentification, sans progression, sans annulation, une requête à la fois ([README whisper.cpp](https://github.com/ggml-org/whisper.cpp/)). C'est donc le cœur Rust qui fournit : la file d'attente, l'annulation (arrêt/redémarrage du processus), la progression (estimation par durée audio traitée, **pas de texte partiel promis en v1**, le serveur n'exposant pas de streaming), et le verrouillage réseau (écoute en boucle locale uniquement).

### 2.4 Communication entre composants

- **UI ↔ Cœur Rust** : IPC Tauri (`invoke` pour les commandes, `emit`/`listen` pour les événements asynchrones comme `transcription:progress`, `transcription:done`, `note:saved`).
- **Cœur ↔ Moteur** : requêtes HTTP sur `http://127.0.0.1:<port aléatoire>` (API compatible OpenAI de `whisper-server`), audio en WAV 16 kHz, une requête à la fois (le cœur sérialise). Le port n'est jamais exposé au réseau extérieur (écoute sur loopback uniquement).
- **Cœur ↔ File de tâches** : file JSON sur disque (répertoire appdata) — permet de reprendre une file après redémarrage.

---

## 3. Stockage et modèle de données

### 3.1 Principe directeur : le dossier de notes est la source de vérité

L'utilisateur peut ouvrir, éditer, déplacer, supprimer ses notes dans l'Explorateur Windows, Obsidian ou n'importe quel éditeur. L'app s'adapte, jamais l'inverse.

| Donnée | Emplacement | Rôle |
|---|---|---|
| **Notes** (`.md`) | Dossier choisi par l'utilisateur | Source de vérité. Lisible partout, synchronisable (Dropbox, Syncthing, Obsidian) |
| **Index de recherche** (`index.db`, SQLite) | `%APPDATA%/Écho/` | Cache reconstructible : plein texte, étiquettes, dates, miniatures. Si supprimé → reconstruction par scan du dossier |
| **Paramètres** (`settings.json`) | `%APPDATA%/Écho/` | Dossier des notes, raccourcis, modèle, préférences |
| **Modèles Whisper** (`.bin`) | `%LOCALAPPDATA%/Écho/models/` | Téléchargés une fois, vérifiés (hash SHA-256) |
| **Audio temporaire** | `%TEMP%/Écho/` | WAV de session, supprimé après transcription (sauf conservation activée) |
| **Audio conservé** (option) | `<dossier notes>/.echo/audio/` | Optionnel, désactivé par défaut, activé par note ou globalement |

### 3.2 Arborescence du dossier de notes (par défaut, configurable)

```
MesNotes/
├── 2026-10-19.md              ← note du jour (mode "journal")
├── 2026-10-18.md
├── notes/
│   ├── 2026-10-15T1423-reunion-equipe.md   ← notes ponctuelles
│   └── 2026-10-02T0915-idee-app.md
├── .echo/
│   ├── audio/                  ← WAV conservés (option)
│   └── trash/                  ← corbeille interne (30 jours)
└── .echo-ignore                ← patterns à exclure de l'index
```

### 3.3 Format d'une note

```markdown
---
id: 8f3c1a2e-9d4b-4c6a-b1e0-3f2d5a7c9b8d
created: 2026-10-19T08:41:12+02:00
modified: 2026-10-19T08:41:12+02:00
tags: [idée, projet-echo]
type: journal        # journal | note | reunion | tache | import
source: dictée      # dictée | frappe | import-audio
engine: whisper-small (multilingue, langue fr)
---

# 2026-10-19

## 08:41
Rappel de l'idée : ajouter un mode push-to-talk maintenu...

## 08:47
*dictée brute conservée en commentaire ci-dessous*
<!-- audio: 2026-10-19T084703.wav -->
Penser à tester la fenêtre flottante sur écran secondaire...
```

- L'en-tête YAML (`frontmatter`) est optionnel et réparable : si absent, l'app le régénère à la première modification.
- Chaque entrée dictée est horodatée (`## HH:MM`).
- La **transcription brute** est toujours conservée : en commentaire HTML dans la note, **et** de façon canonique dans `<dossier>/.echo/sessions/` (JSON : texte brut, segments horodatés, moteur, modèle). Ce fichier latéral est la source canonique des métadonnées de session : comme il vit à côté des notes et non dans SQLite, l'index reste 100 % reconstructible sans perdre les données de transcription.

### 3.4 Schéma de l'index SQLite

```sql
CREATE TABLE notes (
    id TEXT PRIMARY KEY,            -- UUID stable, dans le frontmatter
    path TEXT UNIQUE NOT NULL,      -- chemin relatif au dossier racine
    title TEXT, created TEXT, modified TEXT,
    type TEXT, tags TEXT,           -- tags en JSON
    content_hash TEXT,              -- détecte les modifications externes
    indexed_at TEXT
);
CREATE VIRTUAL TABLE notes_fts USING fts5(
    note_id UNINDEXED, body        -- table virtuelle FTS5 (recherche plein texte)
);
CREATE TABLE segments (             -- miroir indexé des sessions .echo/, reconstruit depuis elles
    id INTEGER PRIMARY KEY, note_id TEXT, t_start REAL, t_end REAL, text TEXT
);
```

L'index SQLite ne contient jamais de donnée non reconstruisible : les sessions (transcriptions brutes, segments) vivent dans `.echo/sessions/`, l'index n'en est que le miroir.

### 3.5 Règles de cohérence (comportements attendus)

| Situation | Comportement |
|---|---|
| Note modifiée dans un éditeur externe | L'index se met à jour au prochain scan (watcher de fichiers + scan au démarrage et toutes les 60 s). Le `content_hash` détecte le changement. Conflit d'édition simultanée : **pas de « dernière écriture gagne » aveugle** — avant chaque écriture, le cœur vérifie le hash du fichier sur disque ; s'il a changé depuis le dernier chargement, l'app écrit sa version en `nom-de-note.conflit-YYYYMMDD-HHMM.md` et avertit l'utilisateur, sans jamais écraser silencieusement |
| Note supprimée extérieurement | Disparaît de l'index ; si elle était ouverte dans l'UI, l'onglet affiche « fichier supprimé » avec option de restaurer depuis `.echo/trash/` ou le journal de récupération |
| Écriture interrompue (crash, coupure) | **Deux mécanismes combinés** : (1) journal de récupération persistant (`recovery.log`, append-only dans appdata) : chaque segment confirmé à l'écran y est inscrit avant d'être rapproché de la note — au redémarrage, tout ce qui y figure est réinséré ; (2) écriture atomique adaptée à Windows : écriture dans un fichier temporaire du **même volume** que la cible, puis remplacement via `ReplaceFile` (préserve attributs et liens), jamais d'écrasement direct. L'UI n'affiche « Enregistré » qu'après persistance effective (flush) |
| Dossier sur un cloud sync (Dropbox/OneDrive) | Compatible (fichiers plats .md), mais l'audio conservé peut générer des conflits de synchronisation → avertissement à l'activation. Aucune garantie de fusion parfaite, le format .md limite les dégâts |
| Dossier introuvable au démarrage | Mode « orphelin » : l'app affiche un écran de reconfiguration, garde les notes en mémoire, propose la localisation du dossier déplacé |

---

## 4. Parcours utilisateur de bout en bout

### 4.1 Parcours A — Premier lancement

1. L'app démarre, aucun dossier configuré → écran d'accueil : « Choisissez votre dossier de notes » (bouton natif via plugin `dialog`).
2. Téléchargement du modèle proposé selon le matériel détecté (CPU seul → `small` quantifié léger ; GPU détecté → `medium`). Barre de progression, hash vérifié, possibilité d'annuler.
3. Test micro : « Dites une phrase pour vérifier » → transcription affichée → confiance installée.
4. Écran « prêt » avec le raccourci par défaut `Ctrl+Alt+Espace` et un bouton « essayer maintenant ».
5. L'app se réduit dans le tray.

**Erreurs couvertes** : micro refusé (guide vers les réglages Windows de confidentialité), pas d'Internet (lien direct vers téléchargement manuel du modèle), disque plein.

### 4.2 Parcours B — Dictée vers la note du jour (cœur du produit)

1. `Ctrl+Alt+Espace` (ou clic bouton micro, ou maintien de la touche en push-to-talk).
2. La fenêtre flottante apparaît en bas de l'écran ; indicateur rouge « ● Enregistrement » + niveau audio (VU-mètre).
3. L'utilisateur parle. VAD Silero : l'enregistrement peut s'arrêter automatiquement après 2 s de silence (option) ou via un second appui.
4. Transcription (objectif de latence : voir §5.4).
5. Le texte est inséré dans la note du jour à l'endroit du curseur, précédé de l'horodatage. Sauvegarde immédiate via journal de récupération + écriture atomique (§3.5).
6. L'UI affiche le texte final avec le bouton « Modifier » (réécoute de l'audio si conservé, édition manuelle).

**États affichés** : `Prêt → Enregistrement → Traitement → Terminé` (+ `Annulé`, `Erreur`).

### 4.3 Parcours C — « Dicter ailleurs » (dans DeepSeek, Slack, un mail…)

1. L'utilisateur est dans une autre application (focus sur cette app).
2. `Ctrl+Alt+V` : l'app capture la **cible** : identifiant de la fenêtre au premier plan (`GetForegroundWindow`) + nom du processus. Enregistrement, transcription.
3. **Avant toute insertion, re-vérification de la cible** : la fenêtre au premier plan doit toujours être la même (même handle + même processus). **Si le focus a changé pendant la transcription : aucun collage automatique** — le texte est affiché dans la fenêtre flottante avec un bouton « Copier », et une notification l'explique.
4. Insertion du texte (si cible inchangée) :
   - **Stratégie 1** : simulation de frappe clavier (`SendInput` sur Windows) — rapide, marche dans la plupart des apps, mais peut être interceptée par les jeux ou les apps admin.
   - **Stratégie 2 (repli)** : copie dans le presse-papiers + `Ctrl+V` simulé.
   - **Stratégie 3 (toujours disponible, dès la v1)** : bouton « **Copier** » dans la fenêtre flottante — insertion manuelle par l'utilisateur. C'est le garant de base : quoi qu'il arrive, le texte est récupérable d'un clic.
5. Une notification discrète confirme l'insertion ; « Rappeler la dernière dictée » est proposé via le tray.

**Et les dictées vers d'autres apps, sont-elles dans les notes ?** Par défaut : non, elles ne créent pas de note (c'est du texte « jetable » vers l'app cible), mais elles restent 24 h dans « dictées récentes » (tray) pour copie ou transformation en note. Option : « archiver aussi chaque dictée externe dans le journal du jour ».

**Point d'honnêteté** : l'insertion universelle n'existe pas (applications protégées, sessions admin, machines distantes/RDP). C'est pour ça que les trois stratégies existent et que la stratégie 3 est disponible dès la v1, même si l'insertion automatique arrive en v2.

### 4.4 Parcours D — Import d'un enregistrement

1. Glisser-déposer d'un fichier audio (MP3, M4A, OGG, WAV) sur la fenêtre, ou « Importer ».
2. File d'attente visible : nom, durée, progression.
3. Transcription en tâche de fond (worker), note créée avec `type: import`.
4. **Politique audio claire et distincte du reste** : le **fichier source reste où il est** (l'app ne le déplace ni ne le copie — elle le lit) ; une **copie temporaire** de travail est purgée après transcription. La **conservation d'une copie dans `.echo/audio/`** suit la même politique que partout ailleurs : désactivée par défaut, activée explicitement — pour un import, l'utilisateur a déjà l'original chez lui, la copie n'a donc pas d'intérêt par défaut.

### 4.5 Parcours E — Recherche et relecture

1. `Ctrl+Alt+F` : palette de recherche (style Spotlight) en plein écran léger.
2. Recherche plein texte instantanée (FTS5) sur toutes les notes ; résultats avec extrait surligné.
3. Flèches pour naviguer, Entrée pour ouvrir la note au bon segment (les timestamps de segments permettent d'ouvrir l'audio à la bonne seconde si conservé).

---

## 5. Le moteur de transcription

### 5.1 Machine à états

```
PRÊT ──start──▶ ENREGISTREMENT ──stop──▶ TRAITEMENT ──▶ TERMINÉ
  ▲                │ │                      │             │
  │   (pause)      │ └─ PAUSE/REPRISE      ├─ annuler ──▶ ANNULÉ
  │                │                        └─ erreur ──▶ ERREUR ──réessayer──▶ PRÊT
  └────────────────┴────────────────────────────────────────┘
```

### 5.2 Pipeline de traitement

1. **Capture** : WAV 16 kHz mono (format natif Whisper).
2. **VAD** (Silero, intégré à whisper.cpp) : supprime le silence, découpe en segments de parole — accélère le traitement et permet l'affichage progressif ([VAD dans whisper.cpp](https://github.com/ggml-org/whisper.cpp/)).
3. **Transcription brute** (whisper.cpp, modèle selon matériel).
4. **Post-traitement local léger** (règles, pas d'IA) : ponctuation de base, majuscules en début de phrase, correction des mots du dictionnaire personnel (voir §7.2), suppression des hésitations (« euh », « tu vois ») — *optionnel et désactivable*.
5. **Nettoyage IA (optionnel, phase 2)** : reformulation, structuration. Toujours sur une copie : la version brute reste dans la note (voir §3.3).

### 5.3 Gestion des modèles

| Modèle | Taille approx. | Cible | Quand |
|---|---|---|---|
| `tiny` / `base` | 75–150 Mo | PC très modeste, aperçu instantané | Option « vitesse » |
| `small` | ~460 Mo | Défaut CPU | Recommandé |
| `medium` | ~1,5 Go | PC avec GPU ou bon CPU | Option « précision » |
| `large-v3` | ~3 Go | PC puissants | Option expert |

Précision : tous ces modèles sont **multilingues** (pas de « modèle français » dédié) — la langue est un **paramètre** (`language: fr` ou auto-détection), choisi par l'app au moment de l'inférence. On écrit donc « modèle multilingue `small`, langue `fr` », jamais « small FR ».

Mise en garde quantification : éviter la quantification entière agressive (Q4) sur les modèles utilisés par défaut — les mesures publiées montrent une dégradation marquée de la précision dans cette configuration ([benchmark Göttingen, 2026](https://hps.vi4io.org/_media/research/labs/2026/2026-02-deepak_budha-performance_benchmarking_and_evaluation_of_open_source_asr_frameworks_for_real_time_transcription-report.pdf)) ; préférer F16 ou une quantification légère (Q5/Q8).

- Téléchargement à la première utilisation avec vérification SHA-256, changement de modèle à chaud (redémarrage du processus moteur), suppression propre.
- Si une implémentation `faster-whisper` est ajoutée plus tard, les modèles (CT2) sont téléchargés séparément — l'interface `TranscriptionEngine` rend ce changement transparent pour le reste de l'app.

### 5.4 Objectifs de performance (à mesurer, pas des promesses)

Deux latences distinctes, mesurées séparément :

| Métrique | Objectif sur PC moyen (CPU 4 cœurs, AVX2) |
|---|---|
| Activation du micro (raccourci → VU-mètre actif) | < 300 ms, modèle déjà chargé en mémoire |
| Fin de parole → texte affiché (note < 30 s) | < 2 s, modèle `small` déjà chargé |
| Démarrage à froid de l'app (tray, modèle non chargé) | < 1,5 s ; chargement du modèle en tâche de fond ensuite |
| Transcription d'1 h d'audio (import) | < 15 min en tâche de fond |
| Empreinte mémoire au repos | < 150 Mo (hors modèle) |

Ces chiffres sont des **objectifs à valider par mesure** sur une machine de référence, pas des résultats acquis.

---

## 6. Confidentialité et sécurité

### 6.1 Principes

1. **Local par défaut.** L'audio ne quitte jamais le PC sans action explicite. Le seul accès réseau par défaut est le téléchargement initial du modèle.
2. **Audio éphémère.** Conservation désactivée par défaut ; le WAV de session est supprimé dès la fin de la transcription. L'activation de la conservation est un choix clair, par note ou global.
3. **Notes jamais dans les logs.** Les journaux techniques (crash reports locaux) ne contiennent jamais le contenu des notes ni les transcriptions — seulement des identifiants et codes d'erreur.
4. **Cloud opt-in, explicite.** Si un mode cloud est ajouté (meilleure qualité, nettoyage IA puissant), il est : désactivé par défaut, activé par un interrupteur dédié, accompagné d'un encart « ce qui sera envoyé », et le statut est visible en permanence dans le tray (icône « nuage » quand actif).
5. **Pas d'automatisme dangereux.** Aucune exécution d'action, aucun envoi, aucune interprétation automatique déclenchée par une transcription ambiguë. Les « commandes vocales » (§7.2) ne font que ce qui est explicitement listé dans les paramètres.

### 6.2 Mesures techniques

- Écoute HTTP du moteur sur `127.0.0.1` uniquement, port aléatoire, token local.
- **Hygiène Tauri** : permissions minimales par fenêtre (capabilities par fenêtre, pas globales), accès fichiers restreints par *scopes* explicites (le dossier des notes + appdata, rien d'autre), rendu Markdown **sans contenu actif** (pas de HTML/JS arbitraire dans les notes, pas de liens cliquables exécutant des actions — les notes sont du texte, pas une page web).
- Pas de télémétrie par défaut ; si télémétrie opt-in un jour : anonyme, désactivable, documentée.
- Mises à jour signées via le plugin `updater` officiel de Tauri ([plugins Tauri](https://v2.tauri.app/blog/tauri-20/)).
- Verrouillage optionnel de l'app (Windows Hello) pour les notes sensibles.

---

## 7. Catalogue des fonctionnalités

Regroupées par usage. Chaque fonction : bénéfice, comportement, dépendances, phase.

### 7.1 Capture (le cœur)

| # | Fonctionnalité | Détails | Phase |
|---|---|---|---|
| 1 | **Dictée clic** | Bouton micro dans la fenêtre flottante et le tray : clic = démarrer, second clic = arrêter | v1 |
| 2 | **Push-to-talk maintenu** | Maintenir `Ctrl+Alt+Espace` : on parle, on relâche, ça transcrit. Le plugin global-shortcut de Tauri expose les états Pressed/Released, exactement ce qu'il faut ([doc](https://v2.tauri.app/plugin/global-shortcut/)) | v1 |
| 3 | **Pause / reprise** | Pendant l'enregistrement, bouton pause (couper le micro sans clôturer la note) | v2 |
| 4 | **Arrêt automatique au silence** | VAD : arrêt après N secondes de silence (configurable, 2 s par défaut) | v1 |
| 5 | **Repères pendant l'enregistrement** | Toucher `F8` (ou double-clic VU-mètre) pose un marqueur dans l'audio : « j'ai un doute sur ce passage ». À la relecture, les marqueurs sont surlignés | v3 |
| 6 | **Import audio/vidéo** | Glisser-déposer MP3/M4A/OGG/WAV (et MP4 audio), file d'attente, transcription en tâche de fond | v2 |
| 7 | **Dictée continue en arrière-plan** | Mode « journal de bord » : micro qui tourne par tranches, chaque silence crée un paragraphe horodaté dans la note du jour. Pour les marches, le brainstorming oral | v3 |
| 8 | **Capture depuis le tray sans fenêtre** | Dictée « invisible » : raccourci → petit compteur en haut de l'écran, rien d'autre, insertion directe là où est le focus | v2 |

### 7.2 Écriture et qualité du texte

| # | Fonctionnalité | Détails | Phase |
|---|---|---|---|
| 9 | **Dictionnaire personnel** | Liste de noms propres, sigles, jargon : « Ngian », « Doische », « faster-whisper ». Le post-traitement corrige les erreurs de reconnaissance fréquentes (« Ngi An » → « Ngian ») via règles de substitution apprises des corrections manuelles | v2 |
| 10 | **Apprentissage des corrections** | Quand l'utilisateur édite une transcription, le diff (avant/après) alimente des suggestions : « Tu corriges souvent "sa" en "ça", appliquer partout ? » | v3 |
| 11 | **Ponctuation et hésitations** | Suppression configurable des hésitations (« euh », « tu vois », « genre »), ponctuation automatique — désactivable pour garder le verbatim | v1 (simple) / v2 (config) |
| 12 | **Commandes vocales locales** | Phrases reconnues en priorité : « nouvelle note », « titre : … », « point d'interrogation », « virgule », « paragraphe ». Liste fixe, éditable, traitée localement par correspondance de texte — pas d'IA ambigüe | v3 |
| 13 | **Templates de notes** | Modèles : idée / réunion / tâche / journal / bug / appel. Un template = frontmatter + sections préremplies. Un template de réunion peut être déclenché par la commande vocale « nouvelle réunion » | v2 |
| 14 | **Extraits réutilisables (snippets)** | Phrases fréquentes insérées par commande vocale ou raccourci (« signature », « disclaimer ») | v3 |
| 15 | **Multi-langue par note** | Détection automatique FR/EN par segment ; une réunion en anglais et une note en français cohabitent | v2 |

### 7.3 Organisation et recherche

| # | Fonctionnalité | Détails | Phase |
|---|---|---|---|
| 16 | **Note du jour (journal)** | `AAAA-MM-JJ.md`, entrées horodatées s'ajoutent au fil de la journée — le fichier du jour s'ouvre par défaut | v1 |
| 17 | **Historique latéral** | Liste des 20 dernières notes avec aperçu, recherche rapide | v1 |
| 18 | **Étiquettes et favoris** | Tags dans le frontmatter (`#idée` en dictée crée le tag), favoris épinglés | v2 |
| 19 | **Recherche plein texte instantanée** | Palette `Ctrl+Alt+F` sur SQLite FTS5, extrait surligné, ouverture à la position | v2 |
| 20 | **Recherche sémantique locale (option)** | Index d'embeddings calculés localement (petit modèle local type sentence-transformers quantifié) : « trouve mes notes où je parle de trucs liés au micro ». Coût : index plus lourd, construction en tâche de fond | v4 |
| 21 | **Liens entre notes** | `[[wiki-links]]` façon Obsidian (format compatible Obsidian par nature du .md) | v3 |
| 22 | **Corbeille douce** | Suppression → `.echo/trash/` pendant 30 jours, restauration en un clic | v2 |
| 23 | **Onboarding du dossier existant** | Pointeur vers un dossier Obsidian existant : l'app indexe sans rien changer, les wiki-links et tags Obsidian sont respectés | v2 |

### 7.4 Exploitation et IA (optionnel, local d'abord)

| # | Fonctionnalité | Détails | Phase |
|---|---|---|---|
| 24 | **Sélection → actions** | Sélectionner du texte → menu : reformuler / résumer / extraire les tâches / corriger / traduire. Moteur : d'abord règles locales + modèle LLM local (llama.cpp, option expert), ou API cloud opt-in | v3 |
| 25 | **Comparaison avant/après** | Vue côte-à-côte transcription brute ↔ note nettoyée, bascule en un clic — la confiance vient de la transparence | v3 |
| 26 | **Extraction de tâches** | « Transforme cette note en checklist » : chaque élément devient une tâche `- [ ]` avec tag `#tâche`. Vue « tâches ouvertes » agrégée sur toutes les notes | v3 |
| 27 | **Résumé quotidien** | Le soir, résumé de la note du jour en 5 points (local si LLM local configuré, sinon opt-in cloud) | v4 |
| 28 | **Envoi structuré** | « Envoie ça par mail » / « crée un draft Slack » : génère le texte, ouvre l'app avec un brouillon prérempli — jamais d'envoi automatique | v4 |

### 7.5 Confort, intégration et différenciation

| # | Fonctionnalité | Détails | Phase |
|---|---|---|---|
| 29 | **Profils par application** | Règles selon l'app au premier plan : dans Slack → insertion directe sans horodatage ; dans Obsidian → avec tags ; dans le bloc-notes Écho → horodatage complet. Config simple : « quand je dicte dans X, fais Y » | v3 |
| 30 | **Mode « ne pas écouter »** | Interruption totale (pause micro + raccourcis désactivés) pendant X minutes ou jusqu'à réactivation — pour les appels confidentiels. Icône tray barrée, impossible à oublier | v2 |
| 31 | **Lancement automatique** | Démarrage avec Windows (plugin `autostart` Tauri), démarrage en mode silencieux dans le tray | v1 |
| 32 | **Mise à jour automatique** | Updater signé Tauri, notification discrète, pas de redémarrage forcé | v2 |
| 33 | **Accessibilité** | Raccourcis entièrement personnalisables (conflits détectés), taille de police, contraste élevé, lecture vocale des confirmations (TTS) pour malvoyants | v2/v3 |
| 34 | **Mode concentration** | La fenêtre flottante se réduit à un seul VU-mètre + bouton : dictée « sans distraction » | v3 |
| 35 | **Statistiques personnelles** | « Cette semaine : 47 notes, 2 h 15 de dictée, 18 000 mots » — données 100 % locales, motivation gamifiée légère | v4 |
| 36 | **Observatoire de confidentialité** | Écran toujours accessible : ce qui est stocké où, ce qui a quitté le PC (rien, par défaut), bouton « tout purger » (audio, index, cache — jamais les notes) | v2 |
| 37 | **Sauvegarde chiffrée de l'audio** | Si conservation activée : option chiffrement AES-256 du dossier `.echo/audio/` (clé locale protégée par DPAPI Windows) | v4 |
| 38 | **Portage mobile (companion)** | Envoi d'une note vocale depuis le téléphone vers le dossier (via service local sur le réseau ou fichier partagé) — pas une app mobile complète | v5+ |

### 7.6 Fonctionnalités écartées (et pourquoi)

- **Synchronisation cloud intégrée** : les fichiers .md sont déjà synchronisables par Dropbox/Syncthing/Obsidian Sync ; réimplémenter ça est un piège à conflits.
- **Transcription temps réel mot à mot en continu** : spectaculaire mais gourmand et anxiogène ; le flux « parle → 1 s → texte » suffit. À revisiter si la demande existe.
- **Plugin système généralisé** : interface unique `TranscriptionEngine` suffit ; un marketplace de plugins est une usine à gaz pour une v1.

---

## 8. Spécification technique

### 8.1 Arborescence du projet

```
echo/
├── src/                          # UI TypeScript
│   ├── windows/
│   │   ├── main/                 # bloc-notes + historique
│   │   ├── quick/                # fenêtre flottante de dictée
│   │   ├── settings/
│   │   └── palette/               # recherche (Ctrl+Alt+F)
│   ├── stores/                   # état (notes, session, settings)
│   └── lib/ipc.ts                # typage des commandes/événements
├── src-tauri/                    # cœur Rust
│   ├── src/
│   │   ├── commands/             # invoke handlers (notes, audio, settings)
│   │   ├── capture/              # session micro, VU-mètre, pause
│   │   ├── transcription/
│   │   │   ├── mod.rs            # trait TranscriptionEngine
│   │   │   ├── whisper_cpp.rs    # impl locale (HTTP loopback)
│   │   │   └── cloud.rs          # impl opt-in
│   │   ├── notes/                # parseur frontmatter, écriture atomique
│   │   ├── watcher/              # scan dossier, hash, index
│   │   └── tray.rs / shortcuts.rs
│   ├── binaries/                 # whisper-server empaqueté par plateforme
│   └── tauri.conf.json
├── docs/adr/                     # décisions d'architecture (ADR)
└── tests/e2e/
```

### 8.2 Contrats IPC principaux

```rust
// Commandes (UI → cœur)
invoke("start_capture", { mode: "push" | "click", target: "note" | "active-app" })
invoke("stop_capture") → CaptureResult
invoke("transcribe_file", { path }) → TaskId
invoke("get_note", { date_or_id }) → Note
invoke("append_to_note", { id, text, timestamped: bool })
invoke("search", { query, limit }) → Vec<SearchHit>
invoke("get_settings") / invoke("set_settings", { ... })
invoke("select_notes_folder") → Path   // via plugin dialog
invoke("download_model", { id }) → TaskId
invoke("cancel_task", { task_id })
invoke("insert_text_in_active_app", { text, strategy: "keyboard"|"clipboard"|"manual", target_check: { window_handle, process_name } })
// la cible est re-vérifiée côté cœur AVANT l'insertion (§4.3) : si le focus a changé, l'insertion est refusée et l'événement `insertion:skipped` est émis

// Événements (cœur → UI)
"capture:level"        { rms: f32 }                    // VU-mètre, ~30 Hz
"transcription:progress" { task_id, percent }   // estimation par durée traitée ; pas de texte partiel en v1 (§2.3)
"insertion:skipped"    { task_id, reason: "focus-changed" }
"transcription:done"   { task_id, text, segments, engine }
"transcription:error"  { task_id, code, message }
"note:changed"         { id, reason: "external"|"app" }
"engine:status"        { state: "ready"|"restarting"|"error" }
```

### 8.3 Schémas de données clés

```rust
struct CaptureResult {
    wav_path: PathBuf, duration_secs: f32,
    target: CaptureTarget,          // Note { id } | ActiveApp { window_handle, process_name }
    started_at: DateTime<Local>,
}

struct TranscriptionResult {
    raw_text: String,               // jamais jeté
    cleaned_text: Option<String>,   // si post-traitement
    segments: Vec<Segment>,         // t_start, t_end, text
    language: String, confidence_avg: f32,
    engine: EngineInfo, model: String,
}

struct Note { id: Uuid, path: PathBuf, frontmatter: Frontmatter, body: String }
```

### 8.4 Installation, mises à jour, migrations

- **Distribution** : installateur NSIS/MSI (Tauri), ~10–15 Mo sans modèle ; le modèle se télécharge au premier lancement (ou bundle « offline » optionnel avec le modèle `small` inclus).
- **Mises à jour** : plugin `updater`, releases signées ; le worker et les binaires whisper sont versionnés avec l'app.
- **Migrations** : l'index SQLite porte un `schema_version` ; toute migration se fait en recréant l'index depuis les fichiers (puisque l'index est un cache) — pas de migration de données utilisateur, la source de vérité (les .md) est immuable de format.

### 8.5 Bibliothèques Rust envisagées

- `cpal` (capture audio multiplateforme), `hound` (WAV), `notify` (watcher fichiers), `rusqlite` + FTS5 (index), `serde`/`toml` (settings), `uuid`, `sha2` (vérification modèles), `windows` crate pour `SendInput` et DPAPI.

---

## 9. Tests et critères d'acceptation

### 9.1 La promesse centrale, testée en priorité

| Test | Critère d'acceptation |
|---|---|
| Crash brutal pendant la sauvegarde | Au redémarrage, la note du jour contient tout ce qui a été confirmé à l'écran — garanti par le journal de récupération persistant (§3.5), pas par la mémoire vive |
| Coupure de courant pendant transcription | Aucune note corrompue ; l'audio temporaire est re-transcrit ou proprement supprimé |
| Refus du micro (permissions Windows) | Message clair + lien vers les réglages de confidentialité Windows ; jamais de boucle d'erreur |
| Modèle absent / corrompu (hash invalide) | Re-téléchargement proposé ; l'UI reste utilisable en mode bloc-notes sans dictée |
| Disque plein | Sauvegarde en échec → avertissement visible, texte conservé en mémoire, rien n'est perdu silencieusement |
| Note modifiée/supprimée dans un éditeur externe | L'index se resynchronise ; l'onglet ouvert propose restauration ou rechargement |
| Perte de focus pendant « dicter ailleurs » | Repli automatique sur la stratégie presse-papiers + notification « texte copié » |
| Raccourci déjà pris par une autre app | Détection au démarrage + assistant de choix d'un raccourci libre |

### 9.2 Tests fonctionnels par phase

- **v1** : parcours A, B complets ; push-to-talk ; note du jour ; arrêt au silence ; écriture atomique vérifiée (kill -9 pendant 1 000 sauvegardes).
- **v2** : parcours C (3 stratégies d'insertion) ; import audio ; recherche FTS ; dictionnaire personnel ; « ne pas écouter ».
- **v3+** : commandes vocales (faux positifs < 1 %) ; modifications externes ; mode journal de bord.

### 9.3 Critères UX mesurés

- Activation du micro < 300 ms au p95, et fin de parole → texte < 2 s au p95 sur machine de référence (deux mesures séparées, cf. §5.4).
- Zéro donnée envoyée (vérifié par capture réseau en mode par défaut : seules les requêtes de téléchargement du modèle au premier lancement).

---

## 10. Roadmap de livraison

| Phase | Contenu | Objectif |
|---|---|---|
| **v1 — La boucle parfaite** | Tray + fenêtre flottante + bloc-notes, note du jour, dictée clic + push-to-talk + arrêt au silence, dossier choisi, sauvegarde atomique, modèle `small`, lancement auto | « Je ne perds plus jamais une idée » |
| **v2 — L'habitude** | Dicter ailleurs (3 stratégies), import audio, recherche, tags/favoris, dictionnaire personnel, templates, « ne pas écouter », updater, observatoire de confidentialité | Usage quotidien dans toutes les apps |
| **v3 — L'intelligence douce** | Commandes vocales, apprentissage des corrections, liens inter-notes, mode journal de bord, profils par application, extraction de tâches, comparaison avant/après | L'app gagne en finesse sans perdre la confiance |
| **v4 — La puissance** | Recherche sémantique locale, résumés, LLM local optionnel (llama.cpp), statistiques, chiffrement audio | Pour les utilisateurs avancés |
| **v5+** | Companion mobile (envoi de note vocale), portage macOS/Linux | Élargissement |

---

## Sources

- [Tauri 2.0 — annonce de la version stable](https://v2.tauri.app/blog/tauri-20/) — architecture plugins officiels (global-shortcut, tray, clipboard, autostart, updater, dialog, fs).
- [Tauri — plugin Global Shortcut](https://v2.tauri.app/plugin/global-shortcut/) — API raccourcis globaux, états Pressed/Released, permissions.
- [whisper.cpp (ggml-org)](https://github.com/ggml-org/whisper.cpp/) — implémentation C/C++, VAD Silero, quantification, backends GPU (CUDA, Vulkan, OpenVINO), serveur HTTP à API compatible OpenAI.
- [Benchmark ASR temps réel, Université de Göttingen (2026)](https://hps.vi4io.org/_media/research/labs/2026/2026-02-deepak_budha-performance_benchmarking_and_evaluation_of_open_source_asr_frameworks_for_real_time_transcription-report.pdf) — précision de faster-whisper comparable au Whisper original ; les chiffres de WER publiés comparent des configurations différentes, d'où la recommandation de comparer à paramètres identiques avant tout choix définitif.
