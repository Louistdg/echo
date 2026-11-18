# ÉCHO V3 — réactivité de la dictée, 23 octobre 2026

## Installation

Le correctif de placement du curseur est installé et Louis a confirmé qu’il écrit dans ChatGPT sans rouvrir le bloc-notes. À sa demande, le délai a ensuite été travaillé.

Le moteur optimisé est installé dans `D:\Écho V3\moteur\echo_worker.py` : SHA-256 `84863849fcc0009f4d88cef1ceba6ce411b2e58c7b2b3f3ab08379be5e97bd3e`. L’exécutable reste celui vérifié et livré précédemment, SHA-256 `41e0e9621d883e7ca8dbde9d89fd92979f8a226532de0273b6b952b4d9117d5c`.

L’installateur 3.0.1 a été refabriqué avec ce moteur, puis remplacé après contrôle de son empreinte. Détails : `docs/performance-20261023/PERFORMANCE-INSTALLEE.json`. Les empreintes de livraison du 23 octobre ont été mises à jour. Le `BUILD.json` de la première correction reste sa preuve historique ; le rapport de performance décrit la mise à jour des ressources Python. Les sources natives et l’interface ont été comparées aux empreintes de cette compilation : elles sont inchangées.

Écho a été quitté par son menu normal et relancé sans débogueur. Le moteur a été préchargé via le bouton des réglages ; état « Moteur prêt » observé à 17 h 50. Les réglages ont été refermés et le petit socle est resté seul visible. Le processus de moteur chargé utilise environ 497 Mio de mémoire à cette observation.

## Changements

- Avant : un seul objet de modèle. Chaque aperçu chargeait Base et la phrase finale rechargeait Small ; les poids étaient déchargés et relus entre les deux.
- Maintenant : cache limité à deux modèles. Aperçu et version finale réutilisent leurs instances déjà prêtes. Le changement de modèle conserve une borne mémoire et l’échec d’un chargement laisse le modèle d’aperçu disponible.
- Le bouton « Charger le moteur » prépare l’aperçu et le modèle final choisi.
- Pause entre phrases enregistrée à 600 ms, au lieu de 900 ms.
- Qualité finale Small, recherche avec faisceau de 5, langue française et fonctionnement local conservés. Aucun service payant ni nouveau téléchargement.
- Dix tests du moteur réussis, dont alternance sans rechargement, borne du cache, échec de chargement et annulation sans émission de résultat.

## Mesures

Même enregistrement vocal, recadrage de quatre secondes et original de 8,79 secondes, séquence aperçu → texte final. Les mesures portent sur le calcul après préchargement, sans la pause de fin de phrase ni l’insertion dans l’interface.

| Essai final | Avant | Après | Texte final |
| --- | ---: | ---: | --- |
| 1 | 5,625 s | 4,203 s | Identique |
| 2 | 6,109 s | 3,000 s | Identique |
| 3 | 5,579 s | 3,500 s | Identique |

Gain moyen dans cette comparaison : **38,2 %**. Aucun rechargement de modèle dans les trois étapes finales après préchargement. Une première mesure plus légère donnait 4,2–4,6 s avant et 2,4–2,9 s après. La charge du PC fait donc varier les durées. Une mesure effectuée pendant la compression de l’installateur est également conservée, avec des durées plus élevées ; elle n’est pas présentée comme une comparaison à charge contrôlée.

Le premier préchargement prend davantage de temps puisqu’il prépare deux modèles. Le gain concerne les phrases lorsque les modèles sont prêts. L’identité du texte sur cet enregistrement français ne constitue pas une mesure générale de précision ni une validation nouvelle de toutes les langues.

## Preuves et reprise

- `docs/performance-20261023/AVANT-FINAL.json`, `APRES.json` : comparaison finale.
- `APRES-PREMIERE-MESURE.json`, `APRES-PENDANT-FABRICATION.json` : autres mesures conservées.
- `SELF-TEST.json` : dix tests et empreinte du moteur exact.
- `worker-avant.py` : version du moteur avant optimisation, pour retour arrière.
- `PERFORMANCE-INSTALLEE.json` : livraison, paramètres, empreinte de l’installateur et limites de mesure.
- `docs/correctif-direct/PRODUCTION-LANCEE.json` : dernier lancement et empreintes du programme/moteur.

Les essais de l’ancien dossier `correctif-direct/qa-runtime` restent historiques pour la correction initiale ; leur moteur copié est antérieur à cette optimisation. Pour un nouvel essai QA, recopier le moteur courant après avoir fermé l’instance QA et générer de nouvelles preuves.

Les notes n’ont pas été supprimées : Louis autorise leur réorganisation, mais elle n’était pas nécessaire pour réduire le délai. Un nouvel essai utilisateur de réactivité a été demandé via « Action requise » ; retour en attente à cette rédaction.
