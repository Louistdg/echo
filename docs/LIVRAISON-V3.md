# Écho V3 — livraison et vérifications
21 octobre 2026 · Windows x64 · version 3.0.0

## Ouvrir
Programme portable : D:\Écho V3\Écho V3.exe. Conserver le dossier moteur à côté et le marqueur echo-v3.portable. Installateur : installateur\Écho V3_3.0.0_x64-setup.exe. Le paquet est construit ; aucune installation sur C n’a été effectuée pendant cette mission. L’application est laissée fermée, sans port de diagnostic ouvert.

Cette V3 est une application distincte, issue de la base V2.1 et des fonctions V2 déjà rapprochées. Elle ne remplace pas les exécutables historiques. Les notes de test sont exclusivement sous docs\qa\donnees. Aucune migration automatique des notes anciennes n’est imposée.

## Ce qui est intégré
- Socle compact196×64, déplaçable et épinglable ; colonne vers le haut, blocs ajoutables/retirables, largeur280–640 et hauteurs120–640, ordre et réduction.
- Vrais blocs de notes, dictée directe, commandes et traduction. Notes enregistrées avec contrôle des conflits et sauvegarde de la dernière frappe avant retrait.
- Aperçu vocal provisoire pendant la prise, finalisation et transmission une seule fois ; modes maintenir, appuyer/arrêter et mains libres. Qualité Rapide/base ou Précise/small, lexique personnel.
- Correction française Grammalecte locale : suggestions acceptées explicitement, Ignorer et Ajouter au lexique ; une suggestion obsolète ne peut plus modifier un texte changé.
-36 palettes de matières, familles, favoris et dosage, plus identifiants historiques, ressources embarquées.
- Traduction locale français, anglais, néerlandais, allemand, espagnol, italien ; saisie et zone d’écran, suivi optionnel, cache des images inchangées, arrêt et pause pendant la dictée.
- Accessibilité : grandes commandes, texte agrandi, animations réduites, préréglage mains libres posé ; pauses possibles jusqu’à6secondes. Le préréglage ne démarre pas le micro.
- Réglages→Tutoriel :12 rubriques, recherche, filtres et actions pour découvrir toutes les fonctions.
- Nouveau logo original Écho : PNG, ICO, icônes natives, favicon et identité dans les réglages et le guide.

## Fichiers et provenance des changements
Les composants Notes, capture, palettes, moteur vocal et traduction proviennent de la base rapprochée V2/V2.1 ; ils sont copiés physiquement dans V3. Les modules internes voice_v21.rs et capture_v21.rs gardent leur nom technique.

Changements V3 :
- application/src/lib/api.ts, app.css et routes/+layout.svelte : préférences d’accessibilité, propagation entre fenêtres, grandes commandes et mouvement réduit.
- application/src/routes/settings/+page.svelte et src/lib/Tutorial.svelte : nouvel onglet accessibilité, préréglage et tutoriel complet.
- application/src-tauri/src/workspace.rs : identité/version et validation/persistance des nouveaux réglages ; préservation des préférences inconnues.
- application/src-tauri/src/lib.rs et tray.rs : moteur chargé à la demande, état du micro affiché correctement, identité V3.
- moteur/echo_worker.py : plafond de pause aligné à6secondes ; tests sur le vrai segmenter.
- application/src-tauri/tauri.conf.json et capture_v21.rs : fenêtres sans ombre pour conserver les dimensions utiles ; empaquetage V3 autonome.
- Manifestes npm/Cargo, intitulés, clés de cache frontend et marqueur portable : isolation V3.
- application/static/brand/echo-v3.png, logo-bulle.png, favicon.png et src-tauri/icons : nouvelle identité.
- docs : conception, recherche OSS, preuves de compilation, essais et livraison.

Les sources d’origine n’ont pas été utilisées comme zone de développement V3. Seul leur cache Cargo ignoré D:\Écho\v2.1\application\src-tauri\target a servi à accélérer la compilation. Les deux copies partielles de cache propres à V3 ont été supprimées. SOURCES-V21-PRESERVEES.json donne le résultat exact de comparaison avec la base copiée ; PROTECTION-DONNEES.json donne le contrôle des anciennes notes après sauvegarde.

