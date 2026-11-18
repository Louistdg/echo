<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import MiniSearch from 'minisearch';
  import AudioLines from 'lucide-svelte/icons/audio-lines';
import Plus from 'lucide-svelte/icons/plus';
import Search from 'lucide-svelte/icons/search';
import FileText from 'lucide-svelte/icons/file-text';
import ImageIcon from 'lucide-svelte/icons/image';
import Mic from 'lucide-svelte/icons/mic';
import FolderOpen from 'lucide-svelte/icons/folder-open';
import SettingsIcon from 'lucide-svelte/icons/settings';
import ChevronRight from 'lucide-svelte/icons/chevron-right';
import Check from 'lucide-svelte/icons/check';
import Copy from 'lucide-svelte/icons/copy';
import ScanLine from 'lucide-svelte/icons/scan-line';
import MoreHorizontal from 'lucide-svelte/icons/more-horizontal';
import Pencil from 'lucide-svelte/icons/pencil';
import Trash2 from 'lucide-svelte/icons/trash-2';
import PanelLeftClose from 'lucide-svelte/icons/panel-left-close';
import PanelLeftOpen from 'lucide-svelte/icons/panel-left-open';
import AlertCircle from 'lucide-svelte/icons/alert-circle';
import X from 'lucide-svelte/icons/x';
import Square from 'lucide-svelte/icons/square';
import Play from 'lucide-svelte/icons/play';
import Paperclip from 'lucide-svelte/icons/paperclip';
import RefreshCw from 'lucide-svelte/icons/refresh-cw';
import Save from 'lucide-svelte/icons/save';
import ArrowUpRight from 'lucide-svelte/icons/arrow-up-right';
  import MarkdownEditor from '$lib/notes/MarkdownEditor.svelte';
  import ProofreadPanel from '$lib/ProofreadPanel.svelte';
  import NoteTranslation from '$lib/NoteTranslation.svelte';
  import AssetPreview from '$lib/notes/AssetPreview.svelte';
  import { SaveSession, type SaveSnapshot } from '$lib/notes/save-session';
  import type { AssetItem, NoteDocument, NoteSummary, VoiceState, WorkspaceSettings } from '$lib/notes/types';

  type Section = 'notes' | 'captures' | 'audio';
  let settings = $state<WorkspaceSettings | null>(null);
  let notes = $state<NoteSummary[]>([]);
  let captures = $state<AssetItem[]>([]);
  let recordings = $state<AssetItem[]>([]);
  let note = $state<SaveSnapshot | null>(null);
  let section = $state<Section>('notes');
  let query = $state('');
  let mediaQuery = $state('');
  let error = $state('');
  let mediaError = $state('');
  let notice = $state('');
  let loading = $state(true);
  let busy = $state(false);
  let dictationPending = $state(false);
  let showTranslation = $state(false);
  let quitAcknowledged = $state(false);
  let sidebarOpen = $state(true);
  let renaming = $state(false);
  let renameTitle = $state('');
  let voice = $state<VoiceState | null>(null);
  let noteDictating = $derived(voice?.kind === 'dictation' && (voice.microphone_open || voice.phase === 'arming'));
  const previewWanted = $derived(section === 'notes' && !!note && (dictationPending || noteDictating || (voice?.kind === 'dictation' && voice.phase === 'transcribing')));
  $effect(() => { void invoke('voice_preview_interest', { enabled: previewWanted }).catch(problem => { error = String(problem); }); });
  let editor = $state<{ focus: () => void; find: () => void; insertText: (text: string) => void } | undefined>();
  let session: SaveSession | null = null;
  let alive = true;
  let taskChain: Promise<void> = Promise.resolve();
  let taskCount = 0;
  let externalTimer: ReturnType<typeof setTimeout> | undefined;
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let assetEpoch = 0;
  let loadedWorkspace = $state('');
  let notesSearch = $derived.by(() => {
    const index = new MiniSearch<NoteSummary>({
      idField: 'path', fields: ['title', 'preview', 'path'], storeFields: ['path'],
      processTerm: term => term.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLocaleLowerCase('fr'),
      searchOptions: { prefix: true, fuzzy: 0.15, boost: { title: 3 } },
    });
    index.addAll(notes);
    return index;
  });
  let visibleNotes = $derived.by(() => {
    if (!query.trim()) return notes;
    const byPath = new Map(notes.map(item => [item.path, item]));
    return notesSearch.search(query.trim()).map(item => byPath.get(String(item.id))).filter((item): item is NoteSummary => Boolean(item));
  });
  let visibleAssets = $derived((section === 'captures' ? captures : recordings).filter(item => item.name.toLocaleLowerCase('fr').includes(mediaQuery.toLocaleLowerCase('fr'))));
  let wordCount = $derived(note?.text.trim() ? note.text.trim().split(/\s+/u).length : 0);
  let recording = $derived(voice?.kind === 'recording' && (voice.microphone_open || ['arming', 'recording', 'listening'].includes(voice.phase)));
  let voiceBusy = $derived(Boolean(voice && (voice.microphone_open || ['arming', 'listening', 'recording', 'transcribing'].includes(voice.phase))));
  let folderName = $derived(settings?.workspace.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || 'Choisir un dossier');
  let saveLabel = $derived(note?.saving ? 'Enregistrement…' : note?.error || note?.recoveryConflict ? 'Non enregistré' : note?.dirty ? 'À enregistrer' : 'Enregistré');

  function messageOf(problem: unknown) { return problem instanceof Error ? problem.message : String(problem); }
  function closeMenus() { document.querySelectorAll('.note-actions[open]').forEach(menu => menu.removeAttribute('open')); }
  function announce(text: string) {
    notice = text;
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => { notice = ''; }, 5500);
  }
  function task(work: () => Promise<void>): Promise<void> {
    taskCount++; busy = true; closeMenus();
    const next = taskChain.then(async () => { if (!alive) return; error = ''; await work(); })
      .catch(problem => { if (alive) error = messageOf(problem); });
    taskChain = next;
    return next.finally(() => { taskCount--; if (alive) busy = taskCount > 0 || quitAcknowledged; });
  }
  async function flush() { if (session) await session.flush(); }
  function useDocument(document: NoteDocument) {
    session?.dispose(); renaming = false;
    session = new SaveSession({
      document, storage: localStorage,
      save: request => invoke<NoteDocument>('note_save', { ...request }),
      changed: state => {
        note = state;
        if (!state.dirty && !state.saving) {
          notes = notes.map(item => item.path === state.document.path && state.document.workspace === loadedWorkspace ? {
            ...item, title: state.document.title, modified: state.document.modified,
            preview: state.text.replace(/^#+\s+/gm, '').replace(/<!--[\s\S]*?-->/g, '').trim().slice(0, 160),
            bytes: new TextEncoder().encode(state.text).length,
          } : item);
        }
      },
    });
  }
  async function refreshLists() {
    const workspace = settings?.workspace;
    const next = await invoke<NoteSummary[]>('notes_list');
    if (alive && workspace === settings?.workspace) notes = next;
  }
  async function refreshAssets() {
    const epoch = ++assetEpoch;
    const results = await Promise.allSettled([
      invoke<AssetItem[]>('assets_list', { kind: 'captures' }), invoke<AssetItem[]>('assets_list', { kind: 'audio' }),
    ]);
    if (!alive || epoch !== assetEpoch) return;
    mediaError = '';
    if (results[0].status === 'fulfilled') captures = results[0].value;
    else mediaError = `Les captures ne peuvent pas être lues : ${messageOf(results[0].reason)}`;
    if (results[1].status === 'fulfilled') recordings = results[1].value;
    else mediaError = `Les enregistrements ne peuvent pas être lus : ${messageOf(results[1].reason)}`;
  }
  async function loadWorkspace(next: WorkspaceSettings) {
    settings = next; loadedWorkspace = next.workspace; query = ''; mediaQuery = ''; captures = []; recordings = [];
    await refreshLists(); await refreshAssets();
    const path = notes.find(item => item.path === next.active_note)?.path ?? notes[0]?.path;
    if (path) {
      const document = await invoke<NoteDocument>('note_read', { path, workspace: next.workspace });
      await invoke('note_activate', { path: document.path, workspace: document.workspace });
      useDocument(document);
    } else { session?.dispose(); session = null; note = null; }
  }
  function openNote(path: string) {
    return task(async () => {
      await flush();
      if (!settings) throw new Error('Choisis d’abord un dossier pour tes notes.');
      const document = await invoke<NoteDocument>('note_read', { path, workspace: settings.workspace });
      await invoke('note_activate', { path: document.path, workspace: document.workspace });
      useDocument(document); section = 'notes';
      if (window.innerWidth < 760) sidebarOpen = false;
    });
  }
  function createNote() {
    return task(async () => {
      await flush();
      const document = await invoke<NoteDocument>('note_create', {});
      await invoke('note_activate', { path: document.path, workspace: document.workspace });
      useDocument(document); await refreshLists(); section = 'notes'; query = '';
      if (window.innerWidth < 760) sidebarOpen = false;
      await tick(); editor?.focus();
    });
  }
  function saveNow() { return task(async () => { await flush(); announce('Note enregistrée.'); }); }
  function changeWorkspace() {
    return task(async () => {
      await flush();
      const next = await invoke<WorkspaceSettings | null>('workspace_choose');
      if (next) { await loadWorkspace(next); announce('Le dossier de destination a été changé.'); }
    });
  }
  function changeSection(next: Section) {
    return task(async () => { await flush(); section = next; mediaQuery = ''; if (next !== 'notes') await refreshAssets(); if (window.innerWidth < 760) sidebarOpen = false; });
  }
  function copyNote() {
    return task(async () => { if (note) { await invoke('clipboard_copy', { text: note.text }); announce('Note copiée.'); } });
  }
  function openVoice() {
    return task(async () => { await flush(); if (note) await invoke('note_activate', { path: note.document.path, workspace: note.document.workspace }); await invoke('show_window', { label: 'quick' }); });
  }
  async function showSmallBase() {
    try { await invoke('show_window', { label: 'quick' }); }
    catch (problem) { error = messageOf(problem); }
  }
  async function toggleNoteDictation() {
    if (noteDictating) {
      try { voice = await invoke<VoiceState>('voice_stop'); } catch (problem) { error = messageOf(problem); }
      return;
    }
    if (busy || dictationPending || voiceBusy || !note) return;
    dictationPending = true; error = '';
    try {
      await flush();
      await invoke('note_activate', { path: note.document.path, workspace: note.document.workspace });
      editor?.focus(); await tick();
      voice = await invoke<VoiceState>('voice_start', { kind: 'dictation', mode: 'toggle' });
    } catch (problem) { error = messageOf(problem); }
    finally { dictationPending = false; }
  }
  function captureScreen() { return task(async () => { await flush(); await invoke('capture_begin'); }); }
  function renameNote() {
    return task(async () => {
      await flush(); if (!note || !renameTitle.trim()) return;
      const document = await invoke<NoteDocument>('note_rename', { path: note.document.path, title: renameTitle.trim(), workspace: note.document.workspace });
      await invoke('note_activate', { path: document.path, workspace: document.workspace });
      useDocument(document); await refreshLists(); announce('Le fichier a été renommé.');
    });
  }
  function trashNote() {
    return task(async () => {
      await flush(); if (!note || !settings) return;
      await invoke('note_trash', { path: note.document.path, workspace: note.document.workspace });
      session?.dispose(); session = null; note = null;
      await loadWorkspace(await invoke<WorkspaceSettings>('settings_get'));
      announce('Note déplacée dans la corbeille du dossier (.echo/trash).');
    });
  }
  function recoverCopy() {
    return task(async () => {
      if (!session) return;
      const oldSession = session; const state = oldSession.snapshot();
      const created = await invoke<NoteDocument>('note_create', { title: `${state.document.title} — récupération` });
      const saved = await invoke<NoteDocument>('note_save', { path: created.path, workspace: created.workspace, text: state.text, expected: created.text });
      await invoke('note_activate', { path: saved.path, workspace: saved.workspace });
      oldSession.clearRecoveredDraft();
      settings = await invoke<WorkspaceSettings>('settings_get'); loadedWorkspace = settings.workspace;
      useDocument(saved); await refreshLists(); announce('Brouillon récupéré dans une nouvelle note. Le fichier d’origine a été conservé.');
    });
  }
  function openAsset(asset: AssetItem) { return task(async () => { await flush(); await invoke(section === 'captures' ? 'capture_load' : 'asset_open', { path: asset.path }); }); }
  function attachAsset(asset: AssetItem) {
    return task(async () => {
      if (!session) throw new Error('Crée ou ouvre une note avant d’ajouter ce fichier.');
      const state = session.snapshot();
      const root = state.document.workspace.replace(/\\/g, '/').replace(/\/$/, '') + '/';
      const path = asset.path.replace(/\\/g, '/');
      if (!path.toLocaleLowerCase('fr').startsWith(root.toLocaleLowerCase('fr'))) throw new Error('Ce fichier et la note sont dans des dossiers différents. Ouvre une note du dossier courant.');
      const depth = state.document.path.replace(/\\/g, '/').split('/').length - 1;
      const relative = '../'.repeat(depth) + path.slice(root.length);
      const url = relative.split('/').map(part => encodeURIComponent(part)).join('/');
      const label = asset.name.replace(/[\[\]\\]/g, '\\$&');
      const link = `${section === 'captures' ? '!' : ''}[${label}](<${url}>)`;
      session.edit(state.text.trimEnd() + '\n\n' + link + '\n'); await session.flush();
      announce(`Fichier ajouté à « ${state.document.title} ».`);
    });
  }
  function toggleRecording() {
    return task(async () => { await flush(); voice = await invoke<VoiceState>(recording ? 'voice_stop' : 'voice_start', recording ? {} : { kind: 'recording' }); await refreshAssets(); });
  }
  function dateLabel(raw: string, full = false) {
    const date = new Date(raw); if (!Number.isFinite(date.getTime())) return '';
    const sameDay = date.toDateString() === new Date().toDateString();
    return new Intl.DateTimeFormat('fr-BE', full ? { dateStyle: 'medium', timeStyle: 'short' } : sameDay ? { hour: '2-digit', minute: '2-digit' } : { day: 'numeric', month: 'short' }).format(date);
  }
  function fileSize(bytes: number) { return bytes >= 1_000_000 ? `${(bytes / 1_000_000).toLocaleString('fr-BE', { maximumFractionDigits: 1 })} Mo` : `${Math.max(1, Math.round(bytes / 1000))} Ko`; }
  function duration(seconds: number) { const total = Math.max(0, Math.floor(seconds)); return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`; }
  function refreshExternal() {
    if (externalTimer) clearTimeout(externalTimer);
    externalTimer = setTimeout(() => {
      if (!alive || loading || busy) return;
      void (async () => {
        try {
          await Promise.all([refreshLists(), refreshAssets()]);
          const current = session;
          if (current && !current.snapshot().dirty && !current.snapshot().saving && current.snapshot().document.workspace === settings?.workspace) {
            const document = current.snapshot().document;
            const updated = await invoke<NoteDocument>('note_read', { path: document.path, workspace: document.workspace });
            if (session === current) current.acceptExternal(updated);
          }
        } catch (problem) { if (alive) error = messageOf(problem); }
      })();
    }, 220);
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') closeMenus();
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    if (event.key.toLowerCase() === 'n') { event.preventDefault(); if (!busy) void createNote(); }
    if (event.key.toLowerCase() === 's' && !event.defaultPrevented) { event.preventDefault(); if (!busy) void saveNow(); }
  }
  function outsideMenu(event: MouseEvent) { if (event.target instanceof Element && !event.target.closest('.note-actions')) closeMenus(); }
  onMount(() => {
    alive = true; if (window.innerWidth < 760) sidebarOpen = false;
    const unlisteners: UnlistenFn[] = [];
    window.addEventListener('keydown', keydown); window.addEventListener('focus', refreshExternal); window.addEventListener('click', outsideMenu);
    const subscribe = async <T,>(name: string, callback: (payload: T) => void) => {
      const unlisten = await listen<T>(name, event => { if (alive) callback(event.payload); });
      if (alive) unlisteners.push(unlisten); else unlisten();
    };
    void (async () => {
      try {
        const closeUnlisten = await getCurrentWindow().onCloseRequested(async event => {
          event.preventDefault(); await task(async () => { await flush(); await getCurrentWindow().hide(); });
        });
        if (alive) unlisteners.push(closeUnlisten); else closeUnlisten();
        await Promise.all([
          subscribe<VoiceState>('voice:state', state => {
            const wasRecording = voice?.kind === 'recording' && ['recording', 'listening'].includes(voice.phase);
            voice = state; if (wasRecording && !['recording', 'listening', 'arming'].includes(state.phase)) refreshExternal();
          }),
          subscribe<unknown>('notes:changed', refreshExternal), subscribe<unknown>('captures:changed', refreshExternal), subscribe<unknown>('assets:changed', refreshExternal), subscribe<unknown>('recordings:changed', refreshExternal),
          subscribe<unknown>('app:quit-cancelled', () => { quitAcknowledged=false; busy=taskCount>0; }),
          subscribe<unknown>('app:quit-request', () => {
            void task(async () => {
              try { await flush(); quitAcknowledged=true; await invoke('app_quit_ready'); }
              catch (problem) { quitAcknowledged=false; await invoke('app_quit_abort'); await invoke('show_window', { label: 'main' }); throw problem; }
            });
          }),
          subscribe<WorkspaceSettings>('settings:changed', next => {
            if (!settings || settings.workspace === next.workspace) { settings = next; return; }
            void task(async () => {
              settings = next; if (loadedWorkspace === next.workspace) return;
              try { await flush(); }
              catch (problem) { notes = []; captures = []; recordings = []; throw new Error(`Le dossier a changé. Le brouillon ouvert reste rattaché à son fichier d’origine. ${messageOf(problem)}`); }
              await loadWorkspace(next);
            });
          }),
        ]);
        await loadWorkspace(await invoke<WorkspaceSettings>('settings_get')); voice = await invoke<VoiceState>('voice_get');
      } catch (problem) { if (alive) error = messageOf(problem); }
      finally { if (alive) loading = false; }
    })();
    return () => {
      alive = false; session?.dispose(); if (externalTimer) clearTimeout(externalTimer); if (noticeTimer) clearTimeout(noticeTimer);
      unlisteners.forEach(unlisten => unlisten()); window.removeEventListener('keydown', keydown); window.removeEventListener('focus', refreshExternal); window.removeEventListener('click', outsideMenu);
    };
  });
</script>

<svelte:head><title>Écho — Mes notes</title></svelte:head>

<div class="workspace-shell" class:sidebar-hidden={!sidebarOpen}>
  <aside class="sidebar" aria-label="Navigation et fichiers">
    <div class="brand-row">
      <div class="brand"><AudioLines size={24} strokeWidth={1.75} /><span>Écho</span><small>V3</small></div>
      <button class="icon-btn sidebar-toggle" title="Masquer la liste" aria-label="Masquer la liste des notes" onclick={() => sidebarOpen = false}><PanelLeftClose size={17} /></button>
    </div>
    <button class="btn primary new-note" onclick={createNote} disabled={busy || loading}><Plus size={17} />Nouvelle note<kbd>Ctrl N</kbd></button>
    <nav class="sections" aria-label="Bibliothèque">
      <button class:current={section === 'notes'} aria-current={section === 'notes' ? 'page' : undefined} onclick={() => changeSection('notes')} disabled={busy}><FileText size={17} /><span>Notes</span><span class="count">{notes.length}</span></button>
      <button class:current={section === 'captures'} aria-current={section === 'captures' ? 'page' : undefined} onclick={() => changeSection('captures')} disabled={busy}><ImageIcon size={17} /><span>Captures</span><span class="count">{captures.length}</span></button>
      <button class:current={section === 'audio'} aria-current={section === 'audio' ? 'page' : undefined} onclick={() => changeSection('audio')} disabled={busy}><Mic size={17} /><span>Enregistrements</span><span class="count">{recordings.length}</span></button>
    </nav>
    <div class="notes-heading"><h2>Mes notes</h2><span>{query ? visibleNotes.length : ''}</span></div>
    <label class="note-search"><Search size={15} /><input aria-label="Rechercher une note" placeholder="Rechercher une note" bind:value={query} type="search" /></label>
    <div class="note-list" aria-label="Fichiers de notes" aria-busy={loading}>
      {#if loading}<div class="list-state">Ouverture du dossier…</div>
      {:else if visibleNotes.length === 0}<div class="list-state">{query ? 'Aucune note ne correspond.' : 'Tes nouvelles notes apparaîtront ici.'}</div>
      {:else}
        {#each visibleNotes as item (item.path)}
          <button class="note-item" class:selected={note?.document.path === item.path && note.document.workspace === loadedWorkspace} onclick={() => openNote(item.path)} disabled={busy} title={item.path} aria-current={note?.document.path === item.path ? 'true' : undefined}>
            <span class="note-item-title">{item.title || item.path}</span><span class="note-item-preview">{item.preview || 'Note vide'}</span><span class="note-item-date">{dateLabel(item.modified)}</span>
          </button>
        {/each}
      {/if}
    </div>
    <footer class="sidebar-footer">
      <div class="folder-actions">
        <button class="folder-picker" onclick={changeWorkspace} disabled={busy || loading} title={settings?.workspace || 'Choisir le dossier de destination'}>
          <FolderOpen size={17} /><span><strong>{folderName}</strong><small>Dossier de destination</small></span><ChevronRight size={14} />
        </button>
        <button class="icon-btn" title="Ouvrir le dossier dans l’Explorateur" aria-label="Ouvrir le dossier dans l’Explorateur" onclick={() => task(async () => { await invoke('workspace_open'); })} disabled={busy || !settings}><ArrowUpRight size={16} /></button>
      </div>
      <button class="settings-link" onclick={showSmallBase}><Mic size={17} />Afficher le petit socle</button>
      <button class="settings-link" onclick={() => task(async () => { await flush(); await invoke('show_window', { label: 'settings' }); })} disabled={busy}><SettingsIcon size={17} />Réglages</button>
    </footer>
  </aside>

  <main class="main-pane">
    {#if error}<div class="feedback error-feedback" role="alert"><AlertCircle size={18} /><span>{error}</span><button class="icon-btn" onclick={() => error = ''} aria-label="Masquer le message"><X size={16} /></button></div>{/if}
    {#if notice}<div class="feedback notice-feedback" role="status"><Check size={17} /><span>{notice}</span><button class="icon-btn" onclick={() => notice = ''} aria-label="Masquer le message"><X size={16} /></button></div>{/if}
    {#if section === 'notes'}
      <header class="document-header">
        {#if !sidebarOpen}<button class="icon-btn restore-sidebar" title="Afficher mes notes" aria-label="Afficher mes notes" onclick={() => sidebarOpen = true}><PanelLeftOpen size={19} /></button>{/if}
        <div class="document-heading"><h1>{note?.document.title || 'Mes notes'}</h1><span title={note ? `${note.document.workspace}\\${note.document.path}` : settings?.workspace}>{note?.document.path || 'Un espace pour tes idées'}</span></div>
        {#if note}
          <div class="save-state" class:save-error={Boolean(note.error || note.recoveryConflict)} aria-live="polite">{#if note.error || note.recoveryConflict}<AlertCircle size={14} />{:else if !note.dirty && !note.saving}<Check size={14} />{:else}<span class="save-dot" class:saving={note.saving}></span>{/if}{saveLabel}</div>
          <details class="note-actions">
            <summary class="icon-btn" aria-label="Actions de la note" title="Actions de la note"><MoreHorizontal size={20} /></summary>
            <div class="action-menu">
              <button onclick={() => { closeMenus(); renaming = true; renameTitle = note?.document.title || ''; }} disabled={busy}><Pencil size={15} />Renommer le fichier</button>
              <button onclick={saveNow} disabled={busy}><Save size={15} />Enregistrer maintenant</button>
              <button class="danger-action" onclick={trashNote} disabled={busy}><Trash2 size={15} />Mettre à la corbeille</button>
            </div>
          </details>
        {/if}
      </header>
      {#if renaming}
        <form class="rename-row" onsubmit={event => { event.preventDefault(); void renameNote(); }}>
          <label for="rename-note">Nom du fichier</label><input class="field" id="rename-note" bind:value={renameTitle} maxlength="120" autocomplete="off" disabled={busy} />
          <button class="btn primary" disabled={busy || !renameTitle.trim()} type="submit">Renommer</button><button class="btn ghost" type="button" onclick={() => renaming = false} disabled={busy}>Annuler</button>
        </form>
      {/if}
      {#if note?.error || note?.recoveryConflict || note?.draftError}
        <div class="recovery-bar" role="alert">
          <AlertCircle size={18} /><div><strong>{note.recoveryConflict ? 'Deux versions ont été conservées' : 'La note attend d’être enregistrée'}</strong><p>{note.error || (note.recoveryConflict ? 'Le fichier a changé sur disque. Ton brouillon reste ici ; récupère-le dans une nouvelle note.' : note.draftError)}</p></div>
          {#if !note.recoveryConflict}<button class="btn" onclick={saveNow} disabled={busy}>Réessayer</button>{/if}<button class="btn" onclick={recoverCopy} disabled={busy}>Récupérer dans une nouvelle note</button><button class="icon-btn" onclick={copyNote} aria-label="Copier le brouillon" title="Copier le brouillon" disabled={busy}><Copy size={16} /></button>
        </div>
      {:else if note?.recovered}<div class="recovered-hint" role="status">Ton brouillon a été récupéré. Il va être enregistré dans ce fichier.</div>{/if}
      {#if note}
        <div class="editor-toolbar" aria-label="Outils de la note">
          <button class="btn ghost" onpointerdown={event => event.preventDefault()} onclick={toggleNoteDictation} disabled={!noteDictating && (busy || dictationPending || voiceBusy)} title="Dicter directement dans cette note">{#if noteDictating}<Square size={16} />Arrêter la dictée{:else}<Mic size={16} />Dicter{/if}</button>
          <button class="btn ghost" onclick={() => showTranslation = !showTranslation} aria-expanded={showTranslation}>Traduire</button>
          <button class="btn ghost" onclick={captureScreen} disabled={busy}><ScanLine size={16} />Capturer</button><span class="toolbar-divider"></span>
          <button class="btn ghost" onclick={copyNote} disabled={busy}><Copy size={15} />Copier la note</button><button class="icon-btn find-note" onclick={() => editor?.find()} aria-label="Rechercher dans la note" title="Rechercher dans la note · Ctrl+F"><Search size={16} /></button>
        </div>
        {#if dictationPending || noteDictating || (voice?.kind === 'dictation' && voice.phase === 'transcribing')}
          <div class="note-voice-status" role="status" aria-live="polite">
            <strong>{voice?.phase === 'arming' || (dictationPending && !noteDictating) ? 'Préparation du micro…' : noteDictating ? 'Je t’écoute — clique sur Arrêter la dictée pour terminer.' : 'Transcription en cours…'}</strong>
            {#if voice?.partial}<p>{voice.partial}</p>{/if}
          </div>
        {/if}
        {#if voice?.error && voice.kind === 'dictation'}<div class="feedback error-feedback" role="alert"><span>{voice.error}{voice.text ? ' Le texte reste disponible dans les commandes vocales.' : ''}</span><button class="btn ghost" onclick={openVoice}>Afficher les commandes</button></div>{/if}
        <div class="editor-region">
          {#key `${note.document.workspace}/${note.document.path}`}<MarkdownEditor bind:this={editor} value={note.text} readOnly={busy} onChange={text => session?.edit(text)} onSave={() => { if (!busy) void saveNow(); }} />{/key}
        </div>
        {#if showTranslation}<div class="note-translation"><NoteTranslation text={note.text} sourceLanguage={settings?.language ?? 'fr'} locked={busy || dictationPending} /></div>{/if}
        <ProofreadPanel text={note.text} onReplace={(next,checked)=>{if(!note||note.text!==checked||busy)return false;session?.edit(next);return true}}/>
        <footer class="document-footer"><span title={`${note.document.workspace}\\${note.document.path}`}><FolderOpen size={13} />{note.document.path}</span><span>{wordCount.toLocaleString('fr-BE')} {wordCount > 1 ? 'mots' : 'mot'}<span class="footer-separator">·</span>Markdown</span></footer>
      {:else}
        <div class="empty-workspace"><FileText size={38} strokeWidth={1.25} /><h2>{loading ? 'Ouverture des notes…' : 'Une idée commence ici'}</h2><p>{loading ? 'Écho retrouve les fichiers de ton dossier.' : 'Écris, dicte ou rassemble tes captures dans des notes enregistrées dans le dossier de ton choix.'}</p>
          {#if !loading}<button class="btn primary" onclick={createNote} disabled={busy}><Plus size={17} />Créer une note</button><button class="btn ghost" onclick={changeWorkspace} disabled={busy}><FolderOpen size={16} />Ouvrir un autre dossier</button>{/if}
        </div>
      {/if}
    {:else}
      <header class="library-header">
        <div class="library-heading">{#if !sidebarOpen}<button class="icon-btn restore-sidebar" title="Afficher mes notes" aria-label="Afficher mes notes" onclick={() => sidebarOpen = true}><PanelLeftOpen size={19} /></button>{/if}<div><h1>{section === 'captures' ? 'Captures' : 'Enregistrements'}</h1><p>{section === 'captures' ? 'Retrouve et retouche tes images.' : 'Garde une voix, une idée, une conversation.'}</p></div></div>
        {#if section === 'captures'}<button class="btn primary" onclick={captureScreen} disabled={busy}><ScanLine size={17} />Nouvelle capture</button>
        {:else}<button class="btn" class:primary={!recording} class:record-stop={recording} onclick={toggleRecording} disabled={busy || (!recording && voiceBusy)}>{#if recording}<Square size={15} fill="currentColor" />Arrêter {duration(voice?.duration_secs || 0)}{:else}<Mic size={17} />Enregistrer{/if}</button>{/if}
      </header>
      <div class="library-tools"><label class="media-search"><Search size={16} /><input aria-label={section === 'captures' ? 'Rechercher une capture' : 'Rechercher un enregistrement'} type="search" placeholder="Rechercher un fichier" bind:value={mediaQuery} /></label><span class="library-count">{visibleAssets.length} {visibleAssets.length > 1 ? 'fichiers' : 'fichier'}</span><button class="icon-btn" onclick={() => task(refreshAssets)} title="Actualiser les fichiers" aria-label="Actualiser les fichiers" disabled={busy}><RefreshCw size={16} /></button></div>
      {#if mediaError}<div class="feedback error-feedback" role="alert"><AlertCircle size={17} /><span>{mediaError}</span></div>{/if}
      {#if recording}<div class="recording-banner" role="status"><span class="recording-dot"></span><strong>Le micro enregistre</strong><span>{duration(voice?.duration_secs || 0)}</span><button class="btn ghost" onclick={openVoice} disabled={busy}>Afficher les commandes<ArrowUpRight size={14} /></button></div>{/if}
      <div class="library-content">
        {#if visibleAssets.length === 0}
          <div class="empty-workspace media-empty">{#if section === 'captures'}<ImageIcon size={38} strokeWidth={1.25} />{:else}<Mic size={38} strokeWidth={1.25} />{/if}<h2>{mediaQuery ? 'Aucun fichier trouvé' : section === 'captures' ? 'Tes captures, prêtes à annoter' : 'Tes enregistrements restent à portée de main'}</h2><p>{mediaQuery ? 'Essaie un autre nom de fichier.' : section === 'captures' ? 'Sélectionne une zone de l’écran. L’éditeur s’ouvre pour ajouter du texte, une flèche ou un recadrage.' : 'Lance un enregistrement, puis arrête-le pour le retrouver dans ce dossier.'}</p>{#if !mediaQuery && section === 'captures'}<button class="btn primary" onclick={captureScreen} disabled={busy}><ScanLine size={17} />Faire une capture</button>{/if}</div>
        {:else if section === 'captures'}
          <div class="asset-grid">
            {#each visibleAssets as asset (asset.path)}
              <article class="capture-item"><button class="preview-button" onclick={() => openAsset(asset)} title={`Retoucher ${asset.name}`} disabled={busy}><AssetPreview path={asset.path} name={asset.name} /></button>
                <div class="capture-caption"><h2 title={asset.name}>{asset.name}</h2><p>{dateLabel(asset.modified, true)}<span>·</span>{fileSize(asset.bytes)}</p><div class="capture-actions"><button class="btn ghost" onclick={() => openAsset(asset)} disabled={busy}><Pencil size={14} />Retoucher</button><button class="icon-btn" onclick={() => attachAsset(asset)} disabled={busy || !note} title={note ? `Ajouter à « ${note.document.title} »` : 'Ouvre une note pour y ajouter cette capture'} aria-label={`Ajouter ${asset.name} à la note`}><Paperclip size={16} /></button></div></div>
              </article>
            {/each}
          </div>
        {:else}
          <div class="audio-list">
            {#each visibleAssets as asset (asset.path)}
              <article class="audio-item"><AudioLines class="audio-symbol" size={23} strokeWidth={1.5} /><div class="audio-detail"><h2 title={asset.name}>{asset.name}</h2><p>{dateLabel(asset.modified, true)}<span>·</span>{fileSize(asset.bytes)}</p></div><button class="btn" onclick={() => openAsset(asset)} disabled={busy}><Play size={15} />Écouter</button><button class="icon-btn" onclick={() => attachAsset(asset)} disabled={busy || !note} title={note ? `Ajouter à « ${note.document.title} »` : 'Ouvre une note pour y ajouter ce fichier'} aria-label={`Ajouter ${asset.name} à la note`}><Paperclip size={16} /></button></article>
            {/each}
          </div>
        {/if}
      </div>
      <footer class="library-footer"><FolderOpen size={13} /><span title={settings?.workspace}>{settings?.workspace}{section === 'captures' ? '\\captures' : '\\.echo\\audio'}</span></footer>
    {/if}
  </main>
</div>

<style>
  .note-voice-status { margin: 8px 24px; padding: 12px 16px; border: 1px solid var(--border); border-radius: 12px; font-size: 13px; }
  .note-voice-status p { margin: 8px 0 0; white-space: pre-wrap; }
  .note-translation { flex: 0 1 auto; max-height: 45%; overflow: auto; padding: 12px 24px; border-top: 1px solid var(--border); }
  .workspace-shell { display: grid; grid-template-columns: 236px minmax(0, 1fr); height: 100vh; overflow: hidden; background: var(--surface); color: var(--text); font-family: 'Segoe UI Variable', 'Segoe UI', sans-serif; }
  .workspace-shell.sidebar-hidden { grid-template-columns: minmax(0, 1fr); }
  .sidebar-hidden .sidebar { display: none; }
  .sidebar { display: flex; min-height: 0; flex-direction: column; background: var(--sidebar); border-right: 1px solid var(--border); padding: 20px 12px 12px; }
  .brand-row { display: flex; align-items: center; justify-content: space-between; padding: 0 8px 23px; }
  .brand { display: flex; align-items: center; gap: 8px; }
  .brand :global(svg) { color: var(--accent); }
  .brand > span { font-size: 23px; font-weight: 660; letter-spacing: -.025em; }
  .brand small { margin-left: 2px; font-size: 10px; font-weight: 600; color: var(--muted); align-self: center; }
  .sidebar-toggle { opacity: .75; }
  .new-note { width: 100%; justify-content: flex-start; min-height: 39px; gap: 8px; font-size: 13px; }
  kbd { margin-left: auto; font-family: inherit; font-size: 10px; opacity: .85; font-weight: 400; white-space: nowrap; }
  .sections { display: grid; gap: 3px; margin-top: 19px; padding-bottom: 20px; border-bottom: 1px solid var(--border); }
  .sections button { display: flex; align-items: center; width: 100%; gap: 10px; padding: 10px 11px; text-align: left; font-size: 13px; font-weight: 540; color: var(--muted); border: 0; border-radius: 7px; background: transparent; cursor: pointer; }
  .sections button:hover { color: var(--text); background: var(--bg); }
  .sections button.current { color: var(--accent); background: var(--accent-soft); font-weight: 650; }
  .count { margin-left: auto; font-size: 11px; font-variant-numeric: tabular-nums; }
  .notes-heading { display: flex; align-items: center; justify-content: space-between; padding: 22px 10px 12px; }
  .notes-heading h2 { font-size: 12px; font-weight: 650; margin: 0; }
  .notes-heading > span { font-size: 11px; color: var(--muted); }
  .note-search { display: flex; align-items: center; gap: 7px; background: var(--surface); border: 1px solid var(--border); border-radius: 7px; min-height: 34px; padding: 0 9px; margin-bottom: 8px; color: var(--muted); }
  .note-search input, .media-search input { width: 100%; min-width: 0; padding: 6px 0; border: 0; outline: 0; background: transparent; color: var(--text); font: inherit; font-size: 12px; }
  input::placeholder { color: var(--muted); opacity: 1; }
  .note-search:focus-within, .media-search:focus-within { border-color: var(--accent); outline: 2px solid var(--accent-soft); }
  .note-list { flex: 1; min-height: 70px; overflow: auto; padding-top: 2px; scrollbar-color: var(--border) transparent; }
  .note-item { display: grid; width: 100%; gap: 6px; padding: 12px 10px; border: 1px solid transparent; border-radius: 7px; margin-bottom: 4px; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
  .note-item:hover { background: var(--bg); }
  .note-item.selected { background: var(--surface); border-color: var(--border); }
  .note-item-title { font-size: 13px; font-weight: 620; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note-item-preview { font-size: 11px; color: var(--muted); line-height: 1.5; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .note-item-date { color: var(--muted); font-size: 10px; font-variant-numeric: tabular-nums; }
  .list-state { font-size: 12px; color: var(--muted); padding: 15px 9px; line-height: 1.6; }
  .sidebar-footer { margin-top: 12px; border-top: 1px solid var(--border); padding-top: 10px; }
  .folder-actions { display: flex; align-items: center; gap: 1px; }
  .folder-picker { flex: 1; min-width: 0; display: flex; align-items: center; gap: 9px; padding: 9px 8px; background: transparent; border: 0; color: var(--text); text-align: left; border-radius: 6px; cursor: pointer; }
  .folder-picker:hover, .settings-link:hover { background: var(--bg); }
  .folder-picker > span { flex: 1; min-width: 0; display: grid; gap: 3px; }
  .folder-picker strong { font-size: 12px; font-weight: 600; white-space: nowrap; text-overflow: ellipsis; overflow: hidden; }
  .folder-picker small { color: var(--muted); font-size: 10px; white-space: nowrap; }
  .settings-link { display: flex; width: 100%; align-items: center; gap: 10px; padding: 10px 8px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); font-size: 12px; cursor: pointer; }
  .main-pane { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .document-header { min-height: 83px; display: flex; align-items: center; padding: 18px 28px 16px; gap: 16px; border-bottom: 1px solid var(--border); }
  .document-heading { min-width: 0; flex: 1; }
  h1 { font-size: 21px; line-height: 1.3; font-weight: 640; letter-spacing: -.018em; margin: 0; }
  .document-heading h1 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .document-heading > span { display: block; margin-top: 5px; color: var(--muted); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .save-state { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 11px; white-space: nowrap; }
  .save-state.save-error { color: var(--danger); }
  .save-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--muted); }
  .save-dot.saving { background: var(--accent); }
  .note-actions { position: relative; }
  .note-actions summary { list-style: none; }
  .note-actions summary::-webkit-details-marker { display: none; }
  .action-menu { position: absolute; top: calc(100% + 8px); right: 0; width: 220px; padding: 5px; background: var(--surface); box-shadow: 0 8px 30px rgb(0 0 0 / .15); border-radius: 8px; z-index: 20; }
  .action-menu button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 10px; background: transparent; border: 0; border-radius: 5px; text-align: left; color: var(--text); font-size: 12px; cursor: pointer; }
  .action-menu button:hover { background: var(--bg); }
  .action-menu .danger-action { color: var(--danger); }
  .editor-toolbar { display: flex; align-items: center; gap: 5px; min-height: 53px; padding: 8px 24px; border-bottom: 1px solid var(--border); }
  .editor-toolbar .btn { font-size: 12px; min-height: 31px; padding: 6px 10px; }
  .toolbar-divider { height: 17px; width: 1px; background: var(--border); margin: 0 6px; }
  .find-note { margin-left: auto; }
  .editor-region { flex: 1; min-height: 0; min-width: 0; }
  .document-footer { display: flex; align-items: center; justify-content: space-between; gap: 20px; min-height: 33px; padding: 7px 27px; color: var(--muted); font-size: 10px; border-top: 1px solid var(--border); }
  .document-footer > span { display: flex; align-items: center; gap: 6px; white-space: nowrap; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .document-footer > span:last-child { flex-shrink: 0; }
  .footer-separator { padding: 0 4px; }
  .feedback { display: flex; flex-shrink: 0; align-items: center; gap: 10px; padding: 11px 22px; font-size: 12px; line-height: 1.5; border-bottom: 1px solid var(--border); }
  .feedback > span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .feedback > :global(svg) { flex-shrink: 0; }
  .error-feedback { color: var(--danger); background: var(--surface); }
  .notice-feedback { color: var(--text); background: var(--accent-soft); }
  .notice-feedback > :global(svg) { color: var(--accent); }
  .recovery-bar { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; padding: 15px 24px; background: var(--sidebar); border-bottom: 1px solid var(--border); }
  .recovery-bar > :global(svg) { flex-shrink: 0; color: var(--danger); }
  .recovery-bar > div { flex: 1; min-width: 210px; }
  .recovery-bar strong { font-size: 12px; }
  .recovery-bar p { font-size: 12px; color: var(--muted); line-height: 1.5; margin: 3px 0 0; overflow-wrap: anywhere; }
  .recovery-bar .btn { font-size: 11px; }
  .recovered-hint { padding: 10px 28px; font-size: 12px; color: var(--muted); background: var(--sidebar); }
  .rename-row { display: flex; align-items: center; gap: 8px; padding: 13px 25px; background: var(--sidebar); border-bottom: 1px solid var(--border); }
  .rename-row label { font-size: 12px; white-space: nowrap; }
  .rename-row input { flex: 1; min-width: 80px; }
  .rename-row .btn { font-size: 12px; }
  .empty-workspace { flex: 1; display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 14px; text-align: center; padding: 40px; }
  .empty-workspace > :global(svg) { color: var(--muted); margin-bottom: 7px; }
  .empty-workspace h2 { font-size: 22px; font-weight: 600; letter-spacing: -.02em; margin: 0; }
  .empty-workspace p { max-width: 43ch; font-size: 14px; line-height: 1.7; margin: 0 0 10px; color: var(--muted); }
  .library-header { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 18px; padding: 28px 32px 24px; }
  .library-heading { display: flex; align-items: center; gap: 14px; }
  .library-header h1 { font-size: 26px; }
  .library-header p { margin: 7px 0 0; font-size: 13px; color: var(--muted); }
  .library-header .btn { font-size: 12px; }
  .library-tools { display: flex; align-items: center; gap: 14px; padding: 0 32px 20px; border-bottom: 1px solid var(--border); }
  .media-search { display: flex; align-items: center; gap: 8px; padding: 3px 10px; background: var(--sidebar); border: 1px solid var(--border); border-radius: 7px; color: var(--muted); max-width: 290px; flex: 1; }
  .library-count { margin-left: auto; color: var(--muted); font-size: 11px; white-space: nowrap; }
  .library-content { flex: 1; min-height: 0; overflow: auto; padding: 25px 32px; scrollbar-color: var(--border) transparent; }
  .media-empty { min-height: 320px; height: 100%; }
  .asset-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(225px, 1fr)); gap: 22px; align-items: start; }
  .capture-item { border: 1px solid var(--border); border-radius: 10px; overflow: hidden; background: var(--surface); min-width: 0; }
  .preview-button { display: block; padding: 0; width: 100%; border: 0; background: transparent; cursor: pointer; }
  .preview-button:hover { opacity: .9; }
  .capture-caption { padding: 13px 13px 8px; }
  .capture-caption h2, .audio-detail h2 { font-size: 12px; font-weight: 600; margin: 0; white-space: nowrap; text-overflow: ellipsis; overflow: hidden; }
  .capture-caption p, .audio-detail p { display: flex; align-items: center; gap: 7px; margin: 7px 0 0; font-size: 10px; color: var(--muted); line-height: 1.6; }
  .capture-actions { display: flex; align-items: center; justify-content: space-between; margin-top: 12px; }
  .capture-actions .btn { font-size: 11px; padding: 5px 7px; min-height: 29px; }
  .audio-list { display: grid; }
  .audio-item { display: flex; align-items: center; gap: 16px; padding: 18px 2px; border-bottom: 1px solid var(--border); min-width: 0; }
  .audio-item:first-child { padding-top: 4px; }
  .audio-item :global(.audio-symbol) { color: var(--accent); flex-shrink: 0; }
  .audio-detail { flex: 1; min-width: 0; }
  .audio-detail h2 { font-size: 13px; }
  .audio-item .btn { font-size: 12px; }
  .library-footer { display: flex; align-items: center; gap: 7px; padding: 10px 32px; min-height: 34px; color: var(--muted); font-size: 10px; border-top: 1px solid var(--border); }
  .library-footer > span { text-overflow: ellipsis; overflow: hidden; white-space: nowrap; }
  .recording-banner { display: flex; align-items: center; gap: 10px; padding: 12px 32px; font-size: 12px; background: var(--sidebar); }
  .recording-banner strong { font-weight: 600; }
  .recording-banner > span:last-of-type { font-variant-numeric: tabular-nums; color: var(--muted); }
  .recording-banner .btn { margin-left: auto; font-size: 11px; }
  .recording-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--danger); }
  .record-stop { color: var(--danger); border-color: var(--danger); }
  button:disabled, input:disabled { opacity: .48; cursor: default; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 900px) {
    .document-header { padding: 17px 20px; gap: 10px; }
    .save-state { font-size: 10px; }
    .editor-toolbar { padding-left: 16px; padding-right: 16px; }
    .library-header { padding: 24px; }
    .library-tools { padding: 0 24px 18px; }
    .library-content { padding: 22px 24px; }
  }
  @media (max-width: 760px) {
    .workspace-shell { grid-template-columns: 210px minmax(0, 1fr); }
    .sidebar { padding-inline: 10px; }
    .new-note kbd { display: none; }
    .document-heading h1 { font-size: 18px; }
    .document-header { min-height: 73px; padding: 15px 17px; }
    .document-heading > span { font-size: 10px; }
    .save-state { display: flex; font-size: 10px; }
    .editor-toolbar { gap: 2px; }
    .editor-toolbar .btn { padding: 6px 8px; }
    .toolbar-divider { margin: 0 3px; }
    .document-footer { padding-inline: 20px; }
    .rename-row { flex-wrap: wrap; padding: 12px 18px; }
    .rename-row label { width: 100%; }
    .rename-row input { flex-basis: 100%; }
    .library-header { padding: 22px; }
    .library-header h1 { font-size: 23px; }
    .library-tools { padding: 0 22px 18px; }
    .library-content { padding: 22px; }
    .library-footer { padding-inline: 22px; }
    .audio-item { gap: 10px; }
    .audio-item :global(.audio-symbol) { display: none; }
    .audio-item .btn { font-size: 11px; padding-inline: 8px; }
    .empty-workspace { padding: 30px 23px; }
    .empty-workspace h2 { font-size: 20px; }
  }
</style>
