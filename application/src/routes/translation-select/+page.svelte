<script lang="ts">
 import {onMount} from 'svelte';
 import {invoke} from '@tauri-apps/api/core';
 import {listen,type UnlistenFn} from '@tauri-apps/api/event';
 let token='',start:{x:number;y:number}|null=null,end:{x:number;y:number}|null=null,error='',dragging=false;
 $: rect=start&&end?{x:Math.min(start.x,end.x),y:Math.min(start.y,end.y),w:Math.abs(end.x-start.x),h:Math.abs(end.y-start.y)}:null;
 async function cancel(){await invoke('translation_cancel_selection').catch(()=>{})}
 function down(e:PointerEvent){if(e.button!==0||(e.target as HTMLElement).closest(".instruction"))return;start={x:e.clientX,y:e.clientY};end=start;dragging=true;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);error=''}
 function move(e:PointerEvent){if(dragging)end={x:Math.max(0,Math.min(innerWidth,e.clientX)),y:Math.max(0,Math.min(innerHeight,e.clientY))}}
 async function up(e:PointerEvent){if(!dragging)return;move(e);dragging=false;if(!rect)return;
  try{await invoke('translation_commit_region',{token,x:rect.x/innerWidth,y:rect.y/innerHeight,width:rect.w/innerWidth,height:rect.h/innerHeight});start=end=null}
  catch(e){error=String(e)}
 }
 onMount(()=>{let dead=false;let unlisten:UnlistenFn|undefined;listen<{token:string}>('translation:selection-opened',({payload})=>{token=payload.token;start=end=null;error='';dragging=false}).then(fn=>{if(dead)fn();else unlisten=fn});invoke<{token:string}>('translation_selection_state').then(v=>token=v.token).catch(e=>error=String(e));
  const key=(e:KeyboardEvent)=>{if(e.key==='Escape')void cancel()};window.addEventListener('keydown',key);window.focus();return()=>{dead=true;unlisten?.();window.removeEventListener('keydown',key)};
 });
</script>
<svelte:head><title>Écho — zone à traduire</title></svelte:head>
<div class="selection" role="application" aria-label="Dessine une zone à traduire. Échap pour annuler." on:pointerdown={down} on:pointermove={move} on:pointerup={up}>
 <div class="instruction"><strong>Sélectionne le texte à traduire</strong><span>Glisse sur une zone · Échap pour annuler</span><button on:click={cancel}>Annuler</button>{#if error}<p role="alert">{error}</p>{/if}</div>
 {#if rect}<div class="rectangle" style:left={rect.x+'px'} style:top={rect.y+'px'} style:width={rect.w+'px'} style:height={rect.h+'px'}></div>{/if}
</div>
<style>
 :global(html),:global(body){margin:0!important;background:transparent!important;overflow:hidden!important}
 .selection{position:fixed;inset:0;background:#06112126;cursor:crosshair;user-select:none;touch-action:none}
 .instruction{position:fixed;left:50%;top:24px;transform:translateX(-50%);display:flex;align-items:center;gap:16px;padding:14px 18px;border-radius:16px;background:#101a2af2;color:#fff;font:13px system-ui,sans-serif;box-shadow:0 10px 40px #0005;cursor:default}
 .instruction span{opacity:.8}.instruction p{color:#ff9aa8;margin:0}
 button{color:#fff;background:#ffffff18;border:1px solid #ffffff66;border-radius:8px;padding:8px 13px;cursor:pointer}button:focus-visible{outline:3px solid #9dc3ff;outline-offset:2px}
 .rectangle{position:fixed;box-sizing:border-box;border:2px solid #a3c6ff;background:#a3c6ff0d;box-shadow:0 0 0 9999px #06112136;pointer-events:none}
</style>
