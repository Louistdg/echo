# v4 — nouveau moteur de transcription

> Branche `v4`. Remplace le moteur `whisper.cpp` de la v3 (`ggml-tiny`, décodage
> glouton, aucun garde-fou) par une configuration réellement utilisable en dictée
> française.

## Ce qui n'allait pas en v3

| Symptôme | Cause réelle |
|---|---|
| Phrases approximatives, mots avalés | modèle `ggml-tiny` — le plus petit de la gamme |
| Mots inventés sur les silences | pas de coupure de contexte (`-mc 0` absent) : whisper boucle et brode |
| « Sous-titres réalisés par la communauté d'Amara.org » dans les notes | aucune filtration des hallucinations de silence |
| Rien ne se passe à la première installation | binaire et modèle à configurer à la main, aucune découverte automatique |
| Fenêtre noire qui clignote | `Command::status()` sur un exe console depuis une app GUI |
| Transcriptions vides sans explication | l'erreur ne disait pas que le modèle était trop faible |

## Ce que fait la v4

1. **Découverte automatique** — `WhisperCli::discover()` cherche `whisper-cli.exe`
   et les `ggml-*.bin` à côté de l'exécutable (`/`, `models/`, `modeles/`,
   `vendor/`, `moteur/`, `moteur/vendor/`), puis retient **le meilleur modèle
   disponible** (large > medium > small > base > tiny). Les chemins des réglages
   restent prioritaires… de repli.
2. **Réglages de dictée par défaut** (`WhisperOptions`) :
   `-l fr`, `-bs 5` (faisceau), `-et 2.8` (entropie), `-mc 0` (contexte coupé),
   threads automatiques, lexique optionnel via `--prompt`.
3. **Filtre d'hallucinations** (`is_hallucination`) : les génériques de silence
   connus sont reconnus et rendus comme une transcription vide — que l'app traite
   déjà comme « rien entendu ». Une vraie dictée qui se termine par ces mots
   n'est pas jetée.
4. **Garde-fous audio** : en-tête WAV lu sans décoder (`wav_duration_seconds`) ;
   moins de 0,25 s de son = on ne réveille pas le modèle.
5. **Aucune fenêtre console** : `CREATE_NO_WINDOW`, sortie capturée
   (`Command::output()`), et les erreurs du moteur remontent en clair au lieu
   d'un simple code de sortie.
6. **Message utile quand le modèle plafonne** : `ModelQuality::quality_warning()`
   explique dans l'erreur qu'il faut passer en `small`.

## Modèle recommandé

| Modèle | Taille | Dictée française | Usage |
|---|---|---|---|
| `ggml-tiny` | 75 Mo | insuffisant | v3 |
| `ggml-base` | 142 Mo | limite | — |
| **`ggml-small`** | **466 Mo** | **bon** | **défaut v4** |
| `ggml-medium` | 1,5 Go | très bon | machine puissante |

À déposer à côté de l'exécutable :

```powershell
curl -L -o ggml-small.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```

## Interface publique

```rust
pub trait TranscriptionEngine { fn transcribe(&self, wav: &Path) -> Result<String, String>; }

pub struct WhisperCli { binary, model, options }
impl WhisperCli {
    pub fn new(binary, model) -> Self;                    // compat v3
    pub fn with_options(binary, model, options) -> Self;
    pub fn discover() -> Option<Self>;                    // nouveau
    pub fn check(&self) -> Result<(), String>;
    pub fn arguments(&self, wav, out_base) -> Vec<OsString>; // testable sans exécuter
    pub fn quality(&self) -> ModelQuality;
}
```

Le trait ne bouge pas : `commands::transcribe_and_append` reste le seul point
d'entrée, ce qui laisse la porte ouverte à un moteur résident
(`whisper-server`) ou à `sherpa-onnx` sans toucher au reste de l'app.

## Tests

`cargo test --lib` couvre : le trait remplaçable, le nettoyage des hésitations,
les erreurs explicites (binaire absent, audio absent), la lecture de la qualité
du modèle, la construction des arguments, la transmission du lexique, le filtre
d'hallucinations (y compris le cas d'une vraie phrase qui finit par un motif) et
la lecture de durée WAV sur un fichier synthétique.

## Reste à faire (v4.1)

- VAD réel (silero via `sherpa-onnx`) pour couper l'audio avant whisper : moins
  de segments vides, moins d'hallucinations, plus rapide.
- Mode `whisper-server` résident : le modèle `small` met ~1 s à charger à chaque
  dictée, ce que le mode serveur supprime.
- Réglage « modèle » exposé dans l'interface, avec téléchargement assisté.