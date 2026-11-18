# Outils examinés pour Écho V3
Recherche du21octobre2026, sources officielles consultées. Aucun de ces noms ne signifie qu’un logiciel tiers a été installé.

| Projet | Intérêt concret | Décision V3 |
|---|---|---|
| [Argos Translate](https://github.com/argosopentech/argos-translate) | Bibliothèque de traduction hors ligne, modèles par paire de langues | Retenu via les modèles compacts déjà vérifiés et notre moteur CTranslate2. Widget texte/zone/suivi six langues. |
| [Vosk](https://alphacephei.com/vosk/) | API de streaming hors ligne, français, vocabulaire configurable | Candidat pour commandes courtes et latence ; pas intégré ni comparé sur ce micro. |
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) | ASR streaming/hors ligne, VAD et Windows | Candidat technique ; les modèles ont leurs propres langues/licences et une mesure française reste nécessaire. |
| [RNNoise](https://github.com/xiph/rnnoise) | Réduction de bruit, BSD3 | À comparer avec/sans filtre sur voix faible et bruit. Pas ajouté par défaut : une réduction agressive peut altérer des sons utiles. |
| [Pot](https://github.com/pot-app/pot-desktop/blob/master/README_EN.md) | Interaction sélection de texte, OCR et traduction ; Windows.Media.OCR proposé | Référence ergonomique seulement, aucun code copié. Dépôt marqué archivé le7septembre2026 lors de la consultation. |
| [Grammalecte](https://www.grammalecte.net/) | Orthographe/grammaire françaises, GPL3 | Intégré en privé avec son code/licence ; corrections acceptées manuellement et lexique personnel. |

## Ce que Microsoft propose réellement

[Accès vocal Windows](https://support.microsoft.com/en-us/accessibility/windows/voice-access/use-voice-access-to-control-your-pc-author-text-with-your-voice) permet déjà le contrôle et la dictée hors ligne sous Windows11 compatible. Ce n’est pas la même fonction que la saisie vocale Win+H nécessitant Internet. Certaines nouveautés, dont la dictée fluide, sont limitées à certains matériels et langues : [guide officiel](https://support.microsoft.com/en-US/Accessibility/windows/voice-access/get-started-with-voice-access). Écho ne doit pas promettre de remplacer tout le pilotage Windows.

## Parler de loin

Le signal capté dépend du micro, de sa directivité, de la distance et de la réverbération. Une parole inaudible ne peut pas être restaurée par un dictionnaire. Les [travaux Microsoft sur les microphones distants](https://www.microsoft.com/en-us/research/project/sound-capture-speech-enhancement/) distinguent annulation d’écho, formation de faisceau et réduction du bruit ; ce sont plusieurs traitements, pas une option universelle.

V3 conserve le choix du périphérique, le niveau visible et les mains libres. Des pauses longues évitent de couper une phrase lente. Pour valider la distance, il faudra enregistrer une même phrase à plusieurs distances avec le micro choisi, mesurer erreurs et latence, puis refaire avec bruit. Aucune portée en mètres garantie. Aucune validation auprès de personnes ayant une dysarthrie ou d’autres particularités de parole réalisée.

## Confidentialité et widgets

Traduction locale et zone choisie explicitement, aucun abonnement/API Google ajoutés. Suivi arrêt sur masque/retrait/quitter et priorité à la dictée. OCR Windows fr-FR seul actuellement présent : le repli dans les autres langues latines est annoncé, et ses limites de diacritiques restent documentées. Le runtime Python et le cache Whisper de ce PC ne sont pas encore un paquet universel pour un autre PC.

Agent /root — Consolidation Écho V3 et accessibilité.
