<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  let notesDir = $state<string | null>(null);
  let texte = $state("");
  let message = $state("");
  let enErreur = $state(false);
  let statut = $state<{
    capture_audio: string;
    transcription: string;
    notes: string;
  } | null>(null);

  onMount(async () => {
    try {
      const settings = await invoke<{ notes_dir: string | null }>("get_settings");
      notesDir = settings.notes_dir ?? null;
      statut = await invoke("get_status");
    } catch (e) {
      message = `Erreur : ${String(e)}`;
    }
  });

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
    try {
      // L'UI ne dit « Enregistré » que si le cœur a écrit le journal ET la note.
      const r = await invoke<{ id: string; path: string; state: string }>(
        "append_to_today_note",
        { text: texte },
      );
      message = `${r.state === "enregistré" ? "Enregistré" : r.state} — ${r.path}`;
      enErreur = false;
      texte = "";
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
    <p class="sous-titre">Bloc-notes vocal local — squelette initial</p>
  </header>

  <section class="dossier">
    {#if notesDir}
      <p>Dossier de notes : <code>{notesDir}</code></p>
    {:else}
      <p>Aucun dossier de notes configuré.</p>
    {/if}
    <button onclick={choisirDossier}>Choisir le dossier…</button>
  </section>

  <section class="editeur">
    <textarea
      bind:value={texte}
      placeholder="Écris ici… (la dictée vocale arrive en v1 — module capture)"
      rows="10"
    ></textarea>
    <div class="actions">
      <button class="principal" onclick={ajouterALaNote}>Ajouter à la note du jour</button>
    </div>
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
</style>
