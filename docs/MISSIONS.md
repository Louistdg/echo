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

## M3 — Transcription (non commencée)

**Objectif.** Transformer un WAV 16 kHz mono en texte, hors ligne, sans bloquer
l'interface et sans jamais perdre l'audio en cas de panne du moteur.

**À construire.**

- VAD côté capture (Silero ou équivalent) pour couper les silences et arrêter
  l'enregistrement après N secondes de silence.
- `whisper-server` (whisper.cpp) lancé dans un **processus isolé**, écoute sur
  `127.0.0.1` avec port aléatoire, une requête à la fois, file d'attente
  sérialisée par le cœur Rust.
- Modèle `small` multilingue, langue `fr` par défaut, téléchargement unique avec
  vérification SHA-256.
- En cas de plantage du moteur : redémarrage automatique, message explicite,
  aucun WAV supprimé avant transcription réussie.
- `transcription:progress`, `transcription:done`, `transcription:error` vers l'UI.

**Hors périmètre M3.** Nettoyage IA, dictée vers d'autres applications, cloud.

**Critères d'acceptation pressentis.**

- Un WAV de 10 s de parole française produit un texte non vide en moins de 5 s
  sur cette machine, sans requête réseau hors téléchargement du modèle.
- Tuer `whisper-server` pendant une transcription : l'app le relance et le WAV
  est retranscrit, pas supprimé.
- Aucune donnée audio ne quitte la machine (vérifié par capture réseau).


