# Écho V3 — une application issue des deux propositions

Demande du21octobre2026 : réunir le meilleur de V2 et V2.1, et intégrer des outils utiles aux personnes ayant des difficultés à utiliser souris/clavier. Les deux prototypes sont conservés ; seule V3 sera désormais développée et livrée.

Base technique : V2.1 pour notes Markdown, sauvegardes atomiques avec révisions/conflits, capture annotable et pipeline vocal plus précis selon l’essai utilisateur. Contributions V2 déjà portées : cibles externes UI Automation, aperçu borné pendant capture, gestion des processus, modes de dictée, dock modulaire, palettes et moteur local partagé. Le choix est une consolidation de fonctions concrètes, pas la combinaison de deux moteurs simultanés.

V3 : identité native distincte com.echo.v3, configuration/données propres sous D:\Écho V3, aucune migration automatique ni écriture dans les notes des anciennes versions. Socle196×64 et colonne optionnelle vers le haut. Modules Direct, Notes, Commandes, Traduire. Dictionnaire personnel et corrections locales volontaires.

Accessibilité : mains libres sans maintien, longue pause réglable pour laisser le temps de parler, indicateur micro/texte et boutons nommés accessibles au clavier, commandes44px et focus contrasté. Destination Note ou champ externe normal choisi avant la prise. Vérifier le fonctionnement réel et les refus de champ changé/mot de passe ; ne pas promettre toute application ou toutes les formes de parole.

Traducteur : widget ajoutable dans+, six langues, texte saisi ou zone écran sélectionnée. Suivi activé explicitement, arrêté au retrait/masquage/quitter et désactivé au redémarrage. Exécution locale ; aucun service Google ou clé externe configuré. OCR et traduction embarqués ; signaler le repli OCR fr-FR actuel.

Recherche OSS : Vosk et sherpa-onnx pour véritable streaming/commandes à évaluer ; RNNoise pour réduction de bruit à évaluer ; Argos/CTranslate2 retenu pour traduction ; Pot comme référence d’interaction OCR, pas comme code copié. Ne pas ajouter plusieurs moteurs résidents ni prétendre qu’un filtre répare une voix inaudible à plusieurs mètres. Aucun entraînement de voix atypique et aucune validation auprès d’utilisateurs handicapés réalisés pour l’instant.

Validation : tests frontend/natifs V3 propres, build natif, vraie fenêtre196×64, notes/conflits, cible externe contrôlée, widget traduction texte/zone/suivi/Stop et persistance. Résultats V2/V21 conservés comme historique, jamais présentés comme essais de l’exécutable V3. Livrer un EXE et un installateur V3 sous D, puis sauvegarde vérifiée.

Agent /root — Consolidation Écho V3 et accessibilité.
