---
name: "Écho V2.1"
description: "Système visuel de l’application Windows : notes, voix et captures."
colors:
  bg: "#f5f6f9"
  surface: "#ffffff"
  sidebar: "#f0f1f6"
  text: "#242438"
  muted: "#646477"
  border: "#dddfe8"
  hover: "#e9eaf2"
  accent: "#5d4dba"
  accent-hover: "#4c3d9d"
  accent-soft: "#efebff"
  on-accent: "#ffffff"
  danger: "#b82d46"
  on-danger: "#ffffff"
  danger-soft: "#ffedf0"
  success: "#25754d"
  canvas: "#e5e7ed"
  nuit-accent: "#b4a5ff"
  ocean-accent: "#126d91"
  foret-accent: "#267458"
  violet-accent: "#7450b3"
  rose-accent: "#a33e6a"
  ambre-accent: "#96601f"
  aurore-accent: "#b64c36"
  nuit-on-danger: "#3d101b"
typography:
  body:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif"
    fontSize: "14px"
  note:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif"
    fontSize: "17px"
    lineHeight: 1.8
  note-title:
    fontSize: "21px"
    fontWeight: 640
    lineHeight: 1.3
  label:
    fontSize: "13px"
    fontWeight: 500
  settings-title:
    fontSize: "22px"
    fontWeight: 600
rounded:
  control: "8px"
  icon: "6px"
  navigation: "7px"
  capture-card: "10px"
  voice: "12px"
spacing:
  button: "7px 12px"
  field: "8px 10px"
  settings: "36px 38px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "{spacing.button}"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "{spacing.button}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "{spacing.button}"
  field:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "{spacing.field}"
  icon-button:
    textColor: "{colors.muted}"
    rounded: "{rounded.icon}"
    width: "34px"
    height: "34px"
  navigation:
    textColor: "{colors.muted}"
    rounded: "{rounded.navigation}"
    padding: "11px 12px"
  notice:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "10px 12px"
---

# Design System: Écho V2.1

## Overview

**Creative North Star: "L’atelier de notes"**

La page, la voix et l’image disposent de surfaces stables, reliées par le dossier choisi. La couleur sert à repérer une action, une sélection ou un état pendant le travail. Cette base documente l’application Windows réalisée ; la prochaine intégration des 36 matières est confiée séparément à l’agent Couleurs & matières.

Les surfaces claires froides, la typographie Segoe UI et les contours discrets donnent la priorité aux contenus. Les huit thèmes partagent la même structure. La lecture des notes garde davantage d’espace que les réglages et les commandes.

**Key Characteristics:**

- Surfaces fonctionnelles stables pour écrire, parler et retoucher.
- Typographie Windows et document central lisible.
- Accent pour agir et état du microphone toujours explicite.
- Couleurs partagées entre les fenêtres et sauvegarde visible.

Document extrait du code final et du contrat `docs/interface-v21.md`. La direction a été choisie sous la délégation de Louis après indisponibilité du lanceur Impeccable ; aucun tirage, aucune maquette approuvée ni aucun résultat de détecteur inexistant n’est revendiqué. La revue indépendante finale conclut **ship** pour les correctifs évalués ; voir `../docs/REVUE-VISUELLE.md`.

## Colors

### Primary

Le violet d’encre du thème Clair est l’accent de l’action. Chaque thème possède son accent et ses états associés. Les tokens ci-dessus décrivent la base Clair et les accents des variantes ; les définitions complètes restent dans `src/lib/themes.css`.

### Neutral

Le fond froid, la surface blanche, la bibliothèque légèrement teintée, les contours et l’encre sombre structurent l’espace de lecture. Le texte secondaire utilise un token distinct ; il ne dépend pas d’une baisse arbitraire d’opacité du texte principal.

### Status

Danger, texte sur danger, fond de danger et succès sont des rôles distincts. Le thème Nuit possède un texte sombre sur son bouton Arrêter clair ; ne pas lui réappliquer le texte blanc de Clair. Le contraste observé dans l’état actif final est de 8,42:1 ; il concerne cette combinaison mesurée.

**The Useful Accent Rule.** L’accent distingue les actions principales, la sélection courante, les liens, le caret et le focus. Les fonds de lecture utilisent les surfaces du thème.

Les huit identifiants actuels sont `clair`, `nuit`, `ocean`, `foret`, `violet`, `rose`, `ambre`, `aurore`. L’ajout des 36 palettes devra raccorder les ressources de la collection à ces rôles et étendre la validation native. La base actuelle ne contient pas encore leurs textures.

## Typography

La famille Segoe UI Variable, puis Segoe UI et les polices système, est un choix explicite pour Windows. Aucune famille décorative séparée n’est ajoutée.

Le document utilise le rôle note avec une cible de lecture de 72ch. Les titres de notes sont compacts et plus fermes ; les titres des réglages restent adaptés à une fenêtre utilitaire. Les commandes partagent le rôle label. Le code Markdown utilise Consolas à .92em.

