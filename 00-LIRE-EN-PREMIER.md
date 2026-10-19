# Écho — dossier de travail

Application de bureau de capture vocale locale (Tauri 2 + SvelteKit + Rust).
Créé le 19/10/2026 sous le nom BOCNOT, renommé **Écho** le même jour.
Chemin : `D:\DSH\default-workspace\Écho`.

**Dépôt public : <https://github.com/Louistdg/echo>** (branche `main`).
Les missions (M1 terminée, M2 à venir) sont décrites dans `docs/MISSIONS.md`.

## Fichiers

| Fichier | Rôle |
|---|---|
| `00-LIRE-EN-PREMIER.md` | ce fichier : état vérifié, commandes, limites, suite |
| `01-PROMPT-A-COLLER-SOCLE.txt` | contrat de travail (méthode), recopié octet pour octet — voir la réserve plus bas |
| `README.md` | README du squelette + écart structurel avec le plan |
| `docs/architecture.md` | plan d'architecture v1.0 |
| `docs/MISSIONS.md` | M1 (livrée, avec preuves) et M2 (capture audio) |
| `tests/manuel/` | harnais de test M1 (kill-test, ui-message) |
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
| Clics dans l'interface | non testés par injection (bureau occupé) — IPC et bouton testés par CDP |
| Disque plein | non simulé |
| Capture audio | vérifiée (M2) : WAV 16 kHz mono, niveaux, pause, purge |
| Transcription | absente (M3) |

## Journal

- 19/10/2026 : dossier créé (BOCNOT), contrat recopié, renommé Écho.
- 19/10/2026 19:04 : squelette lancé, fenêtres / tray / raccourci mesurés, IPC vérifié.
- 19/10/2026 19:30 : M1 livrée — journal, atomicité, BOM, 16 tests, test de 100 entrées
  avec 17 puis 21 arrêts brutaux : 100/100, 0 doublon.
- 19/10/2026 21:15 : M2 livrée — capture `cpal`, WAV 16 kHz mono, 22 tests, 10 s mesurées,
  pause 4,05 s sur 6 s, purge après `taskkill`, tracé RMS fourni.
- 19/10/2026 22:05 : M1.1 — corrections après revue externe : note illisible refusée,
  conflits externes préservés, identifiant d'opération stable, dossier journalisé,
  journal abîmé signalé, erreurs WAV remontées, anti-repliement 7,2 kHz. 28 tests.
- 19/10/2026 23:10 : M2.1 — audio rangé dans `<notes>/.echo/audio/` (les WAV finalisés
  ne sont plus purgés), historique cliquable, boutons « Ouvrir le dossier », panneau
  flottant déplaçable / épinglable / trois thèmes.

## M2 — Capture audio (livrée le 19/10/2026)

Détail complet et preuves : `docs/MISSIONS.md`. Résumé mesuré :

- WAV réel de 10 s : 160 425 échantillons, 320 894 octets = 44 + 320 850, en-tête
  RIFF/WAVE, 1 canal, 16 000 Hz, 16 bits.
- `capture:level` : 247 événements sur 10 s, aucun après l'arrêt.
- Pause 2 s au milieu : 4,05 s enregistrées pour 6 s écoulées.
- `taskkill` en cours : WAV laissé, purgé au redémarrage suivant.
- RMS : silence 0,00078 ; ton 1 kHz 0,00295. Tracé `docs/captures/rms-silence-vs-ton.png`.

Limite à ne pas oublier : aucun locuteur n'était disponible, le signal de test est un ton
joué par les haut-parleurs ; le couplage acoustique est faible.

## M1.1 — Ce que la revue a fait changer

Cinq défauts de persistance reproduits puis corrigés (détail et preuves :
`docs/MISSIONS.md`) :

1. Une note illisible était lue comme vide puis écrasée → refus explicite, fichier intact.
2. Une modification externe était écrasée → fichier `.conflit-*.md` et erreur.
3. Un réessai créait un doublon → identifiant d'opération stable côté UI.
4. Le journal ne retenait pas le dossier de destination → champ `dir`, rejeu au bon endroit.
5. Le journal était réparé en silence → lecture honnête, fin tronquée réparée avec
   sauvegarde, compaction refusée si ligne illisible, résumé dans `get_status`.

Côté capture : erreurs d'écriture WAV remontées, session libérée quand le fil meurt,
filtre anti-repliement 4ᵉ ordre à 7,2 kHz avant décimation.

Non corrigé, assumé : pas de verrou d'instance unique (deux Écho peuvent encore se
concurrencer), travail lourd encore dans le callback audio, purge des temporaires à
revoir avec M3, sélection du micro absente.

## Prochaine étape (M3, non commencée)

VAD côté capture, puis `whisper-server` dans un processus isolé avec le modèle `small`
multilingue. Détail : `docs/MISSIONS.md`.

