# Tests manuels (harnais externes)

Ces scripts pilotent l'application réelle pendant qu'elle tourne. Ils ne font pas
partie du produit : ce sont les instruments de mesure de la mission M1
(écriture indestructible).

## Prérequis

1. Un serveur Vite : `npm run dev` (port 1420).
2. `settings.json` dans `%APPDATA%\com.echo.notes\` pointant vers un dossier de test vide.
3. L'application lancée avec le débogage distant :

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
.\src-tauri\target\debug\echo.exe
```

## Scripts

| Script | Rôle |
|---|---|
| `kill-test.mjs` | 100 ajouts avec `taskkill` aléatoires ; vérifie 100 entrées, 0 doublon |
| `ui-message.mjs` | clique « Ajouter à la note du jour » dans la page et lit le message affiché |

Variables d'environnement (facultatives) :

- `ECHO_EXE` : chemin de `echo.exe`
- `ECHO_NOTES` : dossier de notes de test
- `ECHO_CDP` : port de débogage (défaut 9222)

Exemple :

```powershell
$env:ECHO_NOTES = "D:\tmp\echo-test"
node tests/manuel/kill-test.mjs
```

Le script `kill-test.mjs` ferme l'application par `taskkill /F` ; il ne touche à
aucun autre processus.
