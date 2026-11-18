# REPRISE Écho V3 — 2026-10-25 03:25

Dernière version validée par Louis :
D:\Écho V3\Écho V3-candidat-target-first.exe
SHA256 891B01057E98C4F3FBCF45162A01A65040B5D4B2D4248B44001E113C167D1604

État vocal validé : instantané + insertion au curseur.
Worker SHA256 AD9CAA251809F244D5DAEE3D0F784B6C9F1C26855C3C88B09C5C113E6644203E
voice_v21.rs SHA256 B11E22642CE1886ECFF435FCFD86B41031E6E635B4C233C54474066E8F3DA780

Dernier correctif NON encore livré en release :
- Texte en direct forcé temporairement tout en haut pendant la dictée
- Bloc-notes conservé dessous
- colonne remontée automatiquement
- 24 tests frontend verts

La release custom-protocol était encore en compilation au moment du reboot.
Après reboot : npm test, npm run check, npm run build, puis cargo build --release --features custom-protocol avec CARGO_TARGET_DIR=D:\Écho\v2.1\application\src-tauri\target.

Ensuite lancer un seul binaire FINAL et faire le test 15-20 s : Direct visible pendant la parole + final au curseur.
Si validé : préparer la publication open source séparée, sans données privées.

Sauvegarde complète des sources :
D:\Écho V3\docs\SAUVEGARDE-AVANT-REDEMARRAGE-20261025-0325

Nettoyage déjà fait : vieux V2, vieux EXE V3, vieux setup et EXE archivés supprimés.
Ne pas supprimer D:\Écho V3, D:\Laboratoire-Vocal-BubuS, D:\Echo-OpenSource-BubuS-dev.
D:\Écho\v2.1 reste temporairement le cache Rust.
