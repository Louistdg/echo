# Missions d'Écho

Ce fichier est la référence pour tout agent qui reprend le projet : ce qui est
fait, ce qui est prouvé, et ce qui est demandé ensuite.

## M1 — Écriture indestructible (terminée le 19/10/2026)

**Objectif mesurable.** Tuer le processus à n'importe quel moment pendant une
écriture ne perd jamais une entrée confirmée à l'écran, et n'en duplique jamais.

**Livré.**

- `src-tauri/src/recovery.rs` : journal `recovery.log` append-only, une ligne JSON
  par entrée (`id`, `note`, `time`, `text`, `state`), `sync_data()` après chaque
  ligne. `pending()` ignore les entrées ayant un `done`. Compaction : 30 jours.
- `src-tauri/src/atomic.rs` : écriture atomique (temporaire sur le même volume,
  `sync_all`, puis `ReplaceFileW`).
- `src-tauri/src/notes/mod.rs` : marqueur `<!--echo:ID-->` dans la note, rejeu
  idempotent, recréation d'une note supprimée.
- `src-tauri/src/commands/mod.rs` : ordre journal → note → `done`, rejeu au
  démarrage (`replay_pending`), `settings.json` tolérant un BOM UTF-8.
- `src/routes/+page.svelte` : « Enregistré » seulement après journal + note ;
  « NON ENREGISTRÉ » sinon, texte conservé.

**Preuves.**

- `cargo test` : 16/16.
- Preuve négative : contrôle de marqueur désactivé → le test d'idempotence échoue
  (3 occurrences au lieu de 1), restauré → 16/16.
- Test décisif (100 ajouts, 17 puis 21 `taskkill` aléatoires) : 100/100 entrées,
  0 doublon, 100 identifiants uniques. Harnais : `tests/manuel/kill-test.mjs`.
- Interface : messages « Enregistré » et « NON ENREGISTRÉ » vérifiés dans l'application.

**Limites connues.**

- Les trois entrées du menu du tray ne sont pas activées automatiquement (menu
  Win32 non exposé à UI Automation) : ouverture vérifiée, actions non vérifiées.
- Disque plein non simulé.
- Le journal écrit deux lignes par entrée (`pending` + `done`) ; compaction à 30 jours.

## M2 — Capture audio (terminée le 19/10/2026)

**Objectif.** Prouver qu'on enregistre proprement, avant toute transcription.

**Livré.**

- `src-tauri/src/capture/mod.rs` : session micro via `cpal`, ouverte dans le
  format natif du périphérique (ici 2 canaux, 48 kHz, F32).
- Conversion logicielle vers WAV **16 kHz mono** : moyenne des canaux puis
  interpolation linéaire, position fractionnaire conservée d'un tampon à l'autre
  (pas de dérive cumulée).
