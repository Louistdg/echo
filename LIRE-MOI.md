# ÉCHO V3 — 3.0.1

Ouvre **Écho V3.exe**. Le petit socle donne accès au micro, à la capture, au bloc-notes, aux autres outils et aux réglages.

## Dicter là où tu écris

Place le curseur dans le champ où tu veux écrire (une conversation, un bloc-notes, un document), puis clique sur le micro du petit socle. Parle ; un deuxième clic arrête la dictée. Tu n’as pas à choisir « Note » ou « Application ». Les phrases finales arrivent après les pauses ; le texte provisoire peut différer du résultat final.

## Une touche pour démarrer et arrêter

Dans **Réglages → Raccourcis**, tu peux choisir une touche seule : **1**, une lettre ou **F8**, par exemple. Un appui démarre la dictée, un autre l’arrête, sans maintenir la touche. Cette touche est réservée à Écho pendant son exécution. Tu peux aussi conserver une combinaison et utiliser le bouton **Un appui démarre / un autre arrête**, puis **Enregistrer**.

## Retrouver le petit socle

Dans les réglages ou en bas de la grande fenêtre des notes, clique **Afficher le petit socle**. Dans **Réglages → Dossier et démarrage**, **Replacer en bas à droite** réinitialise sa position. Relancer **Écho V3.exe** fait aussi réapparaître le socle si l’application tourne déjà.

Français et anglais sont prioritaires ; allemand et néerlandais sont aussi proposés dans **Réglages → Voix et moteur**. La dictée utilise un moteur local unique. Les noms propres peuvent nécessiter le vocabulaire personnel. La précision dépend de la voix et du bruit.

## Bloc-notes et traduction

Dans la grande fenêtre ou le bloc-notes compact, clique **Traduire**, choisis **De / Vers**, puis **Traduire la note**. Le français vers l'anglais est proposé par défaut. La traduction s'affiche sous la note, sans la remplacer ; le bouton **Copier la traduction** permet de la réutiliser. Arrête la dictée avant de traduire. Français, anglais, allemand et néerlandais sont disponibles.

La traduction peut reformuler des nombres ou des heures : relis ces détails. Les autres directions peuvent passer par l'anglais. Limite : 10 000 caractères par traduction. Une note changée rend son ancien résultat de traduction invisible.

## Dimensions et outils

**Réglages → Disposition des blocs → Appliquer les dimensions** règle la colonne et ses blocs. Préparer les tailles ne déplace pas le socle. Le + conserve l'accès aux outils ; les notes, captures, correction, traduction de zones et thèmes restent disponibles. Le tutoriel est dans les réglages.

## Installation et conservation

Le fonctionnement est local, sans API payante ni abonnement de transcription. Garde **moteur** et **echo-v3.portable** à côté du programme. L'installateur 3.0.1 est dans **installateur** ; il n'est pas exécuté automatiquement. Python et les modèles de dictée utilisent les dépendances déjà installées sur ce PC : une copie vers un autre ordinateur doit aussi prévoir ces dépendances.

Les notes et préférences existantes sont conservées. Les anciennes versions et les états avant la perte restent distincts.

Rapport de reprise : **docs/RAPPORT-REPRISE-2026-10-22.md**. Preuves de livraison : **docs/LIVRAISON-3.0.1-2026-10-22.json**. Sauvegarde : **docs/REPRISE-ACTUELLE.json**.


### Prises conservées
Si une ancienne dictée était bloquée, le prochain démarrage la conserve dans le dossier configuration\voice-jobs\prises-conservees, avec son texte lisible et les chemins des fichiers audio. Elle n’est pas envoyée au nouveau champ.
