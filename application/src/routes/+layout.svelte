<script lang="ts">
import {onMount} from 'svelte';
import {getCurrentWindow} from '@tauri-apps/api/window';
import {listen,emit} from '@tauri-apps/api/event';
import {invoke} from '@tauri-apps/api/core';
import {materialPalettes,type Settings} from '$lib/api';
import '$lib/app.css';
let {children}=$props();
onMount(()=>{
  let alive=true;const disposers:(()=>void)[]=[];
  const keep=(off:()=>void)=>alive?disposers.push(off):off();
  const apply=(s:Pick<Settings,'theme'|'theme_intensity'>&Partial<Pick<Settings,'large_controls'|'reduce_motion'>>)=>{const root=document.documentElement;root.dataset.largeControls=String(s.large_controls===true);root.dataset.reduceMotion=String(s.reduce_motion===true);root.dataset.theme=s.theme;root.dataset.material=String(materialPalettes.some(t=>t.id===s.theme));root.style.setProperty('--theme-intensity',String(Math.max(0,Math.min(1,s.theme_intensity??.45))));localStorage.setItem('echo.v3.theme',s.theme);localStorage.setItem('echo.v3.appearance',JSON.stringify({theme:s.theme,theme_intensity:s.theme_intensity??.45,large_controls:s.large_controls??false,reduce_motion:s.reduce_motion??false}))};
  try{const cached=JSON.parse(localStorage.getItem('echo.v3.appearance')||'null');apply(cached||{theme:localStorage.getItem('echo.v3.theme')||'clair',theme_intensity:.45})}catch{apply({theme:'clair',theme_intensity:.45})}
  try{
    const native=getCurrentWindow();document.body.dataset.window=native.label;
    if(['settings','capture'].includes(native.label)){
      void native.onCloseRequested(async event=>{
        event.preventDefault();
        try{
          if(native.label==='capture')await invoke('capture_close');
          else await native.hide();
        }catch(problem){await emit('app:notice',String(problem));}
      }).then(keep);
    }
  }catch{document.body.dataset.window='preview'}
  invoke<Settings>('settings_get').then(s=>{if(alive)apply(s)}).catch(()=>{});
  listen<Settings>('settings:changed',e=>apply(e.payload)).then(keep);
  return()=>{alive=false;disposers.forEach(off=>off())};
});
</script>
{@render children()}