Les métadonnées locales de 10–11 px existent dans certains éléments ; elles ne constituent pas une règle de corps de texte à propager.

**The Readable Page Rule.** Le texte de note garde sa typographie de lecture ; les petites métadonnées ne définissent pas la taille du contenu.

## Layout

Application de bureau Windows au clavier et à la souris. Les dimensions ci-dessous sont celles de ses fenêtres et de son code, pas une promesse de prise en charge mobile.

| Surface | Taille initiale | Minimum | Structure |
| --- | --- | --- | --- |
| Notes | 1240 × 820 | 750 × 560 | Bibliothèque de 236 px, document central, sauvegarde en en-tête |
| Barre vocale | 650 × 224 | Taille gérée par l’application | Destination, mode, bouton Parler/Arrêter et résultat |
| Retouche | 1240 × 820 | 900 × 620 | Outils de 178 px, grande surface d’image, propriétés des objets |
| Réglages | 960 × 800 | 720 × 600 | Navigation de 220 px, contenu défilant |

À 760 px ou moins dans Notes, la bibliothèque ouverte réserve **210 px dans la grille** ; elle reste repliable et ne recouvre pas le texte. Le statut de sauvegarde reste visible. Dans les réglages, le rail passe à 185 px à cette largeur. La barre vocale s’agrandit par le code pour un résultat ou une erreur.

Les seuils de 900 px pour les notes et 1050 px pour les captures condensent les contrôles. Le fallback CSS de retouche à 800 px n’est pas une surface mobile livrée : sa fenêtre native exige au moins 900 px.

## Elevation & Depth

Les plans permanents utilisent des tons et des bordures de 1 px. Les ombres du menu de note, de l’image et des actions flottantes servent les chevauchements réels. Le token global d’ombre déclaré mais inutilisé ne constitue pas une nouvelle règle à appliquer à toutes les surfaces.

**The Stable Planes Rule.** Les régions permanentes sont séparées par leurs tons et leurs contours. Les ombres correspondent aux éléments réellement superposés.

Le détail des ombres observées et des transitions figure dans `.impeccable/design.json`. Les couleurs des boutons évoluent en 140 ms ease-out ; la largeur du vumètre en 60 ms linear. La préférence de réduction des mouvements ramène transitions et animations à .01 ms.

## Shapes

Les contrôles sont des rectangles doucement arrondis. Les valeurs du frontmatter distinguent commandes, icônes, navigation, cartes de capture et coque vocale. Les bordures servent les champs et séparations, tandis que les boutons fantômes se révèlent au survol.

Les boutons standards ont une **hauteur minimale** de 36 px, les champs de 38 px. Ces minimums sont conservés comme CSS dans le sidecar, sans être présentés comme des hauteurs fixes de tokens.

## Components

### Buttons

Le bouton principal utilise l’accent et le texte sur accent. Le secondaire garde une surface et une bordure ; le fantôme utilise le texte secondaire. Le survol, le focus et l’état désactivé partagent les règles globales : contour de focus de 2 px avec décalage de 3 px, opacité .48 pour la désactivation.

### Inputs / Fields

Les champs utilisent la surface, une bordure du thème et un caret d’accent. Le focus change la bordure. Les erreurs doivent rester proches du champ ou de l’action concernée et reprendre les rôles de danger.

### Navigation

La bibliothèque et les réglages emploient des lignes lisibles avec une sélection teintée. Le fond d’accent doux, le texte d’accent et la graisse renforcée distinguent la section active. Les compteurs restent actualisés pendant la saisie des notes.

### Voice

La destination et le mode précèdent l’action. Le bouton devient Arrêter pendant l’écoute ; sa couleur, son texte et l’état du microphone sont visibles ensemble. La fin de prise et la transcription sont des états distincts.

### Capture Editor

Les outils restent fixes autour de l’image. Les objets de retouche restent modifiables ; l’interface thémée ne colore pas les pixels de la capture ni les annotations déjà choisies par l’utilisateur. Copier et ranger conserve le PNG en même temps que sa copie.

### Notices

Les retours ordinaires utilisent la surface d’accent doux et le texte courant. Les erreurs utilisent les rôles de danger. Une confirmation de sauvegarde décrit une écriture confirmée, pas une simple intention.

## Do's and Don'ts

### Do:

- Do utiliser les variables du thème pour les commandes et leurs états.
- Do conserver la confirmation de sauvegarde visible dans la largeur compacte.
- Do garder le bouton Arrêter lisible dans chaque thème.
- Do respecter la réduction des mouvements demandée par Windows.

### Don't:

- Don't remplacer les composants fonctionnels par les simulations de la galerie de palettes.
- Don't faire passer la bibliothèque compacte au-dessus du texte.
- Don't recolorer le contenu ou les annotations d’une capture lors d’un changement de thème.
- Don't présenter les adaptations de largeur Windows comme une interface mobile validée.
