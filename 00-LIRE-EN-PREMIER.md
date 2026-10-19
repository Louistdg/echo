# Écho — dossier de travail

Application de bureau de capture vocale locale (Tauri 2 + SvelteKit + Rust).
Créé le 19/10/2026 sous le nom BOCNOT, renommé **Écho** le même jour.
Chemin : `D:\DSH\default-workspace\Écho`.

## Fichiers

| Fichier | Rôle |
|---|---|
| `00-LIRE-EN-PREMIER.md` | ce fichier : état vérifié, commandes, limites, suite |
| `01-PROMPT-A-COLLER-SOCLE.txt` | contrat de travail (méthode), recopié octet pour octet — voir la réserve plus bas |
| `README.md` | README du squelette + écart structurel avec le plan |
| `docs/architecture.md` | plan d'architecture v1.0 |
| `src-tauri/src/recovery.rs` | journal de récupération append-only |
| `src-tauri/src/atomic.rs` | écriture atomique (fichier temporaire + `ReplaceFileW`) |
| `src/`, `src-tauri/`, `static/` | le reste du code |

## Le contrat en vigueur

1. Pas de question, sauf deux cas : deux chemins irréversibles indistinguables ; un fait
   qu'aucune mesure ne peut produire. Alors : une question, deux ou trois options, une
   recommandation.
2. Mesurer avant d'affirmer. Disque et réseau sont accessibles : lire, lancer, observer.
3. Jamais « c'est fait » sans la commande et sa sortie brute. Sinon : « non vérifié ».
4. Un contrôle qui passe avant et après ne prouve rien : le montrer échouer, puis passer.
5. Ne pas construire plus que demandé. Corriger l'ancien plutôt que le surcharger.
6. Ne pas sortir du périmètre de fichiers donné ; si nécessaire, s'arrêter et le dire.
7. Ne jamais inventer un fait manquant : « je ne sais pas » puis « voilà ce qu'il me faudrait ».
8. Contredire avec une mesure quand un fait de la demande est faux.
9. Signaler ses propres limites dans le rapport.
10. Ne jamais contourner un refus de sécurité.
11. Bloqué plus de 20 minutes : une ligne, où, quoi, ce qu'il faudrait.
12. Écrire court : résultat, preuve, limites. Chiffres avec unité.

## Lancer et tester

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cd D:\DSH\default-workspace\Écho
npm run check                  # TypeScript + Svelte
cd src-tauri; cargo test       # 16 tests
cargo build; cd ..
npm run tauri dev              # ouvre l'application
```

Test d'écriture indestructible (harnais externe, hors dépôt) :
`node D:\DSH\default-workspace\shots\echo\kill-test.mjs`
Il faut un serveur Vite (`npm run dev`) et `settings.json` pointant vers un dossier de test.

## M1 — Écriture indestructible (19/10/2026)

### Ce qui a été construit

- **Journal** (`recovery.rs`) : `recovery.log` dans `%APPDATA%\com.echo.notes`, une ligne
  JSON par entrée (`id`, `note`, `time`, `text`, `state`), `sync_data()` après chaque
  ligne. `pending()` ignore les entrées ayant reçu un `done`. Compaction : garde tous les
  `pending`, et les `done` de moins de 30 jours.
- **Écriture atomique** (`atomic.rs`) : fichier temporaire sur le même volume, `sync_all`,
  puis `ReplaceFileW`. Jamais d'écrasement en place.
- **Note** (`notes/mod.rs`) : chaque entrée porte `<!--echo:ID-->` (invisible au rendu).
  `append_entry` ne réécrit pas une entrée dont le marqueur est déjà là — c'est
  l'idempotence qui protège du doublon quand la note est écrite mais que le `done` manque.
- **Ordre imposé** (`commands/mod.rs`) : journal → note → `done`. L'UI n'affiche
  « Enregistré » que sur la valeur renvoyée par ce chemin ; une erreur affiche
  « NON ENREGISTRÉ » et le texte reste dans la zone de saisie.
- **Rejeu au démarrage** (`lib.rs`) : `replay_pending` avant toute interaction.
- **BOM** : `settings.json` avec BOM est lu correctement ; l'écriture n'en produit jamais.

### Preuves

Tests unitaires — `cargo test` :

```
test result: ok. 16 passed; 0 failed; 0 ignored
```

dont : idempotence du rejeu, compaction, ligne corrompue ignorée, BOM lu, rejeu dans une
note absente, accents préservés.

**Preuve négative.** J'ai désactivé le contrôle de marqueur (`if false && …`) et relancé
le test d'idempotence :

```
thread 'notes::tests::rejoue_sans_dupliquer_la_meme_entree' panicked at src\notes\mod.rs:94:
assertion `left == right` failed
  left: 3
 right: 1
test result: FAILED
```

Restauré, `cargo test` repasse à 16/16. Le test détecte donc bien la régression.

**Test décisif** — 100 ajouts, `taskkill /F /IM echo.exe` à des instants aléatoires
(0–25 ms après l'appel), redémarrage, rejeu. Deux exécutions complètes :

```
{ "entrees_attendues": 100, "arrets": 21, "incidents": 22,
  "manquantes": [], "doubles": [], "marqueurs": 100, "ok": true }

{ "entrees_attendues": 100, "arrets": 17, "incidents": 16,
  "manquantes": [], "doubles": [], "marqueurs": 100, "ok": true }
```

Note finale : 100 entrées `## `, 100 marqueurs, 100 identifiants uniques, accents intacts,
frontmatter intact. Journal après coup : 100 `pending` + 100 `done` (compactage à 30 jours,
donc conservé aujourd'hui).

### Ce que la première exécution a appris

La v1 du harnais échouait sur l'entrée 085 avec des résultats vides. Ce n'était pas le
produit : le harnais prenait une cible CDP zombie pour une application prête. Corrigé en
vérifiant `window.__TAURI_INTERNALS__` avant chaque appel et en attendant la disparition
réelle du processus après `taskkill`. La v2 passe deux fois de suite.

## État du reste

| Élément | État |
|---|---|
| Squelette, deux fenêtres, tray, `Ctrl+Alt+Espace` | vérifié (voir historique ci-dessous) |
| Journal de récupération, écriture atomique, BOM | vérifié (M1) |
| Icône du tray visible | vérifié : `Écho — notes vocales`, `SystemTray.NormalButton` |
| Menu du tray : « Ouvrir Écho », « Nouvelle dictée », « Quitter » | menu s'ouvre (fenêtre `#32768` du processus), **entrées non activées** |
| Clics dans l'interface | non testés par injection (bureau occupé) — IPC testé par CDP |
| Disque plein | non simulé |
| Capture audio, transcription | absents (M2, M3) |

## Journal

- 19/10/2026 : dossier créé (BOCNOT), contrat recopié, renommé Écho.
- 19/10/2026 19:04 : squelette lancé, fenêtres / tray / raccourci mesurés, IPC vérifié.
- 19/10/2026 19:30 : M1 livrée — journal, atomicité, BOM, 16 tests, test de 100 entrées
  avec 17 puis 21 arrêts brutaux : 100/100, 0 doublon.

## Prochaine étape (M2, non commencée)

Module capture `cpal` : micro, WAV 16 kHz mono par conversion (pas imposé au périphérique),
VU-mètre `capture:level`, pause/reprise. Aucune transcription avant que l'enregistrement
soit prouvé propre.
