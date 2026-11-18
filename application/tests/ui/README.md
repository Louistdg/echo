# Vérification du panneau unique

Après `npm ci` et `npm run build`, exécuter `python tests/ui/unified.py` avec Python, Playwright et Chrome installés. Le navigateur fonctionne sans fenêtre visible et se ferme à la fin. Les résultats et la capture vont dans `test-results/` (ou le chemin `ECHO_TEST_OUTPUT`).

Les commandes Tauri sont **simulées** : ces tests vérifient notamment le choix de destination, l’insertion dans l’application sans modifier la note, la conservation du texte si la cible est fermée, le parcours dictée vers la note, la sauvegarde, le panneau unique, les thèmes, les fichiers à glisser et les six dossiers récents. Ils ne valident pas un vrai microphone ni l’injection de touches dans une application Windows. Les tailles et contrôles de l’interface sont rendus dans un vrai navigateur.

Le test Rust ignoré `transcription_locale_fixture_reelle` utilise le vrai moteur si `ECHO_TEST_BINARY`, `ECHO_TEST_MODEL` et `ECHO_TEST_WAV` sont renseignés. Le WAV doit contenir une phrase avec « bonjour ». Lancer alors `cargo test --lib -- --include-ignored`. La fixture de cette session est une synthèse vocale ; aucune preuve de dictée humaine n’en est déduite.
