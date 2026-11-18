# Écho — squelette initial (Tauri 2 + SvelteKit + Rust)

Application de bureau de capture vocale locale : dictée hors ligne (whisper.cpp à venir), bloc-notes, notes Markdown dans un dossier choisi.

Généré à partir du gabarit officiel `create-tauri-app` (template SvelteKit + TypeScript), puis structuré selon le [plan d'architecture](docs/architecture.md) (copie incluse dans ce dossier `docs/`).

## Prérequis

- **Node.js 20+** et npm
- **Rust** (rustup) : https://rustup.rs
- **Windows** : « VS C++ Build Tools » + WebView2 — voir les prérequis officiels : https://tauri.app/start/prerequisites/

## Démarrage

```bash
npm install        # dépendances JavaScript
npm run check      # vérification Svelte + TypeScript
npm run build      # build frontend (dist dans build/)
npm run tauri dev  # développement (compile le Rust puis ouvre l'app)
npm run tauri build # build de l'installateur
```

> Vérifié sur la machine de génération : `npm install`, `npm run check` (0 erreur), `npm run build`, et `cargo check` (type-check Rust complet, 0 erreur 0 avertissement — via des stubs pkg-config car les libs GTK/WebKit Linux réelles étaient indisponibles ; c'est une vérification des types, pas un lancement validé).
>
> Non vérifié : le lancement réel de l'app (`npm run tauri dev`) — nécessite un environnement graphique complet. À exécuter sur ta machine Windows avec les prérequis ci-dessus.

## Structure

```
echo/
├── src/                          # UI SvelteKit (SPA, adapter-static)
│   └── routes/
│       ├── +layout.svelte        # achemine la fenêtre « quick » vers /quick
│       ├── +page.svelte          # fenêtre principale (bloc-notes)
│       └── quick/+page.svelte     # fenêtre flottante de dictée
└── src-tauri/                     # cœur Rust
    ├── tauri.conf.json            # 2 fenêtres : main + quick (invisible)
    ├── capabilities/              # permissions minimales par fenêtre
    │   ├── main.json
    │   └── quick.json
    └── src/
        ├── lib.rs                # point d'entrée (plugins + commandes)
        ├── tray.rs                # icône de notification + menu
        ├── shortcuts.rs           # Ctrl+Alt+Espace → fenêtre dictée
        ├── commands/mod.rs       # commandes IPC (get_status, settings, notes)
        ├── notes/mod.rs          # note du jour + écriture atomique
        ├── capture/mod.rs        # M2 — micro cpal, WAV 16 kHz mono, VU-mètre
        ├── transcription/mod.rs  # SQUELETTE — trait TranscriptionEngine
        └── watcher/mod.rs        # SQUELETTE — TODO v2 (notify)
```

## Ce qui fonctionne déjà

- Deux fenêtres déclarées : principale + flottante de dictée (sans décorations, toujours au-dessus, cachée par défaut).
- Tray avec menu (Ouvrir / Nouvelle dictée / Quitter).
- Raccourci global `Ctrl+Alt+Espace` : affiche/masque la fenêtre de dictée.
- Choix du dossier de notes (dialogue natif), persisté dans `settings.json` (répertoire applicatif).
- Ajout d'une entrée horodatée à la note du jour `AAAA-MM-JJ.md` (création + écriture atomique).
- **Journal de récupération** append-only avec `fsync` avant écriture, rejeu idempotent au démarrage (M1).
- **Capture audio** `cpal` : WAV 16 kHz mono par conversion logicielle, VU-mètre `capture:level`, pause/reprise, audio rangé dans `<notes>/.echo/audio/` (M2/M2.1).
- **Panneau flottant** déplaçable, épinglable, trois thèmes, historique cliquable (M2.1).
- **Transcription** : moteur remplaçable (`WhisperCli`), commande `transcribe_and_append`, bouton « → note », moteur réel `whisperfile` + modèle `ggml-tiny.bin` (M3).
- **Nettoyage local** du texte : hésitations retirées, majuscules, espaces — sans IA (M3).
- UI française honnête : les modules non implémentés sont signalés (`get_status`), aucun faux succès.

## Prochaines étapes (v1)

1. Modèle `small` multilingue (~466 Mo) au lieu de `tiny`.
2. `whisper-server` (whisper.cpp) résident en processus séparé + file d'attente.
3. Arrêt automatique au silence (VAD Silero côté moteur).
4. Dictée vers d'autres applications (trois stratégies du plan §4.3).

Voir le plan d'architecture complet pour les 38 fonctionnalités et la roadmap v1 → v5.

## Écart connu avec le plan d'architecture (§8.1)

Le code actuel s'écarte du plan sur la structure du frontend et les dépendances
audio. C'est assumé pour l'instant, mais **à ne pas oublier** :

| Prévu au plan §8.1 | Présent aujourd'hui | Phase de rattrapage |
|---|---|---|
| `src/windows/{main,quick,settings,palette}` | `src/routes/{+page, quick/+page}` (SvelteKit SPA) | M3/M4, si les fenêtres se multiplient |
| `src/stores/` (notes, session, settings) | état local dans les pages Svelte | M2/M3 |
| `src/lib/ipc.ts` (typage des commandes) | `invoke<T>(...)` inline dans les pages | M2 |
| Fenêtre Paramètres | aucun écran de paramètres | M2 |
| Palette de recherche `Ctrl+Alt+F` | absente | v2 du plan |
| `cpal`, `hound`, `notify`, `rusqlite` | absents de `Cargo.toml` | M2 (cpal, hound), v2 (notify, rusqlite) |

Ne pas réorganiser ces fichiers « pour être conforme au plan » avant que le besoin
apparaisse : le plan décrit une cible, pas un ordre de construction.

