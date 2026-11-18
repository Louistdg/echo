<script lang="ts">
import {onMount} from 'svelte';
import Plus from 'lucide-svelte/icons/plus';
import Languages from 'lucide-svelte/icons/languages';
import Save from 'lucide-svelte/icons/save';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import MarkdownEditor from '$lib/notes/MarkdownEditor.svelte';
import ProofreadPanel from '$lib/ProofreadPanel.svelte';
import NoteTranslation from '$lib/NoteTranslation.svelte';
import {SaveSession,type SaveSnapshot} from '$lib/notes/save-session';
import {dockDraftStorage} from '$lib/dock-notes';
import type {NoteDocument,NoteSummary} from '$lib/notes/types';
import type {Settings} from '$lib/api';
let {locked=false}:{locked?:boolean}=$props();
let cfg=$state<Settings|null>(null);let notes=$state<NoteSummary[]>([]);let note=$state<SaveSnapshot|null>(null);let error=$state('');let busy=$state(false);let alive=true;let session:SaveSession|null=null;let chain:Promise<void>=Promise.resolve();let refreshTimer:ReturnType<typeof setTimeout>|undefined;
const storage=dockDraftStorage(localStorage);
let translating=$state(false);let filter=$state('');
const saveLabel=$derived(note?.saving?'Enregistrement…':note?.error||note?.recoveryConflict?'À reprendre':note?.dirty?'Modifié':'Enregistré');
const visibleNotes=$derived(notes.filter(n=>{const q=filter.trim().toLocaleLowerCase('fr');return !q||n.path===note?.document.path||n.title.toLocaleLowerCase('fr').includes(q)||n.path.toLocaleLowerCase('fr').includes(q)}));
function use(document:NoteDocument){session?.dispose();session=new SaveSession({document,storage,save:r=>invoke<NoteDocument>('note_save',{...r}),changed:state=>{if(alive)note=state}})}
function task(work:()=>Promise<void>):Promise<void>{const result=chain.then(async()=>{busy=true;error='';try{await work()}catch(e){error=String(e);throw e}finally{busy=false}});chain=result.catch(()=>{});return result}
export async function flush(){await chain;if(session)await session.flush()}
async function list(){notes=await invoke<NoteSummary[]>('notes_list')}
async function select(path:string){await session?.flush();if(!cfg)return;const d=await invoke<NoteDocument>('note_read',{path,workspace:cfg.workspace});await invoke('note_activate',{path:d.path,workspace:d.workspace});use(d)}
function choose(path:string){void task(()=>select(path)).catch(()=>{})}
function create(){void task(async()=>{await session?.flush();const d=await invoke<NoteDocument>('note_create',{});use(d);await invoke('note_activate',{path:d.path,workspace:d.workspace});await list()}).catch(()=>{})}
function save(){void task(async()=>{await session?.flush()}).catch(()=>{})}
async function recover(){await task(async()=>{if(!session)return;const old=session;const state=old.snapshot();const created=await invoke<NoteDocument>('note_create',{title:state.document.title+' — récupération'});const saved=await invoke<NoteDocument>('note_save',{path:created.path,workspace:created.workspace,text:state.text,expected:created.text});old.clearRecoveredDraft();use(saved);await invoke('note_activate',{path:saved.path,workspace:saved.workspace});await list()})}
async function replace(next:string,checked:string){if(!session||session.snapshot().text!==checked)return false;session.edit(next);await session.flush();return true}
function refresh(){if(refreshTimer)clearTimeout(refreshTimer);refreshTimer=setTimeout(()=>{void task(async()=>{await list();const current=session;if(current&&!current.snapshot().dirty&&!current.snapshot().saving){const d=current.snapshot().document;const fresh=await invoke<NoteDocument>('note_read',{path:d.path,workspace:d.workspace});if(session===current)current.acceptExternal(fresh)}}).catch(()=>{})},220)}
onMount(()=>{alive=true;const offs:(()=>void)[]=[];const keep=(off:()=>void)=>alive?offs.push(off):off();
 void task(async()=>{cfg=await invoke<Settings>('settings_get');await list();const path=notes.find(n=>n.path===cfg?.active_note)?.path??notes[0]?.path;if(path)await select(path)}).catch(()=>{});
 void listen('notes:changed',refresh).then(keep);
 void listen<Settings>('settings:changed',e=>{const next=e.payload;if(!cfg||cfg.workspace===next.workspace){cfg=next;return}void task(async()=>{await session?.flush();cfg=next;await list();session?.dispose();session=null;note=null;const path=notes.find(n=>n.path===next.active_note)?.path??notes[0]?.path;if(path)await select(path)}).catch(()=>{})}).then(keep);
 return()=>{alive=false;offs.forEach(f=>f());if(refreshTimer)clearTimeout(refreshTimer);session?.dispose()};
});
</script>
<div class="dock-note">
<div class="note-toolbar"><label class="sr-only" for="dock-note-search">Chercher une note</label><input id="dock-note-search" class="note-search" type="search" bind:value={filter} placeholder="Chercher une note…" disabled={busy||locked}/><label class="sr-only" for="dock-note-select">Note du bloc-notes</label><select id="dock-note-select" disabled={busy||locked} value={note?.document.path||''} onchange={e=>choose(e.currentTarget.value)}>{#if !visibleNotes.length}<option value="">{notes.length?'Aucune note trouvée':'Aucune note'}</option>{/if}{#each visibleNotes as n}<option value={n.path}>{n.title}</option>{/each}</select><button class="btn" disabled={busy||locked} onclick={create}><Plus size={15} aria-hidden="true"/>Nouvelle</button></div>
{#if error||note?.error||note?.draftError||note?.recoveryConflict}<div class="feedback" role="alert">{error||note?.error||note?.draftError||(note?.recoveryConflict?'Le fichier a changé ailleurs. Ton brouillon est conservé.':'')}<button class="btn" onclick={()=>void recover().catch(e=>error=String(e))}>Garder une copie</button></div>{/if}
{#if note}<div class="editor"><MarkdownEditor value={note.text} readOnly={busy||locked} onChange={text=>session?.edit(text)} onSave={save}/></div><div class="note-footer"><span class="save-status" aria-live="polite" title={saveLabel}>{saveLabel}</span><button class="btn ghost" aria-expanded={translating} onclick={()=>translating=!translating} disabled={busy||locked}><Languages size={15} aria-hidden="true"/>Traduire</button><button class="btn ghost save-note" aria-label="Enregistrer la note" title="Enregistrer la note (Ctrl+S)" onclick={save} disabled={busy||locked}><Save size={16} aria-hidden="true"/><span class="save-label">Enregistrer</span></button></div>{#if translating}<div class="translation-area"><NoteTranslation text={note.text} sourceLanguage={cfg?.language} locked={busy||locked}/></div>{/if}<ProofreadPanel compact text={note.text} onReplace={replace}/>{:else}<p class="empty">Ouvre ou crée une note pour écrire ici.</p>{/if}
</div>
<style>
.dock-note{display:flex;flex-direction:column;height:100%;min-height:0;overflow:auto;font-size:13px;container-type:inline-size}
.note-toolbar{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:center;gap:6px;padding:6px 8px;flex:none;background:var(--sidebar)}
.note-search{grid-column:1/-1;min-width:0;min-height:34px;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:7px;padding:5px 8px;font-size:12px}
.note-toolbar select{min-width:0;min-height:36px;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:7px;padding:5px 8px;font-size:12px;text-overflow:ellipsis}
.btn{font-size:12px;min-height:36px;padding:5px 8px;flex:none}
.editor{flex:1;min-height:80px;overflow:hidden}
.editor :global(.cm-editor){font-size:14px!important}
.editor :global(.cm-content){padding:12px 14px 28px!important}
.editor :global(.cm-scroller){line-height:1.7!important}
.editor :global(.cm-focused){box-shadow:inset 2px 0 var(--accent)}
.note-footer{display:flex;align-items:center;flex:none;min-height:38px;padding:0 8px;gap:4px;border-top:1px solid var(--border);color:var(--muted);font-size:11px}
.save-status{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.save-note{min-width:36px;padding:5px}.save-label{display:none}
@container(min-width:400px){.save-label{display:inline}.save-note{padding:5px 8px}.note-footer{gap:8px}}
.translation-area{flex:none;max-height:55%;overflow:auto;min-height:0}
.feedback{padding:8px;color:var(--danger);background:var(--danger-soft);line-height:1.5;flex:none;max-height:140px;overflow:auto}.feedback .btn{margin-top:6px}
.empty{padding:14px;color:var(--muted);line-height:1.6}
</style>
