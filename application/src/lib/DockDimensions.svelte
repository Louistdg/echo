<script lang="ts">
import {emit} from '@tauri-apps/api/event';
import {loadLayout,validateLayout,dockKey,blockNames,type DockLayout} from '$lib/dock';
const initial=loadLayout(localStorage);
let baseline=$state<DockLayout>(initial);
let draft=$state<DockLayout>(JSON.parse(JSON.stringify(initial)));
let busy=$state(false);let notice=$state('');let error=$state('');
const changed=$derived(JSON.stringify(draft)!==JSON.stringify(baseline));
function refresh(){baseline=loadLayout(localStorage);draft=JSON.parse(JSON.stringify(baseline));notice='';error=''}
async function apply(){
 if(!changed||busy)return;busy=true;error='';notice='';
 try{
  if(JSON.stringify(loadLayout(localStorage))!==JSON.stringify(baseline))throw Error('Les blocs ont changé. Recharge la disposition avant de l’appliquer.');
  const next=validateLayout(draft);localStorage.setItem(dockKey,JSON.stringify(next));
  await emit('dock:layout-applied',next);baseline=next;draft=JSON.parse(JSON.stringify(next));notice='Dimensions appliquées.';
 }catch(e){error=String(e)}finally{busy=false}
}
</script>
<section class="dimensions" aria-label="Dimensions des blocs">
<h2>Des tailles faciles à régler</h2>
<p>Prépare les dimensions ici. Le socle et la colonne restent en place pendant tes réglages ; le bouton Appliquer les met à jour ensemble.</p>
<label for="column-width">Largeur de la colonne <strong>{draft.width} px</strong></label>
<input id="column-width" type="range" min="280" max="640" step="10" bind:value={draft.width} disabled={busy}/>
{#each draft.blocks as block (block.id)}
<label for={'height-'+block.id}>{blockNames[block.id]} <strong>{block.height} px</strong></label>
<input id={'height-'+block.id} aria-label={'Hauteur '+blockNames[block.id]} type="range" min="120" max="640" step="10" bind:value={block.height} disabled={busy}/>
{/each}
{#if !draft.blocks.length}<p>Ouvre le bloc-notes ou ajoute un outil depuis + pour régler sa hauteur ici.</p>{/if}
<div class="actions"><button class="btn primary" disabled={!changed||busy} onclick={apply}>Appliquer les dimensions</button><button class="btn" disabled={busy} onclick={refresh}>Recharger la disposition</button></div>
{#if notice}<p role="status">{notice}</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}
</section>
<style>
.dimensions{max-width:650px}h2{font-size:23px;margin:0 0 14px}p{font-size:14px;line-height:1.7;color:var(--muted)}label{display:flex;justify-content:space-between;gap:16px;margin-top:24px;font-size:14px}strong{font-variant-numeric:tabular-nums}input{display:block;width:100%;height:44px;accent-color:var(--accent)}.actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:28px}.error{color:var(--danger)}
</style>
