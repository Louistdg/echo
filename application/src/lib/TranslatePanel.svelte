<script lang="ts">
 import { onMount } from 'svelte';
 import { invoke } from '@tauri-apps/api/core';
 import { listen, type UnlistenFn } from '@tauri-apps/api/event';
 export let active = true;
 type Region={x:number;y:number;width:number;height:number;token:string};
 type Result={text?:string;source?:string;route?:string[];warnings?:string[];ocr_warnings?:string[];changed?:boolean;message?:string};
 const languages=[{code:'fr',name:'Français'},{code:'en',name:'Anglais'},{code:'nl',name:'Néerlandais'},{code:'de',name:'Allemand'},{code:'es',name:'Espagnol'},{code:'it',name:'Italien'}];
 let from='en',to='fr',input='',output='',source='',error='',status='Texte ou zone d’écran, à toi de choisir.';
 let busy=false,following=false,voiceBusy=false,selecting=false,mounted=false,region:Region|null=null;
 let epoch=0,timer:ReturnType<typeof setTimeout>|undefined;
 let warnings:string[]=[];
 let catalogAttempted=false,catalogPending=false;
 async function loadCatalog(){catalogAttempted=true;catalogPending=true;const request=epoch;try{await invoke('translation_catalog');if(mounted&&active&&request===epoch)status='6 langues · traduction locale · aucun abonnement.'}catch(e){if(mounted&&active&&request===epoch)error=String(e)}finally{catalogPending=false}}
 function clearTimer(){if(timer)clearTimeout(timer);timer=undefined}
 async function stop(message='Traduction arrêtée.'){
  epoch++;following=false;busy=false;selecting=false;clearTimer();status=message;
  if(mounted)await Promise.allSettled([invoke('translation_stop'),invoke('translation_cancel_selection')]);
 }
 function later(){clearTimer();if(active&&following&&!voiceBusy)timer=setTimeout(()=>void screen(false),2800)}
 async function screen(force=true){
  if(!active||!region||busy||voiceBusy)return;
  const request=++epoch;busy=true;error='';status=following?'Lecture de la zone…':'Traduction de la zone…';
  try{
   const result=await invoke<Result>('translation_screen',{region,from,to,force});
   if(request!==epoch||!active)return;
   source=result.source??'';output=result.text??'';warnings=[...(result.warnings??[]),...(result.ocr_warnings??[])];
   status=result.message??(following?(result.changed===false?'Zone inchangée · suivi actif':'Traduction actualisée · suivi actif'):'Zone traduite.');
  }catch(e){if(request===epoch){error=String(e);following=false;status='La traduction a été interrompue.'}}
  finally{if(request===epoch){busy=false;later()}}
 }
 async function translate(){
  await stop('Traduction…');
  if(!active||!input.trim())return;
  const request=++epoch,original=input;busy=true;error='';warnings=[];
  try{
   const result=await invoke<Result>('translate_text',{text:original,from,to});
   if(request!==epoch||input!==original||!active)return;
   output=result.text??'';source=original;warnings=result.warnings??[];status=result.route&&result.route.length>2?'Traduit localement via l’anglais.':'Traduit sur cet ordinateur.';
  }catch(e){if(request===epoch)error=String(e)}
  finally{if(request===epoch)busy=false}
 }
 async function select(){
  await stop('Dessine la zone à lire. Échap pour annuler.');error='';selecting=true;
  try{await invoke('translation_select_region')}catch(e){error=String(e);selecting=false}
 }
 async function toggleFollow(){
  if(following){await stop();return}
  if(!region)return;
  following=true;error='';status=voiceBusy?'En pause pendant la dictée.':'Suivi de la zone activé.';
  await screen(true);
 }
 async function copy(){try{await navigator.clipboard.writeText(output);status='Traduction copiée.'}catch(e){error='Copie impossible : '+String(e)}}
 async function paste(){error='';try{const text=await navigator.clipboard.readText();if(!text.trim()){status='Presse-papiers vide.';return}input=text.slice(0,10000);invalidate();status=text.length>10000?'Texte collé · limité à 10 000 caractères.':'Texte collé.'}catch(e){error='Lecture du presse-papiers impossible : '+String(e)}}
 function clearInput(){input='';invalidate();status='Texte effacé.'}
 function invalidate(){
  output='';source='';warnings=[];
  if(busy||following||selecting)void stop('Prêt à traduire.');else {epoch++;status='Prêt à traduire.'}
 }
 function voiceState(payload:any){
  const next=['arming','recording','listening','transcribing','finalizing'].includes(payload.phase??'')||['ecoute','transcription'].includes(payload.state??'')||payload.microphone_open===true||payload.recording===true||payload.busy===true;
  if(next&&!voiceBusy){voiceBusy=true;epoch++;busy=false;clearTimer();void invoke('translation_stop').catch(()=>{});status='En pause pendant la dictée.'}
  else if(!next&&voiceBusy){voiceBusy=false;if(following){status='Reprise du suivi…';later()}}
 }
 onMount(()=>{
  mounted=true;let dead=false;const cleanups:UnlistenFn[]=[];
  Promise.all([
   listen<Region>('translation:region',({payload})=>{if(!active)return;selecting=false;region=payload;source='';output='';void screen(true)}),
   listen('translation:selection-cancelled',()=>{selecting=false;if(active)status='Sélection annulée.'}),
   listen('voice:state',({payload})=>voiceState(payload)),
   listen('dock:voice',({payload})=>voiceState(payload)),
   listen('dock:hidden',()=>void stop()),
  ]).then(items=>{if(dead)items.forEach(fn=>fn());else cleanups.push(...items)}).catch(e=>{error=String(e)});
  return()=>{void stop();dead=true;cleanups.forEach(fn=>fn());mounted=false;clearTimer()};
 });
 $: if(mounted&&active&&!voiceBusy&&!catalogAttempted)void loadCatalog();
 $: if(mounted&&!active&&(busy||following||selecting))void stop();
