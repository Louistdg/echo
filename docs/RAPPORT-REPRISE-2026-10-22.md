# ÉCHO V3 — reprise du 22 octobre 2026

## Demande de Louis

Terminer le correctif 3.0.1, vérifier l'ancien travail, conserver le fonctionnement local gratuit, afficher la transcription pendant la parole et permettre de traduire directement le texte du bloc-notes. Français et anglais prioritaires ; allemand et néerlandais complémentaires. Aucune API OpenAI et aucun achat.

## Avant la reprise

Le programme portable livré était en 3.0.0. Les sources du correctif 3.0.1 étaient déjà présentes, avec le clic pour démarrer/arrêter le micro, la barre compacte de 244 × 64, les accès directs capture/bloc-notes, le choix automatique du champ et les dimensions réglées dans une fenêtre fixe.

La sauvegarde avant intervention est référencée dans `REPRISE-ACTUELLE.json`. Les 71 entrées du ZIP ont été comparées au manifeste SHA-256, sans divergence.

## Travail ajouté pendant cette reprise

- Contrôle Svelte : zéro erreur, zéro avertissement. Tests frontend existants réussis.
- 8 auto-tests du worker de dictée et 4 régressions de découpe des phrases réussis.
- 41 tests Rust réussis sur les sources natives conservées. Le nouveau paquet réutilise cette preuve après comparaison de toutes les sources natives ; aucune modification Rust dans l'ajout Traduire.
- Ajout du bouton Traduire au bloc-notes compact et du panneau `NoteTranslation.svelte`. Le texte n'est pas remplacé ; le résultat est distinct, copiable, et masqué si la note ou les langues changent. La traduction est désactivée pendant la dictée.
- Essai réel du moteur installé : il supprimait silencieusement la deuxième phrase sur certains paragraphes français/anglais. Correction dans `moteur/echo_translate.py` : traduction de chaque phrase, avec conservation des séparateurs et traitement des abréviations usuelles.
- Signalement des nombres/heures reformulés par le modèle. Exemple constaté : « 14 heures » devient « 2:00 » sans précision de période ; la sortie nécessite une relecture.
- Essais réels des modèles locaux français → anglais, anglais → français, français → allemand, français → néerlandais. Les deux phrases sont maintenant restituées dans les cas de régression français/anglais.
- Essais du worker de dictée réel, modèle small, sur voix synthétiques française et anglaise. Résultats dans `correctif-direct/DICTEE-LANGUES-REELLE.json`. Des erreurs restent possibles sur les noms propres (« Ouvran » pour « Ouvrane ») ; ces essais ne remplacent pas la voix et le micro de Louis.

## Choix open source

- Dictée : faster-whisper/CTranslate2 en CPU int8, modèles multilingues déjà installés. Aperçu base et résultat final small selon le réglage. https://github.com/SYSTRAN/faster-whisper
- Traduction : modèles Argos déjà installés, exécutés localement par CTranslate2/SentencePiece. Le français/anglais utilise une direction directe ; allemand/néerlandais peuvent passer par l'anglais. https://github.com/argosopentech/argos-translate
- Whisper et ses poids sont sous licence MIT : https://github.com/openai/whisper

## Validation et livraison en cours

### Échec observé dans la vidéo de Louis (22 octobre, enregistrement 19:52)

### Précision de Louis après l’essai manuel

Priorité confirmée : le petit socle doit dicter à l'emplacement du curseur, dans l'application où Louis écrit, sans sélection « Note / Application » ni redirection imposée vers les notes Écho. Le projet temporaire de restitution directe dans une note figée a été annulé avant livraison. La grande fenêtre et la traduction restent des outils annexes.

Le dernier essai a capté et transcrit plusieurs phrases. Une partie est enregistrée dans `Essai micro Écho V3.md`, puis la protection UI Automation refuse les insertions suivantes avec « Le champ actif a changé ». L’essai et les prises en attente ont été archivés dans `correctif-direct/essai-manuel-bloque-20261022-2228` avant fermeture normale du candidat. Cela ne constitue pas une validation du parcours complet.

Corrections suivantes : identification du champ parent pour les fragments éditables, rôle accessible explicite du champ Markdown, prise en charge d'une touche simple configurable pour la dictée (1, lettre, F8), et démarrage/arrêt au deuxième appui pour une touche seule. Les raccourcis de capture gardent leur combinaison avec modificateur. La touche simple est réservée à Écho pendant son exécution ; le réglage le précise. Nouvelle compilation et essais nécessaires.

L'ancienne conversation du 21/10/2026 à 02:42:28 confirme le socle minimaliste en bas à droite et les blocs ajoutés au-dessus. Le dessin référencé `codex-clipboard-9514968b-f79e-44ec-b137-171a2b27fd4c.png` n’est pas embarqué dans l’export texte et son chemin temporaire n’existe plus. Son contenu visuel n’a pas été relu ; la description écrite a été retrouvée.

La grande fenêtre « Mes notes » et le bloc-notes compact ont deux parcours différents. Le bouton « Dicter » de la grande fenêtre appelait uniquement `show_window(quick)` : il ne démarrait pas le micro. La petite barre restait sur un autre bureau Windows, alors que Louis utilisait Bureau 2. Les essais précédents du bloc compact ne validaient donc pas ce parcours réel. Un fichier WAV de 8,79 secondes a bien été créé dans le profil QA : l'enregistrement audio et la dictée sont deux fonctions distinctes.

Le WAV réel de Louis a ensuite été transcrit localement avec le worker actuel et le modèle small : son test vocal est reconnu en 9 secondes (preuve `correctif-direct/ENREGISTREMENT-MICRO-REEL.json`). Cela confirme la présence de sa voix dans cet enregistrement et la capacité du moteur à le transcrire ; cela ne valide pas encore la dictée en direct ni l'insertion dans la note.

