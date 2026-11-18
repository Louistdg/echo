<script lang="ts">
import {onMount,untrack} from 'svelte';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {VoiceState} from '$lib/api';

let {text,sourceLanguage='fr',locked=false}:{text:string;sourceLanguage?:string;locked?:boolean}=$props();
const languages=[{code:'fr',name:'Français'},{code:'en',name:'Anglais'},{code:'de',name:'Allemand'},{code:'nl',name:'Néerlandais'}];
let from=$state(untrack(()=>sourceLanguage==='en'?'en':'fr'));
let to=$state(untrack(()=>sourceLanguage==='en'?'fr':'en'));
let busy=$state(false),voiceBusy=$state(true),error=$state(''),status=$state('');
let result=$state<{original:string;from:string;to:string;text:string;warnings:string[]}|null>(null);
let alive=true,epoch=0;
const visibleResult=$derived(result&&result.original===text&&result.from===from&&result.to===to?result:null);
const voiceIsBusy=(voice:VoiceState)=>voice.microphone_open||['arming','recording','listening','transcribing','finalizing'].includes(voice.phase);

async function translate(){
 if(locked||busy||voiceBusy||!text.trim())return;
 const request=++epoch,original=text,source=from,target=to;
 busy=true;error='';status='Traduction locale…';result=null;
 try{
  const next=await invoke<{text:string;warnings?:string[]}>('translate_text',{text:original,from:source,to:target});
  if(!alive||request!==epoch)return;
  if(text!==original||from!==source||to!==target){status='La note ou les langues ont changé. Relance la traduction.';return}
  result={original,from:source,to:target,text:next.text,warnings:next.warnings??[]};
  status='Traduction terminée. Ta note originale est conservée.';
 }catch(e){if(alive&&request===epoch){error=String(e);status=''}}
 finally{if(alive&&request===epoch)busy=false}
}
async function stop(){epoch++;busy=false;status='Traduction arrêtée. Ta note est conservée.';try{await invoke('translation_stop')}catch(e){if(alive)error=String(e)}}
async function copy(){if(!visibleResult)return;try{await navigator.clipboard.writeText(visibleResult.text);status='Traduction copiée.'}catch{error='Impossible de copier la traduction.'}}
onMount(()=>{
 alive=true;let changed=false;let off:(()=>void)|undefined;
 void listen<VoiceState>('voice:state',e=>{changed=true;voiceBusy=voiceIsBusy(e.payload)}).then(fn=>alive?off=fn:fn());
 void invoke<VoiceState>('voice_get').then(voice=>{if(alive&&!changed)voiceBusy=voiceIsBusy(voice)}).catch(e=>{if(alive)error=String(e)});
 return()=>{alive=false;off?.();epoch++;if(busy)void invoke('translation_stop').catch(()=>{})};
});
</script>

<section class="note-translation" aria-label="Traduire la note" aria-busy={busy}>
 <div class="languages">
  <label>De<select aria-label="Langue de la note" bind:value={from} disabled={busy}>{#each languages as language}<option value={language.code}>{language.name}</option>{/each}</select></label>
  <button class="swap" aria-label="Inverser les langues de la note" disabled={busy} onclick={()=>{[from,to]=[to,from];status='';error=''}}>⇄</button>
  <label>Vers<select aria-label="Traduire la note vers" bind:value={to} disabled={busy}>{#each languages as language}<option value={language.code}>{language.name}</option>{/each}</select></label>
 </div>
 <div class="actions"><button class="btn primary" disabled={locked||busy||voiceBusy||!text.trim()||text.length>10000||from===to} onclick={translate}>Traduire la note</button>{#if busy}<button class="btn" onclick={stop}>Arrêter</button>{/if}</div>
 {#if voiceBusy}<p class="hint">Arrête la dictée pour traduire la note.</p>{/if}
 {#if text.length>10000}<p class="hint">La traduction est limitée à 10 000 caractères par note.</p>{/if}
 {#if visibleResult}<div class="translated"><h3>Traduction</h3><p>{visibleResult.text}</p><button class="btn" onclick={copy}>Copier la traduction</button>{#each visibleResult.warnings as warning}<p class="hint">{warning}</p>{/each}</div>{/if}
 {#if status}<p class="hint" role="status">{status}</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 <p class="hint">Sur ce PC, sans API payante. Le texte original reste dans la note au-dessus.</p>
</section>

<style>
.note-translation{padding:10px;display:grid;gap:8px;border-top:1px solid var(--border);font-size:12px}.languages{display:flex;gap:6px;align-items:end}.languages label{display:grid;gap:4px;flex:1;min-width:0}.languages select{width:100%;padding:7px;border:1px solid var(--border);border-radius:8px;background:var(--surface);color:var(--text);font:inherit}.swap{border:1px solid var(--border);background:var(--surface);color:var(--text);border-radius:8px;min-height:34px;padding:4px 9px}.actions{display:flex;gap:6px;flex-wrap:wrap}.btn{min-height:34px;font-size:12px}.translated{padding:10px;background:var(--surface);border:1px solid var(--border);border-radius:9px}.translated h3{margin:0 0 6px;font-size:12px}.translated p{white-space:pre-wrap;overflow-wrap:anywhere;margin:0 0 8px}.hint{margin:0;color:var(--muted);font-size:11px;line-height:1.5}.error{margin:0;color:var(--danger);overflow-wrap:anywhere}button:focus-visible,select:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
</style>
