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

## M2 — Capture audio (non commencée)

**Objectif.** Prouver qu'on enregistre proprement, avant toute transcription.

**À construire.**

- `src-tauri/src/capture/mod.rs` : session micro via `cpal`.
- Sortie WAV **16 kHz mono** par conversion logicielle (ne pas imposer ce format
  au périphérique : ouvrir dans le format natif du device, puis rééchantillonner).
- Événement `capture:level` (niveau RMS) à ~30 Hz pour le VU-mètre.
- Pause / reprise sans clôturer la session.
- Fichier temporaire dans `%TEMP%/Écho/`, supprimé après usage (sauf conservation
  explicite, désactivée par défaut).
- Micro refusé : message clair + lien vers les réglages Windows, jamais de boucle
  d'erreur.

**Hors périmètre M2.** Aucune transcription, aucun modèle Whisper.

**Critères d'acceptation.**

- Un WAV de 10 s fait exactement 320 000 octets de données (16 kHz × 2 octets × 10).
- En-tête WAV relu et validé (RIFF/WAVE, fmt PCM, canaux 1, taux 16 000).
- `capture:level` arrive pendant l'enregistrement et cesse après l'arrêt.
- Pause puis reprise : la durée finale = somme des deux segments.
- Aucun fichier audio ne subsiste dans `%TEMP%` après transcription simulée.
