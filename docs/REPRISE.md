# Écho — dossier de reprise

> Écrit le 20 octobre 2026, 01:00. Tout ce qui suit a été mesuré dans cette
> session ; ce qui n'est pas vérifié est signalé comme tel.

## En une phrase

Application de bureau Windows de notes vocales **hors ligne** : on appuie sur un
micro, on parle, le texte transcrit arrive dans la note Markdown du jour, dans un
dossier choisi. Rien ne quitte la machine.

Dépôt public : <https://github.com/Louistdg/echo> — branche `main`, commit `12196d3`.
Notion : base « Projets », page « Écho » (mise à jour le 20/10).

## Où en est le projet

| Mission | État | Contenu |
|---|---|---|
| **M1** — écriture indestructible | livrée | journal append-only + `fsync`, écriture atomique (`ReplaceFileW`), rejeu idempotent, correctif BOM |
| **M1.1** — corrections après revue | livrée | note illisible refusée, conflits externes préservés (`.conflit-*.md`), réessai sans doublon (`entryId`), dossier journalisé, journal abîmé signalé |
| **M2** — capture audio | livrée | `cpal` en format natif, WAV 16 kHz mono, filtre anti-repliement, `capture:level`, pause/reprise |
| **M2.1** — rangement + panneau | livrée | audio dans `<notes>/.echo/audio/`, historique cliquable, boutons « Ouvrir le dossier », panneau déplaçable/épinglé/3 thèmes |
| **M3** — transcription | livrée (modèle `tiny`) | moteur remplaçable `WhisperCli`, `transcribe_and_append`, bouton « → note », moteur réel `whisperfile` |
| **Note améliorée** | livrée (nettoyage local) | `clean_text` : hésitations retirées, majuscules, espaces — aucune reformulation IA |

Tests : **32** (`cargo test`), `npm run check` : 0 erreur.

## Ce qui marche aujourd'hui (vérifié dans cette session)

- **Enregistrer** depuis le panneau flottant → WAV 16 kHz mono dans
  `D:\test1.écho\.echo\audio\` (fichier `.part` renommé à l'arrêt ; `.part` purgé
  au démarrage, WAV finalisés jamais supprimés).
- **Transcrire réellement** : `whisperfile` + `ggml-tiny.bin` ont transcrit une
  voix française synthétisée — « bonjour, ceci est un test de transcription
  vocale… » — et le texte a été écrit dans `D:\test1.écho\2026-10-19.md`.
- **Historique** : liste des enregistrements, durée, date, clic = ouverture,
  bouton « → note » = transcription + ajout.
- **Sauvegarde** : 100 ajouts avec arrêts brutaux → 100/100, 0 doublon (M1).
- **Panneau** : zone de glissement, 3 thèmes, bouton épingler, micro + VU-mètre.

## Ce qui est configuré sur ce PC (chemins)

| Élément | Chemin |
|---|---|
| Projet | `D:\DSH\default-workspace\Écho` |
| Réglages de l'app | `%APPDATA%\com.echo.notes\settings.json` (`notes_dir`, `whisper_bin`, `whisper_model`) |
| Dossier de notes de test | `D:\test1.écho` |
| Moteur de transcription | `D:\DSH\default-workspace\tools\whisper\whisperfile.exe` |
| Modèle | `D:\DSH\default-workspace\tools\whisper\ggml-tiny.bin` (77,7 Mo) |
| Logo mascotte (généré) | `D:\DSH\default-workspace\tools\logo\mascotte\` |
| Icônes installées (local, non poussées) | `Écho\src-tauri\icons\` |
| Archive de transfert | `D:\DSH\default-workspace\echo-dossier-transfert.zip` |
| Anciennes icônes (sauvegarde) | `D:\DSH\default-workspace\tools\logo\anciens-icons\` |

## Ce qui reste à faire

1. **Modèle `small` multilingue** (~466 Mo) au lieu de `tiny` : meilleure
   précision française. Changer `whisper_model` dans les réglages.
2. **`whisper-server` résident** + file d'attente + redémarrage automatique
   (remplace le CLI un-coup) — plan §2.3.
3. **VAD** : arrêt automatique au silence.
4. **Verrou d'instance unique** : deux Écho peuvent encore se disputer une note.
5. **Dictée vers d'autres applications** (3 stratégies du plan §4.3).
6. **Travail lourd dans le callback audio** : à sortir vers une file bornée.

## Points à trancher (demandés, pas encore décidés)

1. **Le « T » du bouclier** est la lettre de PastatShield, pas d'Écho. Remplacer
   par « É », retirer le bouclier, ou garder tel quel ?
2. **Publier le logo sur le dépôt public ?** Le logo (chien + capture
   d'écran PastatShield) est installé **en local seulement** : les fichiers
   `src-tauri/icons/*` sont modifiés mais **non commités**. Décision à prendre.

## Comment lancer / tester

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cd D:\DSH\default-workspace\Écho
npm run check                      # TypeScript + Svelte
cd src-tauri; cargo test           # 32 tests
cd ..
npm run tauri dev                  # lance l'app (ou le .exe déjà compilé)
```

Pour transcrire : ouvrir l'app, choisir le dossier de notes, enregistrer avec
`Ctrl+Alt+Espace`, puis bouton « → note » dans l'historique.

## Liens

- Dépôt : <https://github.com/Louistdg/echo>
- Missions et preuves : [docs/MISSIONS.md](MISSIONS.md)
- État détaillé : [../00-LIRE-EN-PREMIER.md](../00-LIRE-EN-PREMIER.md)
- Plan d'architecture : [architecture.md](architecture.md)
- Prompt de présentation : [PROMPT-ECHO.md](PROMPT-ECHO.md)
- Notion : base « Projets », page « Écho »

## Journal de la session

- 19/10 18:55 : squelette lancé, fenêtres / tray / raccourci mesurés.
- 19/10 19:30 : M1 livrée (journal, atomicité, BOM).
- 19/10 21:15 : M2 livrée (capture cpal, WAV 16 kHz).
- 19/10 22:05 : M1.1 (corrections revue externe).
- 19/10 23:10 : M2.1 (audio rangé, historique, panneau flottant).
- 19/10 23:30 : M3 (moteur réel `tiny`, nettoyage des hésitations).
- 20/10 00:30 : logo mascotte (chien + bouclier) installé en local.

## Ce qui n'est PAS vérifié

- Aucune **voix humaine réelle** n'a été testée : la transcription réelle a été
  mesurée sur de la synthèse vocale (TTS Windows), pas sur ta voix au micro.
- Le rendu du logo dans la fenêtre n'a pas été vu par un humain après
  recompilation (l'agent l'a vérifié par les fichiers et le DOM, pas à l'œil).
- Aucun test de coupure électrique ; `taskkill` tue un processus, pas le courant.
