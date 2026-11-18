<script lang="ts">
import {onMount} from 'svelte';
import {invoke} from '@tauri-apps/api/core';
import {listen,emit} from '@tauri-apps/api/event';
import {getCurrentWindow} from '@tauri-apps/api/window';
import {getAllWebviewWindows} from '@tauri-apps/api/webviewWindow';
import Mic from 'lucide-svelte/icons/mic';
import Square from 'lucide-svelte/icons/square';
import Plus from 'lucide-svelte/icons/plus';
import ScanLine from 'lucide-svelte/icons/scan-line';
import NotebookPen from 'lucide-svelte/icons/notebook-pen';
import SettingsIcon from 'lucide-svelte/icons/settings';
import GripVertical from 'lucide-svelte/icons/grip-vertical';
import {emptyVoice,type Settings,type VoiceState} from '$lib/api';
import {loadLayout} from '$lib/dock';
let cfg=$state<Settings|null>(null);let voice=$state<VoiceState>({...emptyVoice});let pending=$state(false);let error=$state('');let alive=true;let hidden=false;
const active=$derived(voice.microphone_open||['arming','listening','recording'].includes(voice.phase));
const retained=$derived((voice.phase==='result'||voice.phase==='error')&&!!(voice.audio_path||voice.text||voice.queued));
const label=$derived(active?'Arrêter':voice.phase==='transcribing'?'Finalisation en cours':'Démarrer la dictée');
async function open(action:string,focus=true){try{hidden=false;const l=loadLayout(localStorage);await invoke('dock_sync',{request:{width:l.width,height:260,visible:true,focus,action}})}catch(e){error=String(e)}}
function received(next:VoiceState){voice=next}
async function run(command:string,args?:Record<string,unknown>){pending=true;error='';try{const v=await invoke<VoiceState>(command,args);if(v?.phase)received(v)}catch(e){error=String(e);await emit('app:notice',error)}finally{pending=false}}
async function click(){if(pending&&!active)return;hidden=false;if(active){void run('voice_stop');return}await run('voice_start',{kind:'dictation',mode:'toggle'});if(!error)void open('live-auto',false)}
async function captureScreen(){if(active||pending)return;try{await invoke('capture_begin')}catch(e){error=String(e);await emit('app:notice',error)}}
onMount(()=>{alive=true;const offs:(()=>void)[]=[];const keep=(off:()=>void)=>alive?offs.push(off):off();
 void Promise.all([invoke<Settings>('settings_get'),invoke<VoiceState>('voice_get')]).then(([s,v])=>{cfg=s;voice=v;if(v.engine!=='ready')void invoke<VoiceState>('voice_preload').then(received).catch(e=>error=String(e))}).catch(e=>error=String(e));
 void listen<Settings>('settings:changed',e=>cfg=e.payload).then(keep);
 void listen<VoiceState>('voice:state',e=>received(e.payload)).then(keep);
 void listen('dock:hidden',()=>{hidden=true;error=''}).then(keep);
 void listen<string>('app:notice',e=>error=e.payload).then(keep);
 void listen('dock:hide-fallback',()=>{hidden=true;void invoke('dock_hide')}).then(keep);
 void getCurrentWindow().onFocusChanged(e=>{if(e.payload)hidden=false}).then(keep);
 void getCurrentWindow().onCloseRequested(async e=>{e.preventDefault();hidden=true;await requestHide()}).then(keep);
 return()=>{alive=false;offs.forEach(f=>f())};
});
async function requestHide(){try{const windows=await getAllWebviewWindows();if(windows.some(w=>w.label==='dock-panels'))await emit('dock:request-hide');else await invoke('dock_hide')}catch(e){error=String(e)}}
</script>
<svelte:head><title>Écho V3 — Socle</title></svelte:head>
<div class="base" aria-label="Socle Écho">
<button class="grip" aria-label="Déplacer le socle et ses blocs" title="Déplacer" onpointerdown={e=>{if(e.button===0)void getCurrentWindow().startDragging()}}><GripVertical size={14}/></button>
<button class="micro" class:recording={active} class:working={voice.phase==='transcribing'} disabled={!cfg||(pending&&!active)||(voice.phase==='transcribing'&&!active)} aria-label={label} title={label} onpointerdown={e=>e.preventDefault()} onclick={click}>{#if active}<Square size={19} fill="currentColor"/>{:else}<Mic size={21}/>{/if}</button>
<button class="tool" aria-label="Capturer l’écran" title="Capturer l’écran" disabled={active||pending} onclick={captureScreen}><ScanLine size={20}/></button>
<button class="tool" aria-label="Ouvrir le bloc-notes" title="Ouvrir le bloc-notes" onclick={()=>open('notes')}><NotebookPen size={20}/></button>
<button class="tool" aria-label={retained?'Ajouter un outil · résultat à reprendre':'Ajouter un outil'} title="Ajouter un outil" onclick={()=>open('add')}><Plus size={20}/>{#if retained||error}<span class="badge" aria-label="Résultat à reprendre"></span>{/if}</button>
<button class="tool settings" aria-label="Ouvrir les réglages" title="Réglages" onclick={()=>invoke('show_window',{label:'settings'})}><SettingsIcon size={19}/></button>
<span class="sr-only" aria-live="polite">{active?'Microphone ouvert':voice.phase==='transcribing'?'Transcription en cours':error?'Action à reprendre':'Microphone fermé'}</span>
</div>
<style>
.base{height:56px;margin:4px;width:calc(100vw - 8px);display:flex;align-items:center;gap:2px;padding:5px 3px;background:var(--surface);border:1px solid var(--border);border-radius:16px;box-shadow:0 2px 10px #0002;overflow:hidden}
button{flex:none;display:grid;place-items:center;border:0;background:transparent;color:var(--muted);height:44px;border-radius:10px;padding:0}
button:hover:not(:disabled){color:var(--text);background:var(--hover)}button:focus-visible{outline:2px solid var(--accent);outline-offset:-3px}
.grip{width:18px;cursor:grab}.micro{width:44px;background:var(--accent);color:var(--on-accent)}.micro:hover:not(:disabled){background:var(--accent-hover);color:var(--on-accent)}
.micro.recording,.micro.recording:hover{background:var(--danger);color:var(--on-danger)}.micro:focus-visible{outline-color:var(--on-accent);outline-offset:-3px}.micro.recording:focus-visible{outline-color:var(--on-danger)}.micro.working{box-shadow:inset 0 0 0 2px var(--accent);background:var(--accent-soft);color:var(--accent)}
.tool{width:38px;position:relative}.badge{position:absolute;right:3px;top:6px;width:7px;height:7px;border-radius:50%;background:var(--danger)}
</style>
