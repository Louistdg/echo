# Écho V3 — état de livraison
21 octobre2026 · Windows natif DeskMCP · D uniquement

Objectif : une V3 issue du meilleur des prototypes V2 et V2.1, avec socle minimal, widgets modulables, notes/captures, aperçu vocal, correction/lexique, palettes, traduction écran optionnelle, accessibilité, tutoriel et nouveau logo.

## Livré
Programme : D:\Écho V3\Écho V3.exe
Installateur : D:\Écho V3\installateur\Écho V3_3.0.0_x64-setup.exe
Guide visuel : 00 - DÉMARRER ICI.html
Rapport : docs/LIVRAISON-V3.md
Empreintes : docs/VALIDATION-FINALE.json

EXE25789952octets, SHA256 771c809d868c89c1a7f7702e2a46ccb3a7ad66075923ffdfacce08ca591fecd7.
NSIS794429912octets, SHA256 f741aa3c06a0fca60a6506632e0e940725ed38ee3b9b52441b2a57d31484bda4.
Aucune installation C. Application laissée fermée ; port9226 fermé, processus vocal/traduction terminé. Profil QA déplacé sous docs/qa/configuration-fin-tests, aucune configuration QA à la racine. Les données de test sont docs/qa/donnees, jamais les anciennes notes.

Sources dans application, ressources moteur copiées physiquement, identité com.echo.v3/version3.0.0, marqueur echo-v3.portable. Les caches partiels V3 target et target-v3 ont été supprimés. La compilation utilisait uniquement le cache Cargo ignoré de D:\Écho\v2.1\application\src-tauri\target ; SOURCES-V21-PRESERVEES.json confirme que les fichiers recensés de la base copiée restent identiques.

## Vérifié
Frontend :0erreur,0avertissement,18scénarios et build réussis. Rust release :39réussis. Segmenter Python :2réussis. EXE et NSIS compilés ;9avertissements natifs de code inutilisé. BUILD-V3.json complet/stable.

Essais sur le SHA final : socle196×64, micro44×44, modules et dernier caractère sauvegardé avant retrait ; thèmes/propagation ; persistance après fermeture normale puis relance d’une disposition distincte510px/bloc330px et préférences ; accessibilité et tutoriel12rubriques ; ouverture/annulation du vrai micro et mains libres ; traduction saisie/OCR/suivi/cache/vide/Stop et pause native pendant dictée ; annotations et Copier et ranger ; correcteur et lexique ; récupération d’un vrai job de transcription synthétique24,576s en3,812s, une seule insertion et suppression de son audio après succès.

Preuves petites copiées sous docs/validation, y compris les échecs initiaux. Les essais pendant compression ne suffisent pas : ils ont été répétés sur le SHA final. Le repère local de test de redémarrage avait disparu ; le contrôle distinct compare au rapport extérieur. Le premier test du correcteur visait la note active du bloc plutôt que celle affichée dans Notes ; test corrigé, sources applicatives inchangées.

## Limites à conserver
Insertion externe non validée : Windows refuse le premier plan de la fixture, contrôles arrêtés sans insertion. Ne pas contourner cette garde.
Aperçu implémenté, mais précision et délai du texte parlé en direct non mesurés ; aucune promesse d’instantané mot à mot.
OCR Windows français seul : PTS-125 lu PTS-1 25 ;125euros et négation corrects, repli signalé.
La sélection native a été testée par IPC sur une région synthétique, pas le geste libre de bout en bout.
Python DSH et modèles Whisper externes déjà présents sur ce PC ; modèles traduction embarqués. Pas encore un installateur vocal universel pour autre PC.
Aucune validation avec handicap/parole atypique ni distance mesurée. Vosk/sherpa/RNNoise candidats seulement, cf.rechercheOSS.
L’installateur est compilé, pas installé/désinstallé pendant les essais.

## Sauvegarde
docs/sauvegarder-v3.py exige la livraison validée, vérifie les notes V1/V21 selon le manifeste antérieur, écrit ZIP.part en flux, relit CRC/SHA de chaque entrée, puis renomme ZIP.
Le succès final est exclusivement attesté par docs/SAUVEGARDE-FINALE.json. PROTECTION-DONNEES.json enregistre les notes vérifiées. Aucun succès de sauvegarde à déduire de ce texte seul.

## Périmètre et reprise
Ne pas toucher V1, D:\R&D\echo-v2\app, ni les notes anciennes. La fiche D:\Récapitulatif projets\Récapitulatif projets.html#echo a été lue ; elle décrit encore les prototypes et n’a pas été modifiée.
DeskMCP autorise D uniquement ; profils C explicitement refusés. Aucun contournement, WSL ou Codex Work. Aucun sous-agent supplémentaire autorisé. Tous les outils graphiques utilisent des fenêtres et données de test possédées.
Ne pas relancer les anciens scripts de copie de cache ni livrer à nouveau les deux prototypes. Les scripts livrer-v3.py et stager-… sont à usage contrôlé et refusent les fichiers existants/profils non QA ; ne pas les rejouer sans lire leurs gardes.

Agent /root — Consolidation Écho V3 et accessibilité.