Corrections en cours de compilation :

- Démarrage et arrêt de la dictée directement dans la grande note, sans passer par la petite barre. Préservation du curseur, aperçu et état du micro visibles dans cette fenêtre.
- Option Traduire ajoutée à cette même fenêtre, français/anglais en premier, allemand et néerlandais disponibles. Résultat distinct et original conservé.
- Autorisation native de traduction de texte limitée aux deux fenêtres de notes. Les captures et traductions d'écran conservent leur restriction au panneau compact.
- La petite barre et ses panneaux doivent rejoindre le bureau Windows de la grande fenêtre visible, via `IVirtualDesktopManager`. Aucun changement de bureau ni activation forcée pendant la frappe. Ce comportement doit encore être observé après compilation.
- Étiquette de version de la grande fenêtre corrigée de « v2.1 » à « V3 ».

Ces modifications remplacent la preuve native précédente : une nouvelle exécution complète des tests Rust et une nouvelle compilation sont requises. Aucun résultat de test de l'ancien exécutable ne valide automatiquement ces nouveaux changements.

La compilation intégrant ces changements est en cours. Le programme et l'installateur ne doivent être annoncés livrés qu'après vérification du BUILD.json final, des empreintes et des essais graphiques. Le parcours complet dans le programme compilé, l'aperçu avant arrêt du micro et l'essai personnel de Louis restent à terminer.

Le contrôle de stabilité de cette compilation couvre le code, la configuration et les manifestes. Les poids `vendor/model.bin`, inchangés par cette mission, ne sont pas relus intégralement à chaque contrôle sur le disque saturé ; leurs empreintes de référence sont conservées dans le rapport de compilation antérieur. Il ne s'agit pas d'une nouvelle certification de tous les modèles tiers.

## Emplacements

- Projet : `D:\Écho V3`
- Sources : `D:\Écho V3\application`
- Moteurs locaux : `D:\Écho V3\moteur`
- Profil d'essai isolé : `D:\Écho V3\docs\correctif-direct\qa-runtime`
- Preuves : `D:\Écho V3\docs\correctif-direct`
- Rapports des anciennes conversations : `D:\Projets recuperes - Conversations et documents\02 - ECHO`

Les états « avant la perte » restent des documents historiques distincts de cette reprise.

## Vidéos du 23 octobre — micro qui rouvre le bloc-notes

Deux vidéos examinées, dont `20261023-1446-23.2388717.mp4` (9,83 secondes). Dans cette dernière, panneaux fermés à 6,567 secondes, rouverts à 7,4 secondes, puis fermés à 8,2 secondes ; socle encore visible. Images décodées réellement avec PyAV, bande son non transcrite.

Cause identifiée dans `application/src/routes/quick/+page.svelte` : le clic micro est détourné vers `toggle-live` lorsqu'un ancien résultat est conservé. L'ouverture de la colonne restaure aussi le bloc-notes. Un second mécanisme `live-auto` ouvre automatiquement cette colonne au début de chaque dictée.

Modification des sources : suppression de ces deux ouvertures automatiques depuis le micro. Le bouton conserve désormais son action démarrer/arrêter. Le bouton bloc-notes reste explicite. Vérification Svelte après modification : 0 erreur, 0 avertissement.

Non livré : la fenêtre de test déjà ouverte utilise encore l'exécutable précédent. La gestion native des résultats bloqués doit être traitée sans perdre texte ni audio ; l'insertion au curseur échoue encore dans les derniers essais. Ne pas annoncer la dictée corrigée ou le programme déployé sur la seule base de cette modification d'interface.
## Correctif du 23 octobre — compilation et reprise des prises

Le bouton du socle ne lance plus `toggle-live` ni `live-auto`. La capture de cible et l'insertion invoquées depuis une WebView sont désormais asynchrones avec travail natif hors du fil de la fenêtre ; le fil de fenêtre reste disponible pour les requêtes UI Automation de Windows. Les appels internes au moteur utilisent des fonctions natives synchrones depuis leurs propres fils.

Une dictée ne démarre plus avec une cible vide : la vérification du champ doit réussir avant l'ouverture du micro. Lorsqu'un résultat bloque une nouvelle dictée, les anciennes prises sont sauvegardées (JSON complet et texte lisible, WAV d'origine conservés) dans `configuration/voice-jobs/prises-conservees`, puis retirées de la file active. Elles ne sont jamais insérées dans la nouvelle destination. Un échec d'archivage laisse la file intacte et empêche le démarrage.

Contrôles frais : interface 0 erreur / 0 avertissement, tests frontend réussis, 45 tests natifs réussis avec test de conservation et échec d'écriture. La compilation finale est encore en cours : aucune livraison confirmée à ce stade.

Les prises manuelles du profil QA ont été copiées avant arrêt dans `correctif-direct/prises-avant-correction-20261023-1658` (configuration et données audio/notes). La fenêtre de test native est sur le bureau Windows actuel, mais était derrière ChatGPT. Elle a été placée visible en haut à gauche, sans injection ni activation forcée. Le déplacement interprocessus par l'API des bureaux a été refusé et n'a pas été poursuivi ; la vérification en lecture a ensuite confirmé qu'aucun déplacement n'était nécessaire.

## Installation achevée le 23 octobre 2026
La version 3.0.1 est installée et relancée. État, preuves, sauvegardes et limites : voir RAPPORT-CORRECTION-MICRO-2026-10-23.md. Le socle seul est visible et le bouton de dictée disponible. Dernier essai utilisateur dans ChatGPT demandé ; résultat encore en attente.