- Événement `capture:level` (RMS) émis au fil de l'eau, ~25–30 Hz.
- Pause / reprise ; la durée finale ne compte que les segments actifs.
- WAV temporaire dans `%TEMP%\Écho\`, purgé au démarrage suivant.
- Erreurs explicites : aucun micro, configuration indisponible, flux perdu.
  Les dépassements de tampon (Xrun) sont tolérés et comptés, pas journalisés en
  boucle : ce pilote en signale des centaines par minute.
- Fenêtre « quick » : bouton Enregistrer / Pause / Arrêter, chrono, VU-mètre en
  décibels (−70 dB → 0 %, −20 dB → 100 %), chemin affiché après l'arrêt.

**Preuves.**

| Mesure | Valeur |
|---|---|
| `cargo test` | 22/22 |
| WAV 10,000 s (test unitaire) | 160 000 échantillons, 320 000 octets de données, en-tête validé champ par champ |
| WAV 10 s (réel) | 10,0266 s, 160 425 échantillons, 320 894 octets = 44 + 320 850 |
| En-tête réel | RIFF/WAVE, 1 canal, 16 000 Hz, 16 bits, `data` cohérent |
| `capture:level` | 247 événements sur 10 s, 0 après l'arrêt |
| Pause | 2 s + pause 2 s + 2 s → 4,05 s enregistrées |
| `taskkill` en cours | 1 WAV laissé, 0 après redémarrage (purge) |
| RMS silence | max 0,00078 |
| RMS ton 1 kHz | max 0,00295 (rapport 3,8×) ; essai antérieur 0,0017 → 0,0081 (10×) |

Tracé : `docs/captures/rms-silence-vs-ton.png` (données brutes :
`docs/captures/rms-trace.json`). Captures de la fenêtre :
`docs/captures/quick-enregistrement.png`, `docs/captures/quick-apres-arret.png`.

**Limites assumées.**

- Aucun locuteur disponible pendant la mission : le signal de test est un ton de
  1 kHz joué par les haut-parleurs et capté par le micro. Le couplage acoustique
  est faible (RMS 0,003), ce qui ne prouve pas un gain nominal sur une vraie voix.
- « Micro coupé » non testé : il faudrait désactiver le périphérique dans Windows.
- Les Xrun sont fréquents sur cette machine (pilote « High Definition Audio ») ;
  ils n'empêchent pas l'enregistrement mais expliquent des niveaux irréguliers.

**Hors périmètre M2.** Aucune transcription, aucun modèle Whisper, aucune VAD.

**Critères d'acceptation.**

- [x] Un WAV de 10 s fait exactement 320 000 octets de données (16 kHz × 2 octets × 10).
- [x] En-tête WAV relu et validé (RIFF/WAVE, fmt PCM, canaux 1, taux 16 000).
- [x] `capture:level` arrive pendant l'enregistrement et cesse après l'arrêt.
- [x] Pause puis reprise : la durée finale = somme des deux segments.
- [x] Aucun fichier audio ne subsiste dans `%TEMP%` après un arrêt brutal et redémarrage.

## M2.1 — Rangement de l'audio et panneau flottant (19/10/2026)

- L'audio est rangé dans `<dossier de notes>/.echo/audio/`, écrit en `.part` puis
  renommé à l'arrêt. Les `.part` (enregistrements interrompus) sont purgés au
  démarrage ; les WAV finalisés ne sont jamais supprimés.
- Commandes : `list_recordings` (20 plus récents), `open_recordings_folder`,
  `open_notes_folder`, `open_recording` (le chemin est validé : il doit être sous
  le dossier d'enregistrements).
- Panneau flottant : déplaçable (barre de glissement), épinglable (bouton 📌),
  trois thèmes persistés (`nuit`, `rose`, `clair`), micro + VU-mètre en décibels.
- Interface principale : boutons « Ouvrir le dossier des notes / des
  enregistrements », historique cliquable avec nom, durée et date.
- Vérifié : 10 s → `D:\test1.écho\.echo\audio\20261019-230720.wav` (321 218 octets,
  44 + 2×160 587), historique d'une entrée, `.part` purgé au redémarrage, WAV
  conservé. DOM du panneau : zone de glissement, 3 pastilles, bouton épingler.

## M3 — Transcription (livrée le 19/10/2026, moteur réel `tiny`)

**Objectif.** Transformer un WAV 16 kHz mono en texte, hors ligne, sans bloquer
l'interface et sans jamais perdre l'audio en cas de panne du moteur.

**Livré (pipeline) — 19/10/2026.**

- Trait `TranscriptionEngine` (moteur remplaçable) + implémentation `WhisperCli` :
  lance whisper.cpp en processus isolé (`-m <modèle> -f <wav> -otxt -l fr -nt`),
  lit le `.txt`, nettoie. Erreurs explicites : binaire absent, modèle absent,
  audio absent.
- Commande `transcribe_and_append` : valide que le fichier est un enregistrement
  d'Écho, transcrit, puis ajoute le texte à la note du jour par le chemin sûr
  (journal → note → done) avec un identifiant dérivé du nom de fichier.
  Retranscrire ne duplique pas.
- Réglages `whisper_bin` / `whisper_model` dans `settings.json` ; bouton « → note »
  sur chaque enregistrement de l'historique.
- `clean_text` : nettoyage local (hésitations, majuscules, espaces), sans IA.

**Livré (moteur réel) — 19/10/2026, 23:30.**

- Moteur : `whisperfile` (llamafile 0.10.6, exécutable portable, 5,5 Mo) et
  modèle `ggml-tiny.bin` (77,7 Mo), posés dans `tools/whisper/` (hors dépôt).
- Vérification réelle : un WAV de parole française synthétisée a été transcrit —
  « bonjour, ceci est un test de transcription vocale. Cette application
  enregistre ma voix… » — puis écrit dans la note avec le marqueur
  `trans-test-fr`, une seule occurrence après rejeu. 32 tests au total.

**Reste à faire (qualité et robustesse).**

- Passer au modèle `small` multilingue (~466 Mo) : le `tiny` fait des fautes
  (« puisse écrire » au lieu de « puis écrit »).
- `whisper-server` résident + file d'attente + redémarrage automatique (remplace
  le CLI un-coup) : le plan §2.3 les prévoit pour l'annulation et la progression.
- VAD côté capture (arrêt au silence).
- Critères restants : un WAV de 10 s de **voix humaine réelle** → texte < 5 s ;
  tuer le moteur → relance et WAV retranscrit ; aucune donnée ne quitte la machine.

**Hors périmètre M3.** Nettoyage IA, dictée vers d'autres applications, cloud.

**Critères d'acceptation pressentis.**

- Un WAV de 10 s de parole française produit un texte non vide en moins de 5 s
  sur cette machine, sans requête réseau hors téléchargement du modèle.
- Tuer `whisper-server` pendant une transcription : l'app le relance et le WAV
  est retranscrit, pas supprimé.
- Aucune donnée audio ne quitte la machine (vérifié par capture réseau).

## M1.1 — Corrections après revue (19/10/2026)

Revue externe (Codex) sur la copie locale, puis reproduction et correction.

| Constat de la revue | État | Correctif et preuve |
|---|---|---|
| Une note illisible est lue comme vide puis écrasée | **corrigé** | `read_note` propage l'erreur (absente = vide, illisible = erreur). Test `une_note_illisible_ne_doit_pas_etre_ecrasee` : échouait avant (fichier remplacé), passe. Vérifié en réel : refus explicite, octets du fichier identiques. |
| Modifications externes écrasées silencieusement | **partiellement corrigé** | Relecture avant remplacement ; si le contenu a changé, la version proposée part dans `note.conflit-AAAAMMJJ-HHMMSS.md` et l'appel échoue. Test `une_modification_externe_part_dans_un_fichier_de_conflit`. **Deux instances simultanées restent possibles** : pas de verrou d'instance unique. |
| Un réessai après échec crée un doublon | **corrigé** | Identifiant d'opération fourni par l'UI (`entryId`), conservé tant que le texte est le même, libéré après succès. Vérifié en réel : deux appels avec `B-fixe` → une seule entrée. |
| Le journal ne retient pas le dossier de destination | **corrigé** | Champ `dir` dans chaque entrée ; le rejeu écrit dans ce dossier. Test `le_rejeu_respecte_le_dossier_journalise_pas_le_dossier_courant`. |
| Lecture du journal trop silencieuse | **corrigé** | `ReadReport` distingue absent / lisible / abîmé ; fin tronquée détectée puis réparée avec sauvegarde `recovery.corrompu-*.log` ; compaction refusée et journal sauvegardé si une ligne est illisible ; résumé affiché dans `get_status`. Tests : fin tronquée réparée, compaction refusée, ligne illisible comptée. |
| Erreurs d'écriture WAV avalées | **corrigé** | Une erreur `write_sample` est mémorisée et `stop_capture` échoue explicitement au lieu de rendre un WAV tronqué. |
| Session de capture « en cours » après une erreur du périphérique | **corrigé** | `is_running` détecte un fil terminé et libère la session. |
| Rééchantillonnage sans anti-repliement | **corrigé** | Filtre biquad 4ᵉ ordre à 7,2 kHz avant décimation. Test `un_ton_aigu_est_attenue_avant_la_decimation` : un 12 kHz tombe sous 20 % du niveau d'un 1 kHz. |
| Travail lourd dans le callback audio | **non corrigé** | À traiter en M2.1 (file bornée + débordement signalé). |
| Purge au démarrage incompatible avec une transcription en attente | **non corrigé** | À traiter en M3 : ne purger que l'audio dont le texte est durablement enregistré. |
| Sélection du micro absente | **non corrigé** | Une seule entrée existe sur la machine ; à faire avec l'interface de réglages. |
| Preuve de durabilité après coupure électrique | **non établie** | `taskkill` tue un processus, pas l'alimentation. Le `fsync` est là, la preuve de bout en bout ne l'est pas. |
| Le journal n'est pas une sauvegarde des notes effacées | **assumé** | Une entrée `done` n'est pas rejouée si la note disparaît : c'est le comportement voulu (respect d'une suppression volontaire), à documenter dans l'UI. |

Tests : **28** au total après M1.1 (`cargo test`), `npm run check` à 0 erreur.



