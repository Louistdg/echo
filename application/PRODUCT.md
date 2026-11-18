# Écho v2.1

<!-- impeccable:product-schema 1 -->

## Platform

web

Interface Svelte intégrée à une application native Tauri 2 pour Windows.

## Users

Louis utilise son PC pour écrire des notes, dicter dans ses conversations et conserver des captures d’écran. La refonte est destinée à cet usage quotidien.

## Product Purpose

Rassembler notes Markdown, enregistrements et captures dans un dossier choisi, avec une barre vocale disponible pendant le travail.

## Capabilities and Constraints

Reconstruction explicitement demandée dans `D:\Écho\v2.1`. V1 conservée. Traitement vocal local avec faster-whisper. Raccourci à maintenir, bascule et mains libres explicites. Capture immédiatement modifiable et copiable. Sauvegarde automatique, destination visible, texte conservé en cas d’erreur. Huit thèmes conservés. L’utilisateur a délégué les choix de conception.

## Operating Context

Windows, clavier et souris, fenêtres d’autres applications. Les données existantes restent dans `D:\Écho\notes`. Le nouveau programme dispose de réglages et d’un dossier de données séparés ; un autre dossier peut être choisi.

## Product Principles

- Montrer où chaque contenu est enregistré.
- Confirmer une sauvegarde seulement après écriture réelle.
- Garder le travail récupérable après une erreur.
- Montrer l’état du micro et permettre son arrêt immédiat.
- Ne jamais envoyer une conversation en simulant Entrée.
