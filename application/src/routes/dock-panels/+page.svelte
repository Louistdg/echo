<script lang="ts">
import {onMount,tick} from 'svelte';
import Plus from 'lucide-svelte/icons/plus';
import Minus from 'lucide-svelte/icons/minus';
import X from 'lucide-svelte/icons/x';
import MoreHorizontal from 'lucide-svelte/icons/ellipsis';
import GripVertical from 'lucide-svelte/icons/grip-vertical';
import Maximize2 from 'lucide-svelte/icons/maximize-2';
import ArrowUp from 'lucide-svelte/icons/arrow-up';
import ArrowDown from 'lucide-svelte/icons/arrow-down';
import ArrowUpRight from 'lucide-svelte/icons/arrow-up-right';
import ArrowDownRight from 'lucide-svelte/icons/arrow-down-right';
import Pin from 'lucide-svelte/icons/pin';
import {invoke} from '@tauri-apps/api/core';
import {listen,emit} from '@tauri-apps/api/event';
import {getCurrentWindow} from '@tauri-apps/api/window';
import DockNote from '$lib/DockNote.svelte';
import TranslatePanel from '$lib/TranslatePanel.svelte';
import ProofreadPanel from '$lib/ProofreadPanel.svelte';
import {emptyVoice,updateSettings,timeLabel,type VoiceState,type Settings} from '$lib/api';
import {loadLayout,validateLayout,reorder,resizeBlock,dockKey,blockIds,blockNames,type BlockId,type DockLayout,type DockState} from '$lib/dock';
let layout=$state<DockLayout>(validateLayout(null));let nativeVisible=$state(false);let launcher=$state<'add'|'menu'|null>(null);let autoLive=$state(false);let liveSuppressed=$state(false);let cfg=$state<Settings|null>(null);let voice=$state<VoiceState>({...emptyVoice});let error=$state('');let busy=$state(false);
let note=$state<{flush:()=>Promise<void>}|undefined>();let liveText=$state<HTMLDivElement|undefined>();let modulesEl=$state<HTMLDivElement|undefined>();let alive=true;let revision=-1;let mounted=$state(false);let quitting=$state(false);let collapseTimer:ReturnType<typeof setTimeout>|undefined;let sizingTimer:ReturnType<typeof setTimeout>|undefined;let chain:Promise<void>=Promise.resolve();let currentSession:string|null=null;let successText=$state('');
const active=$derived(voice.microphone_open||['arming','recording','listening'].includes(voice.phase));
const working=$derived(active||voice.phase==='transcribing');
const hasResult=$derived(voice.phase==='result'||(voice.phase==='error'&&!!voice.text));
const blocks=$derived.by(()=>{const list=layout.blocks.map(b=>({...b}));if(!autoLive)return list;const current=list.find(b=>b.id==='live');const live=current?{...current,collapsed:false}:{id:'live' as BlockId,height:190,collapsed:false};return[live,...list.filter(b=>b.id!=='live')]});
const previewWanted=$derived(nativeVisible&&!liveSuppressed&&blocks.some(b=>b.id==='live'&&!b.collapsed));
$effect(()=>{const enabled=previewWanted;if(mounted)void invoke('voice_preview_interest',{enabled}).catch(e=>error=String(e))});
const displayText=$derived(liveSuppressed?'':working?(voice.partial||''):hasResult?voice.text:successText);
function persist(){try{localStorage.setItem(dockKey,JSON.stringify(layout))}catch(e){error='Disposition non enregistrée : '+String(e)}}
function task(work:()=>Promise<void>){const next=chain.then(async()=>{busy=true;error='';try{await work()}catch(e){error=String(e)}finally{busy=false}});chain=next;return next}
function desiredHeight(){const chrome=layout.width<=310?140:112;return Math.min(1000,chrome+(launcher?(launcher==='menu'?366:260):0)+blocks.reduce((sum,b)=>sum+(cfg?.large_controls?104:52)+(b.collapsed?0:b.height+(cfg?.large_controls?44:8)),0))}
async function sync(){if(!mounted)return;const visible=nativeVisible&&(!!launcher||blocks.length>0);await invoke('dock_sync',{request:{width:layout.width,height:desiredHeight(),visible,focus:false}})}
function scheduleSize(){if(!mounted)return;if(sizingTimer)clearTimeout(sizingTimer);sizingTimer=setTimeout(()=>{void sync().catch(e=>error=String(e))},35)}
$effect(()=>{layout.width;blocks;launcher;nativeVisible;cfg?.large_controls;scheduleSize()});
$effect(()=>{displayText;void tick().then(()=>{if(liveText)liveText.scrollTop=liveText.scrollHeight})});
function add(id:BlockId){liveSuppressed=false;autoLive=id==='live'?false:autoLive;const item=layout.blocks.find(b=>b.id===id);layout={...layout,blocks:item?layout.blocks.map(b=>b.id===id?{...b,collapsed:false}:b):[...layout.blocks,{id,collapsed:false,height:id==='notes'?280:id==='translation'?350:190}]};launcher=null;persist()}
async function change(id:BlockId,remove=false){if(id==='notes')await note?.flush();if(id==='live'){autoLive=false;liveSuppressed=true}const item=layout.blocks.find(b=>b.id===id);if(!item){scheduleSize();return}layout={...layout,blocks:remove?layout.blocks.filter(b=>b.id!==id):layout.blocks.map(b=>b.id===id?{...b,collapsed:!b.collapsed}:b)};persist();if(!blocks.length&&!launcher)nativeVisible=false}
function move(id:BlockId,direction:number){layout=reorder(layout,id,direction);persist()}
function resizeWidth(width:number){layout={...layout,width:Math.max(280,Math.min(640,Math.round(width)))};persist()}
function startWidthResize(event:PointerEvent){
 if(event.button!==0)return;event.preventDefault();
 const handle=event.currentTarget as HTMLElement;const startX=event.screenX;const startWidth=layout.width;
 handle.setPointerCapture(event.pointerId);
 // The native column stays anchored to the right of the microphone.
 const move=(e:PointerEvent)=>resizeWidth(startWidth+startX-e.screenX);
 const finish=()=>{handle.removeEventListener('pointermove',move);handle.removeEventListener('pointerup',finish);handle.removeEventListener('pointercancel',finish);handle.removeEventListener('lostpointercapture',finish);persist()};
 handle.addEventListener('pointermove',move);handle.addEventListener('pointerup',finish,{once:true});handle.addEventListener('pointercancel',finish,{once:true});handle.addEventListener('lostpointercapture',finish,{once:true});
}
function changeHeight(id:BlockId,height:number){layout=resizeBlock(layout,id,height);persist()}
function startHeightResize(event:PointerEvent,id:BlockId){
 if(event.button!==0)return;event.preventDefault();
 const block=layout.blocks.find(b=>b.id===id);if(!block)return;
 const handle=event.currentTarget as HTMLElement;const startY=event.screenY;const startHeight=block.height;
 handle.setPointerCapture(event.pointerId);
 const move=(e:PointerEvent)=>changeHeight(id,startHeight+startY-e.screenY);
 const finish=()=>{handle.removeEventListener('pointermove',move);handle.removeEventListener('pointerup',finish);handle.removeEventListener('pointercancel',finish);handle.removeEventListener('lostpointercapture',finish);persist()};
 handle.addEventListener('pointermove',move);handle.addEventListener('pointerup',finish,{once:true});handle.addEventListener('pointercancel',finish,{once:true});handle.addEventListener('lostpointercapture',finish,{once:true});
}
function enlarge(id:BlockId){launcher=null;const resized=resizeBlock(layout,id,640);layout={...resized,blocks:resized.blocks.map(b=>b.id===id?{...b,collapsed:false}:b)};persist()}
function startDrag(event:PointerEvent,id:BlockId){if(event.button!==0)return;event.preventDefault();const handle=event.currentTarget as HTMLElement;handle.setPointerCapture(event.pointerId);const finish=(e:PointerEvent)=>{handle.removeEventListener('pointerup',finish);handle.removeEventListener('pointercancel',cancel);const target=document.elementFromPoint(e.clientX,e.clientY)?.closest<HTMLElement>('[data-block]')?.dataset.block as BlockId|undefined;if(!target||target===id)return;const from=layout.blocks.findIndex(b=>b.id===id);const to=layout.blocks.findIndex(b=>b.id===target);if(from<0||to<0)return;const next=[...layout.blocks];const [b]=next.splice(from,1);next.splice(to,0,b);layout={...layout,blocks:next};persist()};const cancel=()=>{handle.removeEventListener('pointerup',finish);handle.removeEventListener('pointercancel',cancel)};handle.addEventListener('pointerup',finish,{once:true});handle.addEventListener('pointercancel',cancel,{once:true})}
async function accept(state:DockState){nativeVisible=state.visible;if(!state.visible){launcher=null;autoLive=false;liveSuppressed=true;return}if(state.revision<=revision||!state.action)return;revision=state.revision;
 switch(state.action){case'add':launcher=launcher==='add'?null:'add';break;case'menu':launcher=launcher==='menu'?null:'menu';break;case'live-auto':autoLive=true;liveSuppressed=false;await tick();modulesEl?.scrollTo({top:0,behavior:'auto'});break;case'toggle-live':add('live');break;case'notes':add('notes');break;case'controls':add('controls');break;case'translation':add('translation');break}
}
function received(next:VoiceState){
 const before=working;const previous=voice;voice=next;
 if(next.session_id&&next.session_id!==currentSession){currentSession=next.session_id;liveSuppressed=false;successText='';if(collapseTimer)clearTimeout(collapseTimer)}
 if(next.phase==='idle'&&before){
  if(next.message.startsWith('Texte transmis')){
   successText=next.text||previous.text||previous.partial;liveSuppressed=false;
   if(collapseTimer)clearTimeout(collapseTimer);collapseTimer=setTimeout(()=>{autoLive=false;liveSuppressed=true;successText=''},3000);
  }else{autoLive=false;liveSuppressed=true;successText=''}
 }
}
async function dimensions(){await note?.flush();await invoke('show_window',{label:'settings'});await emit('settings:navigate','layout')}
async function pin(){cfg=await invoke<Settings>('dock_pin',{pinned:!cfg?.always_on_top})}
async function set(patch:Partial<Settings>){cfg=await updateSettings(patch)}
async function voiceCommand(command:string,args?:Record<string,unknown>){if(command==='voice_cancel'){autoLive=false;liveSuppressed=true;successText=''}const value=await invoke<VoiceState>(command,args);if(value?.phase)received(value)}
async function corrected(next:string,checked:string){if(voice.text!==checked)return false;try{voice=await invoke<VoiceState>('voice_correct_result',{expected:checked,text:next});return true}catch(e){error=String(e);return false}}
async function hide(){await note?.flush();if(active)await voiceCommand('voice_stop');launcher=null;autoLive=false;liveSuppressed=true;nativeVisible=false;await invoke('dock_hide')}
async function closeColumn(){await note?.flush();nativeVisible=false;launcher=null;autoLive=false;liveSuppressed=true;await invoke('dock_sync',{request:{width:layout.width,height:desiredHeight(),visible:false,focus:false}})}
async function menu(action:string){await note?.flush();launcher=null;if(action==='quit'){await invoke('app_request_quit');return}if(action==='hide'){await hide();return}if(action==='capture'){await invoke('capture_begin');return}if(action==='resume-capture'){await invoke('capture_resume');return}if(action==='reset'){await invoke('dock_reset');return}await invoke('show_window',{label:action})}
onMount(()=>{alive=true;layout=loadLayout(localStorage);const offs:(()=>void)[]=[];const keep=(off:()=>void)=>alive?offs.push(off):off();
 void (async()=>{try{
  await listen<DockLayout>('dock:layout-applied',e=>{layout=validateLayout(e.payload);persist()}).then(keep);
  await listen<DockState>('dock:changed',e=>{void accept(e.payload)}).then(keep);
  await listen('dock:request-hide',()=>{void task(hide)}).then(keep);
  await listen('dock:hidden',()=>{nativeVisible=false;launcher=null;autoLive=false;liveSuppressed=true}).then(keep);
  await listen<VoiceState>('voice:state',e=>received(e.payload)).then(keep);
  await listen<Settings>('settings:changed',e=>cfg=e.payload).then(keep);
  await listen<string>('app:notice',e=>error=e.payload).then(keep);
  await listen('app:quit-cancelled',()=>quitting=false).then(keep);
  await listen('app:quit-request',()=>{quitting=true;void task(async()=>{try{await note?.flush();nativeVisible=false;await invoke('app_quit_ready')}catch(e){quitting=false;await invoke('app_quit_abort');nativeVisible=true;await invoke('dock_sync',{request:{width:layout.width,height:desiredHeight(),visible:true,focus:true}});throw e}})}).then(keep);
  await getCurrentWindow().onCloseRequested(e=>{e.preventDefault();void task(closeColumn)}).then(keep);
  const [s,v,d]=await Promise.all([invoke<Settings>('settings_get'),invoke<VoiceState>('voice_get'),invoke<DockState>('dock_get')]);cfg=s;voice=v;mounted=true;await accept(d);scheduleSize();
 }catch(e){error=String(e)}})();
 return()=>{alive=false;mounted=false;offs.forEach(f=>f());if(collapseTimer)clearTimeout(collapseTimer);if(sizingTimer)clearTimeout(sizingTimer)};
});
</script>
<svelte:head><title>Écho V3 — Blocs</title></svelte:head>
<div class="column" aria-label="Blocs du socle Écho">
<button class="width-grip" aria-label="Redimensionner la largeur des outils" title="Glisse vers la gauche pour élargir" onpointerdown={startWidthResize} onkeydown={e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();resizeWidth(layout.width+(e.key==='ArrowLeft'?20:-20))}}}></button>
<header class="column-head"><strong>écho<span> / tes outils</span></strong><button class="icon" aria-label="Ajouter un bloc" title="Ajouter un bloc" onclick={()=>launcher=launcher==='add'?null:'add'}><Plus size={16} aria-hidden="true"/></button><button class="icon" aria-label="Autres actions" title="Autres actions" onclick={()=>launcher=launcher==='menu'?null:'menu'}><MoreHorizontal size={16} aria-hidden="true"/></button><button class="icon" aria-label="Replier tous les blocs" title="Replier la colonne" onclick={()=>void task(closeColumn)}><Minus size={16} aria-hidden="true"/></button></header>
<div class="modules" bind:this={modulesEl}>
{#if launcher==='add'}<section class="launcher" aria-label="Ajouter un bloc"><div class="launcher-head"><h2>À portée de main</h2><button class="icon" aria-label="Fermer le choix des outils" title="Fermer le choix des outils" onclick={()=>launcher=null}><X size={16} aria-hidden="true"/></button></div><p>Ajoute les outils dont tu as besoin.</p>{#each blockIds as id}<button class="launcher-item" onclick={()=>add(id)}><span>{blockNames[id]}</span><span aria-hidden="true">{#if layout.blocks.some(b=>b.id===id)}Ouvrir{:else}<Plus size={16} aria-hidden="true"/>{/if}</span></button>{/each}</section>{/if}
{#if launcher==='menu'}<nav class="launcher" aria-label="Menu Écho"><div class="launcher-head"><h2>Ton espace</h2><button class="icon" aria-label="Fermer le menu Écho" title="Fermer le menu" onclick={()=>launcher=null}><X size={16} aria-hidden="true"/></button></div><button class="launcher-item" onclick={()=>void task(()=>menu('main'))}>Ouvrir les notes <ArrowUpRight size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(()=>menu('capture'))}>Capturer l’écran <ArrowUpRight size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(()=>menu('resume-capture'))}>Reprendre la dernière capture <ArrowUpRight size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(()=>menu('settings'))}>Apparence et réglages <ArrowUpRight size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(pin)}>{cfg?.always_on_top?'Désépingler les outils':'Épingler les outils'} <Pin size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(()=>menu('reset'))}>Replacer en bas à droite <ArrowDownRight size={16} aria-hidden="true"/></button><button class="launcher-item" onclick={()=>void task(()=>menu('hide'))}>Masquer Écho <Minus size={16} aria-hidden="true"/></button><button class="launcher-item danger" onclick={()=>void task(()=>menu('quit'))}>Quitter Écho <X size={16} aria-hidden="true"/></button></nav>{/if}
{#each blocks as b (b.id)}
<section class="block" data-block={b.id} aria-label={blockNames[b.id]}>
{#if !b.collapsed}<button class="height-grip" aria-label={'Redimensionner la hauteur de '+blockNames[b.id]} title="Tire vers le haut pour agrandir · flèches haut et bas au clavier" onpointerdown={e=>startHeightResize(e,b.id)} onkeydown={e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();changeHeight(b.id,b.height+(e.key==='ArrowUp'?20:-20))}}}><span></span></button>{/if}
<div class="block-head"><button class="icon grip" aria-label={'Réordonner '+blockNames[b.id]} title="Glisser pour réordonner · flèches à droite" onpointerdown={e=>startDrag(e,b.id)}><GripVertical size={16} aria-hidden="true"/></button><h2>{blockNames[b.id]}</h2><button class="icon" aria-label={'Agrandir '+blockNames[b.id]} title="Agrandir et fermer le menu des outils" onclick={()=>enlarge(b.id)}><Maximize2 size={16} aria-hidden="true"/></button><button class="icon arrow" aria-label={'Monter '+blockNames[b.id]} disabled={layout.blocks.findIndex(x=>x.id===b.id)<=0} onclick={()=>move(b.id,-1)}><ArrowUp size={16} aria-hidden="true"/></button><button class="icon arrow" aria-label={'Descendre '+blockNames[b.id]} disabled={layout.blocks.findIndex(x=>x.id===b.id)<0||layout.blocks.findIndex(x=>x.id===b.id)>=layout.blocks.length-1} onclick={()=>move(b.id,1)}><ArrowDown size={16} aria-hidden="true"/></button><button class="icon" aria-label={(b.collapsed?'Déployer ':'Réduire ')+blockNames[b.id]} aria-expanded={!b.collapsed} onclick={()=>void task(()=>change(b.id))}>{#if b.collapsed}<Plus size={16} aria-hidden="true"/>{:else}<Minus size={16} aria-hidden="true"/>{/if}</button><button class="icon" aria-label={'Retirer '+blockNames[b.id]} onclick={()=>void task(()=>change(b.id,true))}><X size={16} aria-hidden="true"/></button></div>
<div class="block-content" class:collapsed={b.collapsed} style:height={b.height+'px'}>
{#if b.id==='notes'}<DockNote bind:this={note} locked={busy||quitting}/>{:else if b.id==='translation'}<TranslatePanel active={nativeVisible&&!b.collapsed}/>{:else if b.id==='controls'}
<div class="controls"><label>Geste du raccourci<select disabled={working||busy} value={cfg?.voice_mode||'toggle'} onchange={e=>void task(()=>set({voice_mode:e.currentTarget.value as Settings['voice_mode']}))}><option value="hold">Maintenir pour parler</option><option value="toggle">Appuyer pour démarrer / arrêter</option><option value="handsfree">Mains libres</option></select></label><p><kbd>{cfg?.shortcut_dictation.replace('Space','Espace')||'Ctrl+Alt+Espace'}</kbd></p><button class="btn" onclick={()=>void task(()=>menu('settings'))}>Microphone et vocabulaire</button></div>
{:else}
<div class="live"><div class="live-meta"><span>{working?(active?'Je t’écoute':'Finalisation…'):hasResult?'Résultat à reprendre':'Prêt à écouter'}</span>{#if active}<time>{timeLabel(voice.duration_secs)}{voice.mode!=='handsfree'?' / 05:00':''}</time>{/if}</div><div class="live-text" bind:this={liveText} aria-label="Texte de la dictée">{displayText|| (active?'Le texte apparaît après un court délai.':voice.phase==='transcribing'?'Le moteur finalise la prise…':'Le direct apparaîtra pendant ta prochaine dictée.')}</div>
{#if working}<p class="muted">Aperçu provisoire · corrigé à l’arrêt</p><button class="btn" onclick={()=>void task(()=>voiceCommand('voice_cancel'))}>Annuler la prise</button>{/if}
{#if hasResult&&!liveSuppressed}<div class="result-actions"><button class="btn primary" onclick={()=>void task(()=>voiceCommand('voice_resolve',{action:'copy'}))}>Copier</button><button class="btn" onclick={()=>void task(()=>voiceCommand('voice_resolve',{action:'note'}))}>Garder en note</button><button class="btn ghost" onclick={()=>void task(async()=>{await voiceCommand('voice_resolve',{action:'discard'});liveSuppressed=true;autoLive=false})}>Écarter</button></div><ProofreadPanel text={voice.text} onReplace={corrected}/>{/if}
{#if voice.error}<p class="voice-error" role="alert">{voice.error}</p>{#if voice.audio_path&&!hasResult}<button class="btn" onclick={()=>void task(()=>voiceCommand('voice_retry'))}>Réessayer la transcription</button><button class="btn ghost" onclick={()=>void task(()=>voiceCommand('voice_resolve',{action:'discard'}))}>Écarter la prise</button>{/if}{/if}</div>
{/if}
</div>
</section>
{/each}
{#if error}<div class="feedback" role="alert"><span>{error}</span><button class="icon" aria-label="Fermer le message" onclick={()=>error=''}><X size={16} aria-hidden="true"/></button></div>{/if}
</div>
<footer><button class="btn ghost" onclick={()=>void task(dimensions)}>Régler les dimensions</button><span title="Bord gauche : largeur · poignée du bloc : hauteur">Bord gauche : largeur · poignée du bloc : hauteur</span></footer>
</div>
<style>
.launcher-head{display:flex;align-items:center;justify-content:space-between;gap:8px}.launcher-head h2{margin:0}.height-grip{display:flex;align-items:center;justify-content:center;width:100%;height:8px;padding:0;border:0;background:transparent;cursor:ns-resize;touch-action:none}.height-grip span{display:block;width:44px;height:3px;border-radius:3px;background:var(--border)}.height-grip:hover span,.height-grip:focus-visible span{background:var(--muted)}

.width-grip{position:absolute;left:0;top:12px;bottom:12px;width:10px;z-index:20;padding:0;border:0;background:transparent;cursor:ew-resize;touch-action:none}.width-grip:hover,.width-grip:focus-visible{background:var(--border);border-radius:5px}.column{container-type:inline-size;height:calc(100vh - 8px);margin:4px;border:1px solid var(--border);border-radius:16px;background:var(--surface);display:flex;flex-direction:column;overflow:hidden;box-shadow:0 3px 12px #0002}
.column-head{height:48px;flex:none;display:flex;align-items:center;gap:3px;padding:0 8px 0 14px;border-bottom:1px solid var(--border)}strong{font-size:18px;letter-spacing:-.04em;flex:1}strong span{font-size:12px;color:var(--muted);font-weight:400;letter-spacing:0}.icon{width:32px;height:36px;flex:none;padding:0;display:grid;place-items:center;border:0;border-radius:7px;background:transparent;color:var(--muted);font-size:19px}.icon:hover:not(:disabled){background:var(--hover);color:var(--text)}.column-head .icon{width:38px;height:40px}.icon:focus-visible{outline-offset:-3px}.modules{flex:1;overflow:auto;min-height:0;scrollbar-width:thin;scrollbar-color:var(--border) transparent}.launcher{padding:12px}.launcher h2{font-size:15px;margin-bottom:6px}.launcher p{font-size:12px;line-height:1.5;color:var(--muted);margin-bottom:10px}.launcher-item{min-height:44px;width:100%;display:flex;align-items:center;justify-content:space-between;gap:8px;background:transparent;border:0;border-radius:8px;color:var(--text);font-size:13px;padding:9px 10px;text-align:left}.launcher-item:hover{background:var(--hover)}.launcher-item span:last-child{color:var(--muted)}.launcher-item.danger{color:var(--danger)}.block{border-bottom:1px solid var(--border);min-width:0}.block-head{height:48px;display:grid;grid-template-columns:22px minmax(0,1fr) 32px 26px 26px 32px 32px;align-items:center;gap:1px;padding:4px}.block-head h2{flex:1;min-width:0;font-size:13px;margin:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;letter-spacing:0}.grip{width:22px;cursor:grab;font-size:20px}.arrow{width:26px;font-size:16px}.block-content{min-height:0;overflow:auto;background:var(--surface)}.block-content.collapsed{display:none}footer{min-height:52px;flex-wrap:nowrap;flex:none;display:flex;align-items:center;gap:9px;padding:0 12px;font-size:11px;color:var(--muted);border-top:1px solid var(--border);background:var(--surface)}footer span{font-size:10px;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.controls,.live{padding:10px 12px;font-size:13px;line-height:1.55}.controls label{display:block;margin-bottom:10px;font-size:12px;color:var(--muted)}select{display:block;width:100%;min-height:40px;margin-top:5px;padding:6px;border:1px solid var(--border);border-radius:7px;background:var(--surface);color:var(--text);font-size:13px}.controls p{font-size:12px;color:var(--muted)}.btn{font-size:12px;min-height:40px}.live-meta{display:flex;justify-content:space-between;gap:8px;color:var(--muted);font-size:12px}.live-text{margin:10px 0;white-space:pre-wrap;overflow-wrap:anywhere;line-height:1.65;font-size:15px;max-height:260px;overflow:auto}.live .muted{font-size:12px}.result-actions{display:flex;gap:5px;flex-wrap:wrap}.voice-error{color:var(--danger);background:var(--danger-soft);padding:8px;border-radius:7px;font-size:13px;margin-top:10px}.feedback{display:flex;align-items:center;gap:6px;padding:10px;color:var(--danger);background:var(--danger-soft);font-size:13px;line-height:1.5}.feedback span{flex:1;min-width:0;overflow-wrap:anywhere}

.launcher-item>:global(svg){color:var(--muted)}.block-head .icon{color:var(--text)}.block-head .icon:disabled{color:var(--muted);opacity:.42}
:global(html[data-large-controls="true"]) .block-head{grid-template-columns:repeat(5,minmax(0,1fr));grid-template-rows:44px 44px;gap:4px;height:100px;padding:4px}
:global(html[data-large-controls="true"]) .block-head h2{grid-column:2/-1;font-size:15px}
@container(max-width:310px){.column-head strong span{display:none}footer{gap:6px;padding:0 10px}}
</style>
