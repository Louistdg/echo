# Moteur de transcription — faster-whisper (open source)

Écrit le 20 octobre 2026 après mesures sur la machine de Louis
(Intel Core i7-6700, 4 cœurs / 8 threads, Windows).

## Le problème

Le moteur précédent (`whisperfile.exe` = whisper.cpp via llamafile, modèle
`ggml-tiny` puis `ggml-base`) transcrivait une dictée de 9 s en **22 à 30 s**
(2,5 à 3,3 fois le temps réel) et le modèle `tiny` produisait du texte faux
(« C'est la meilleure » au lieu de « tu m'entends pas »).

## La solution retenue

**faster-whisper** (open source, MIT) — Whisper exécuté par **CTranslate2** en
int8 sur CPU, avec VAD (détection d'activité vocale) intégrée.

| Moteur | Audio | Temps | Texte obtenu |
|---|---|---|---|
| whisper.cpp `tiny` | 9,0 s | 9,1 s | « C'est la meilleure. » (faux) |
| whisper.cpp `base` | 9,0 s | 22,2 s | correct |
| **faster-whisper `small` int8** | 9,0 s | **3,7 s** | correct |
| **faster-whisper `small` int8** | 12,4 s | **7,3 s** | « Bonjour, ceci est un test de transcription vocale… » |
| **faster-whisper `small` int8** | 35,4 s | **3,7 s** (VAD coupe les silences) | « Allô, allô, allô, tu m'entends ?… » |

Dans l'app, le parcours complet (clic micro → 3 s de voix → clic stop → texte
dans le bloc-notes) prend maintenant **3,6 s**, contre 27 s avant.

## Installation

Déjà faite sur cette machine :

```powershell
python -m pip install faster-whisper
```

Le modèle `small` (CTranslate2, ~460 Mo quantifié int8) est téléchargé une
première fois puis mis en cache dans `%USERPROFILE%\.cache\huggingface`.
La transcription fonctionne ensuite **hors ligne** (le script force
`HF_HUB_OFFLINE=1`).

## Fichiers

| Fichier | Rôle |
|---|---|
| `tools\whisper\fw_transcribe.py` | script Python : lit le WAV, transcrit, écrit `<sortie>.txt` |
| `tools\whisper\fw-cli\` | petite passerelle Rust qui traduit les arguments whisper.cpp (`-f`, `-of`, `-l`, `-m`) vers le script |
| `tools\whisper\fw-cli.exe` | le binaire compilé (237 Ko), **c'est lui qu'on met comme `whisper_bin`** |
| `tools\whisper\modeles\small\` | marqueur de modèle : le nom du dossier choisit `tiny`, `base`, `small` ou `medium` |
| `D:\DSH\default-workspace\tools\whisper\fw-log.txt` | journal de diagnostic (démarrages, durées) |

## Réglages de l'app

`%APPDATA%\com.echo.notes\settings.json` :

```json
{
  "whisper_bin": "D:\\DSH\\default-workspace\\tools\\whisper\\fw-cli.exe",
  "whisper_model": "D:\\DSH\\default-workspace\\tools\\whisper\\modeles\\small",
  "notes_dir": "D:\\test1.écho"
}
```

Pour changer de modèle : créer/copier un dossier `modeles\base` (ou `tiny`,
`medium`) et pointer `whisper_model` dessus. Le script déduit le modèle du
chemin.

## Limites connues

- **2 à 3 s de chargement par dictée** : le modèle est rechargé à chaque appel
  (un processus Python par transcription). Un serveur résident supprimerait ce
  délai — c'est la prochaine étape.
- La qualité dépend du micro : une dictée dans le bruit donne « [aucune parole
  détectée] » ou du texte approximatif.
- Le premier lancement après installation télécharge le modèle (~460 Mo).
- Python et faster-whisper doivent être présents sur la machine.