</script>

<div class="translator" aria-busy={busy}>
 <div class="language-row">
  <label>De<select bind:value={from} on:change={invalidate} aria-label="Langue du texte source">{#each languages as lang}<option value={lang.code}>{lang.name}</option>{/each}</select></label>
  <button class="swap" title="Inverser les langues" aria-label="Inverser les langues" on:click={()=>{[from,to]=[to,from];invalidate()}}>⇄</button>
  <label>Vers<select bind:value={to} on:change={invalidate} aria-label="Langue de traduction">{#each languages as lang}<option value={lang.code}>{lang.name}</option>{/each}</select></label>
 </div>
 <label class="text-label" for="echo-translation-input">Ton texte</label>
 <textarea id="echo-translation-input" bind:value={input} on:input={invalidate} maxlength="10000" placeholder="Colle un texte à traduire…" rows="3"></textarea>
 <div class="actions">
  <button disabled={!active||busy||voiceBusy} on:click={paste}>Coller</button>
  <button disabled={!input&&!output&&!source&&!warnings.length} on:click={clearInput}>Effacer</button>
  <button class="primary" disabled={!active||catalogPending||busy||voiceBusy||!input.trim()} on:click={translate}>Traduire</button>
  <button disabled={!active||catalogPending||busy||voiceBusy||selecting} on:click={select}>{region?'Changer de zone':'Lire une zone'}</button>
 </div>
 {#if region}
  <div class="screen-controls">
   <span>Zone {region.width} × {region.height}</span>
   <button class:running={following} disabled={!active||(!following&&(voiceBusy||busy))} on:click={toggleFollow}>{following?'Arrêter le suivi':'Suivre la zone'}</button>
   <button disabled={!active||catalogPending||busy||voiceBusy} on:click={()=>screen(true)} aria-label="Traduire à nouveau la zone">↻</button>
  </div>
 {/if}
 {#if busy||selecting}<button class="stop" on:click={()=>stop()}>Arrêter</button>{/if}
 {#if source&&region}<details><summary>Texte lu à l’écran</summary><p class="source">{source}</p></details>{/if}
 {#if output}
  <div class="result"><p>{output}</p><button on:click={copy} aria-label="Copier la traduction">Copier</button></div>
 {/if}
 <p class="status" role="status">{status}</p>
 {#each warnings as warning}<p class="warning">{warning}</p>{/each}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 <p class="hint">Le suivi s’arrête à la fermeture du bloc. Il reste désactivé au redémarrage.</p>
</div>

<style>
 .translator{display:grid;gap:10px;min-width:0;color:var(--text,var(--txt,#e8edf6));font:13px/1.45 system-ui,sans-serif}
 .language-row{display:flex;align-items:end;gap:7px}
 label{display:grid;gap:4px;min-width:0;flex:1;color:inherit;font-size:11px}
 select,textarea,button{font:inherit;color:inherit;border:1px solid var(--border,var(--line-2,#737f96));border-radius:10px}
 select{width:100%;min-width:0;padding:7px;background:var(--surface,#1b2335)}
 textarea{box-sizing:border-box;width:100%;min-height:70px;resize:vertical;max-height:240px;padding:10px;background:var(--surface,#1b2335)}
 button{cursor:pointer;padding:8px 11px;background:var(--surface,#1b2335);min-height:36px}
 button:hover:not(:disabled){filter:brightness(1.14)}
 button:disabled{opacity:.5;cursor:default}
 button:focus-visible,select:focus-visible,textarea:focus-visible{outline:3px solid var(--accent,#93baff);outline-offset:2px}
 .primary{background:var(--accent,#b4caff);color:var(--on-accent,var(--accent-ink,#101729));font-weight:700}
 .swap{padding:6px 10px}.actions{display:flex;gap:7px;flex-wrap:wrap}
 .screen-controls{display:flex;align-items:center;gap:6px;flex-wrap:wrap;padding-top:6px;border-top:1px solid var(--border,var(--line-2,#737f96))}
 .screen-controls span{flex:1;font-size:11px}
 .stop,.running{background:#8b2035!important;color:#fff!important;border-color:#b74259!important;font-weight:650}
 .result{padding:12px;background:var(--surface,#1b2335);border:1px solid var(--border,var(--line-2,#737f96));border-radius:12px}
 .result p,.source{white-space:pre-wrap;overflow-wrap:anywhere;margin:0 0 8px}
 .result button{padding:5px 10px;min-height:30px}
 .status,.hint,.error,.warning{margin:0;overflow-wrap:anywhere}
 .status{font-size:11px}.hint{font-size:10px;opacity:.75}.error{color:var(--danger,var(--err,#b62942))}.warning{font-size:11px;color:inherit}
 details{font-size:11px}summary{cursor:pointer}.text-label{margin-bottom:-6px}
</style>
