# Écho — prompt à coller

> Prompt court et autonome, à coller tel quel à n'importe quel agent.

---

**Écho — qu'est-ce que c'est**

> Une application de bureau Windows qui capture la voix, la garde **hors ligne**, et écrit des notes Markdown dans un dossier que l'utilisateur choisit. « J'appuie, je parle, le texte apparaît dans ma note du jour. Rien ne quitte ma machine. »

**La promesse en une ligne** : ne plus jamais perdre une idée dite à voix haute, sans envoyer une seule seconde d'audio dans le cloud.

**Ce que c'est**
- Une petite fenêtre flottante, épinglable, déplaçable, avec un bouton micro et un VU-mètre.
- Un dictaphone propre : l'audio est rangé dans `<dossier de notes>/.echo/audio/`, jamais perdu.
- Un bloc-notes : la note du jour `AAAA-MM-JJ.md`, des entrées horodatées, un historique des enregistrements cliquable.
- Une sauvegarde indestructible : journal de récupération + écriture atomique — une entrée confirmée à l'écran survit à un crash, sans doublon.

**Ce que ce n'est pas**
- Pas un assistant conversationnel, pas un cloud, pas une extension navigateur.
- Pas de compte, pas de télémétrie. Local par défaut, tout le temps.

**La technique**
Tauri 2 + SvelteKit + Rust. Transcription prévue via whisper.cpp (modèle `small` multilingue, langue `fr`) dans un processus isolé, en boucle locale uniquement.

**Où en est le projet** (19 octobre 2026)
- **M1 — écriture indestructible** : livrée. Journal append-only synchronisé, écriture atomique, rejeu idempotent. 100/100 entrées conservées sous arrêts brutaux, 0 doublon.
- **M1.1 — corrections après revue** : livrée. Note illisible refusée, conflits externes préservés, réessai sans doublon, dossier journalisé, journal abîmé signalé.
- **M2 — capture audio** : livrée. WAV 16 kHz mono, filtre anti-repliement, VU-mètre, pause/reprise. 28 tests.
- **M3 — transcription** : à faire. C'est elle qui transforme le dictaphone en vrai bloc-notes vocal.
- **Ensuite** : nettoyage local du texte (ponctuation, hésitations), dictée vers d'autres applications.

**Les règles du projet**
1. Rien ne quitte la machine sans action explicite.
2. Jamais d'écriture qui écrase silencieusement un fichier modifié dehors.
3. Un stub retourne une erreur explicite, jamais un faux succès.
4. On mesure avant d'affirmer ; « ça devrait marcher » n'existe pas.

Dépôt public : <https://github.com/Louistdg/echo>
