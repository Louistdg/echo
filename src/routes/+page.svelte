<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  let notesDir = $state<string | null>(null);
  let texte = $state("");
  let message = $state("");
  let enErreur = $state(false);
  // Identifiant d'opération stable : un réessai après échec réutilise le même
  // identifiant, sinon le rejeu du journal créerait une seconde entrée.
  let operationId: string | null = null;
  let operationTexte = "";
  let statut = $state<{
    capture_audio: string;
    transcription: string;
    notes: string;
    recovery: string;
  } | null>(null);

  type Enregistrement = {
    name: string;
    path: string;
    duration_secs: number;
    modified: string;
  };

  let enregistrements = $state<Enregistrement[]>([]);

  onMount(async () => {
    try {
      const settings = await invoke<{ notes_dir: string | null }>("get_settings");
      notesDir = settings.notes_dir ?? null;
      statut = await invoke("get_status");
      await rafraichirEnregistrements();
    } catch (e) {
      message = `Erreur : ${String(e)}`;
    }
  });

  async function rafraichirEnregistrements() {
    try {
      enregistrements = await invoke<Enregistrement[]>("list_recordings");
    } catch {
      enregistrements = [];
    }
  }

  async function ouvrirDossier(genre: "notes" | "audio") {
    try {
      const chemin = await invoke<string>(
        genre === "notes" ? "open_notes_folder" : "open_recordings_folder",
      );
      message = `Dossier ouvert : ${chemin}`;
      enErreur = false;
    } catch (e) {
      message = String(e);
      enErreur = true;
    }
  }

  async function ouvrirEnregistrement(path: string) {
    try {
      await invoke("open_recording", { path });
      enErreur = false;
    } catch (e) {
      message = String(e);
      enErreur = true;
    }
  }

  async function transcrire(path: string) {
    try {
      const r = await invoke<{ text: string; path: string; state: string }>(
        "transcribe_and_append",
        { path },
      );
      message = `Transcrit et ajouté à la note — ${r.path}`;
      enErreur = false;
    } catch (e) {
      message = `Transcription impossible — ${String(e)}`;
      enErreur = true;
    }
  }

  function duree(s: number) {
    const m = Math.floor(s / 60);
    return m > 0 ? `${m} min ${String(Math.round(s % 60)).padStart(2, "0")}` : `${s.toFixed(1)} s`;
  }

  async function choisirDossier() {
    try {
      notesDir = await invoke<string>("select_notes_folder");
      message = `Dossier de notes : ${notesDir}`;
      enErreur = false;
    } catch (e) {
      message = String(e);
      enErreur = true;
    }
  }

  async function ajouterALaNote() {
    if (!texte.trim()) return;
    // Même texte + échec précédent = même opération ; texte modifié = nouvelle.
    if (!operationId || texte !== operationTexte) {
      operationId = crypto.randomUUID();
      operationTexte = texte;
    }
    try {
      // L'UI ne dit « Enregistré » que si le cœur a écrit le journal ET la note.
      const r = await invoke<{ id: string; path: string; state: string }>(
        "append_to_today_note",
        { text: texte, entryId: operationId },
      );
      message = `${r.state === "enregistré" ? "Enregistré" : r.state} — ${r.path}`;
      enErreur = false;
      texte = "";
      operationId = null;
      operationTexte = "";
      // Rafraîchit le dossier affiché : après un ajout réussi, il est forcément configuré.
      const s = await invoke<{ notes_dir: string | null }>("get_settings");
      notesDir = s.notes_dir ?? null;
    } catch (e) {
      message = `NON ENREGISTRÉ — ${String(e)}`;
      enErreur = true;
    }
  }
</script>