## Vérifications effectuées
Frontend : vérification Svelte sans erreur ni avertissement,18 scénarios réussis et build de production réussi. Rust release :39 tests réussis,0 échec. Segmenter Python :2 tests réussis. Compilation Windows et NSIS réussies ;9 avertissements natifs de code inutilisé, aucune erreur.

Essais de l’exécutable final, SHA256 771c809d868c89c1a7f7702e2a46ccb3a7ad66075923ffdfacce08ca591fecd7 :
- vrai viewport du socle196×64, micro44×44 entièrement visible, aucun ancien texte au démarrage ;
- ajout/retrait/réduction/ordre/hauteur/largeur des blocs, épingle, autosauvegarde et dernière frappe ;
- thèmes et propagation ; après fermeture normale et relance, disposition distincte510px/bloc330px, thème, favoris, intensité, position et épingle retrouvés ;
- grandes commandes, micro fermé lors du préréglage,12 rubriques du tutoriel, recherche et logo réellement chargé ;
- vrai micro manuel et mains libres brièvement activés puis annulés, aucune modification de la note ;
- traduction saisie EN→FR, OCR natif de la seule fenêtre synthétique dédiée, cache, suivi42→125euros, zone vide, Stop et absence de reprise pendant deux cycles ;
- refus natif de traduction pendant le micro ouvert, bouton Stop utilisable pendant cette pause, aucune reprise après annulation ;
- capture de la région de la fixture, rectangle/texte par gestes dans l’éditeur, annuler/rétablir, recadrage sans perte, Copier et ranger, PNG640×160 et éditeur masqué ;
- correcteur : positions UTF16 avec emoji, correction acceptée et enregistrée, négation/nombre conservés, suggestions obsolètes refusées, Ignorer et lexique sans correction automatique ;
- récupération d’une prise synthétique24,576s après relance, véritable transcription Rust→Python→note en3,812s, une seule insertion,125euros et négation présents, micro fermé, fichier audio de cette prise supprimé après succès ;
- fermeture normale, sortie du processus et port9226 fermé avant livraison.

Les premiers essais pendant la compression utilisaient un candidat avant la modification finale des métadonnées du binaire. Les contrôles ci-dessus ont été exécutés à nouveau sur le SHA final. Les preuves conservées sont sous docs/validation, avec les échecs initiaux et leur diagnostic. Le premier test de persistance dépendait d’un repère de test absent ; un second contrôle compare une disposition distincte à un rapport extérieur à l’application. Le premier test du correcteur visait la note active du bloc plutôt que celle réellement affichée dans Notes ; le périmètre a été corrigé dans le test, sans changer l’application.

## Limites réelles
- Insertion dans une autre application : le test s’est arrêté avant toute insertion, Windows refusant le premier plan de la fixture. Les contrôles natifs existent mais le parcours complet reste à confirmer sur ce PC.
- L’aperçu est provisoire et dépend du moteur/PC. Aucun délai de partiel parlé ni taux d’erreur sur parole réelle n’a été mesuré ici. Le test audio synthétique ne les remplace pas.
- OCR Windows français seul disponible : la référence PTS-125 a été espacée en PTS-1 25, malgré le montant125 et la négation corrects. Le repli est signalé dans l’interface.
- La région de capture/traduction a été validée par les commandes natives sur le rectangle exact d’une fenêtre dédiée ; le geste de sélection libre n’a pas été testé de bout en bout.
- Aucun essai avec parole atypique, utilisateur handicapé ou distance de plusieurs mètres. Vosk, sherpa-onnx et RNNoise restent des pistes documentées, pas des fonctions intégrées.
- Python/faster-whisper et les modèles Whisper utilisent les ressources déjà présentes sur ce PC. Le paquet embarque les modèles de traduction, pas un environnement vocal complet pour un autre PC.
- L’installateur a été produit et contrôlé par empreinte ; installation/désinstallation Windows non exécutées. Le script NSIS généré a été revu : suppression des fichiers empaquetés puis retrait non récursif du dossier installé, sans suppression récursive des notes voisines.
- La sauvegarde finale n’est déclarée vérifiée que par docs/SAUVEGARDE-FINALE.json : ZIP relu, CRC et SHA de chaque fichier contrôlés.

Pour une première prise, choisir Réglages→Voix et moteur→Précise si la priorité est la qualité, puis essayer une phrase courte. Les changements de disposition et d’apparence restent indépendants du moteur vocal.

Agent /root — Consolidation Écho V3 et accessibilité.