<main>
  <header>
    <h1>Écho</h1>
    <p class="sous-titre">Bloc-notes vocal local — enregistrement prêt, transcription à venir</p>
  </header>

  <section class="dossier">
    {#if notesDir}
      <p>Dossier de notes : <code>{notesDir}</code></p>
    {:else}
      <p>Aucun dossier de notes configuré.</p>
    {/if}
    <div class="actions">
      <button onclick={choisirDossier}>Choisir le dossier…</button>
      {#if notesDir}
        <button onclick={() => ouvrirDossier("notes")}>Ouvrir le dossier des notes</button>
        <button onclick={() => ouvrirDossier("audio")}>Ouvrir le dossier des enregistrements</button>
      {/if}
    </div>
  </section>

  <section class="editeur">
    <textarea
      bind:value={texte}
      placeholder="Écris ici… (Ctrl+Alt+Espace ouvre la fenêtre d'enregistrement)"
      rows="8"
    ></textarea>
    <div class="actions">
      <button class="principal" onclick={ajouterALaNote}>Ajouter à la note du jour</button>
    </div>
  </section>

  <section class="historique">
    <div class="titre-ligne">
      <h2>Enregistrements récents</h2>
      <button class="discret" onclick={rafraichirEnregistrements}>Rafraîchir</button>
    </div>
    {#if !notesDir}
      <p class="vide">Choisis d'abord un dossier de notes.</p>
    {:else if enregistrements.length === 0}
      <p class="vide">
        Aucun enregistrement pour l'instant. Ouvre la fenêtre de dictée avec
        <kbd>Ctrl+Alt+Espace</kbd>.
      </p>
    {:else}
      <ul>
        {#each enregistrements as e (e.path)}
          <li class="ligne-li">
            <button class="ligne" onclick={() => ouvrirEnregistrement(e.path)} title={e.path}>
              <span class="nom">{e.name}</span>
              <span class="meta">{duree(e.duration_secs)} · {e.modified}</span>
            </button>
            <button class="transcrire" onclick={() => transcrire(e.path)} title="Transcrire et ajouter à la note du jour">
              → note
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if message}
    <p class="message" class:erreur={enErreur} role="status">{message}</p>
  {/if}

  {#if statut}
    <footer>
      <p>Avancement des modules :</p>
      <ul>
        <li>Capture audio : {statut.capture_audio}</li>
        <li>Transcription : {statut.transcription}</li>
        <li>Notes : {statut.notes}</li>
      </ul>
      {#if statut.recovery}
        <p class="recuperation" role="alert">{statut.recovery}</p>
      {/if}
    </footer>
  {/if}
</main>

<style>
  :global(body) {
    font-family: system-ui, sans-serif;
    background: #f6f6f8;
    color: #1a1a20;
    margin: 0;
  }

  main {
    max-width: 720px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }

  h1 {
    margin: 0;
    font-size: 1.6rem;
  }

  .sous-titre {
    margin: 0.25rem 0 1.5rem;
    color: #666;
  }

  section {
    margin-bottom: 1.25rem;
  }

  textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 0.75rem;
    border: 1px solid #c9c9d4;
    border-radius: 8px;
    font: inherit;
    resize: vertical;
  }

  button {
    font: inherit;
    padding: 0.5rem 1rem;
    border: 1px solid #c9c9d4;
    border-radius: 8px;
    background: #fff;
    cursor: pointer;
  }

  button.principal {
    background: #2d6cdf;
    border-color: #2d6cdf;
    color: #fff;
  }

  .actions {
    margin-top: 0.5rem;
  }

  .message {
    background: #eef4ff;
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
  }

  .message.erreur {
    background: #ffecec;
    border: 1px solid #e5a3a3;
    color: #7a1f1f;
  }

  .recuperation {
    margin-top: 0.75rem;
    padding: 0.5rem 0.75rem;
    border-radius: 8px;
    background: #fff6e5;
    border: 1px solid #e8c27a;
    color: #6b4a00;
  }

  footer {
    color: #666;
    font-size: 0.85rem;
    border-top: 1px solid #ddd;
    padding-top: 1rem;
    margin-top: 2rem;
  }

  code {
    word-break: break-all;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .titre-ligne {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
  }

  h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }

  .historique ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid #dcdce6;
    border-radius: 8px;
    overflow: hidden;
  }

  .historique li + li {
    border-top: 1px solid #ececf2;
  }

  .ligne {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    flex: 1;
    min-width: 0;
    padding: 0.5rem 0.75rem;
    border: none;
    background: #fff;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .ligne:hover {
    background: #eef4ff;
  }

  .ligne-li {
    display: flex;
    align-items: stretch;
    gap: 0;
  }

  .ligne-li + .ligne-li {
    border-top: 1px solid #ececf2;
  }

  .transcrire {
    flex: 0 0 auto;
    font: inherit;
    font-size: 0.8rem;
    padding: 0 0.9rem;
    border: none;
    border-left: 1px solid #ececf2;
    background: #fff;
    color: #2d6cdf;
    cursor: pointer;
  }

  .transcrire:hover {
    background: #eef4ff;
  }

  .nom {
    font-variant-numeric: tabular-nums;
  }

  .meta {
    color: #666;
    font-size: 0.85rem;
    white-space: nowrap;
  }

  .vide {
    color: #666;
    margin: 0;
  }

  .discret {
    font: inherit;
    font-size: 0.8rem;
    padding: 0.2rem 0.6rem;
    border-radius: 6px;
    border: 1px solid #c9c9d4;
    background: #fff;
    cursor: pointer;
  }

  kbd {
    font-family: ui-monospace, monospace;
    font-size: 0.85em;
    padding: 0.05rem 0.3rem;
    border: 1px solid #c9c9d4;
    border-radius: 4px;
    background: #fff;
  }
</style>
