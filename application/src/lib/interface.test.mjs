import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {applySuggestion} from './proofreading.ts';
test('proofreading uses UTF16 offsets, preserves emoji and accepts one explicit suggestion',()=>{
 const source='😀 Je suis aller au bureau.';
 const offset=source.indexOf('aller');
 const match={id:'one',offset,length:5,original:'aller'};
 assert.equal(applySuggestion(source,source,match,'allé'),'😀 Je suis allé au bureau.');
 assert.equal(applySuggestion(source+'!',source,match,'allé'),null);
 assert.equal(applySuggestion(source,source,{...match,offset:offset+1},'allé'),null);
 assert.equal(applySuggestion(source,source,{...match,original:'autre'},'allé'),null);
});
test('a replacement preserves negation, numbers and repeated words outside the selected match',()=>{
 const source='Je ne peux pas traiter 22 dossiers, très très précis.';
 const match={id:'one',offset:source.indexOf('précis'),length:6,original:'précis'};
 assert.equal(applySuggestion(source,source,match,'précis'),'Je ne peux pas traiter 22 dossiers, très très précis.');
});
function luminance(hex){const values=hex.match(/[a-f0-9]{2}/gi).map(x=>parseInt(x,16)/255).map(x=>x<=.04045?x/12.92:((x+.055)/1.055)**2.4);return values[0]*.2126+values[1]*.7152+values[2]*.0722}
function contrast(a,b){const values=[luminance(a),luminance(b)].sort((a,b)=>b-a);return(values[0]+.05)/(values[1]+.05)}
test('all 36 offline material themes define legible functional roles',()=>{
 const palettes=JSON.parse(readFileSync(new URL('./material-palettes.json',import.meta.url),'utf8'));
 const css=readFileSync(new URL('./material-themes.css',import.meta.url),'utf8');
 assert.equal(palettes.length,36);assert.equal(new Set(palettes.map(x=>x.id)).size,36);
 assert.equal(new Set(palettes.map(x=>x.family)).size,4);
 let pairs=0;
 for(const palette of palettes){
  const rule=css.split("\n").find(line=>line.startsWith("[data-theme='"+palette.id+"']"));
  assert.ok(rule,palette.id);const roles=Object.fromEntries([...rule.matchAll(/--([a-z-]+):(#[a-f0-9]{6})/gi)].map(m=>[m[1],m[2]]));
  for(const fg of ['text','muted','accent'])for(const bg of ['bg','surface','sidebar']){assert.ok(contrast(roles[fg],roles[bg])>=4.5,palette.id+' '+fg+'/'+bg);pairs++}
  for(const [fg,bg] of [['on-accent','accent'],['on-danger','danger'],['danger','danger-soft']]){assert.ok(contrast(roles[fg],roles[bg])>=4.5,palette.id+' '+fg+'/'+bg);pairs++}
  assert.ok(readFileSync(new URL('../../static/themes/'+palette.texture+'.png',import.meta.url)).length>1000);
 }
 assert.equal(pairs,432);
});


test('single instant voice UI stays compact and has no legacy quality selector',()=>{
 const quick=readFileSync(new URL('../routes/quick/+page.svelte',import.meta.url),'utf8');
 const settings=readFileSync(new URL('../routes/settings/+page.svelte',import.meta.url),'utf8');
 const tauri=JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json',import.meta.url),'utf8'));
 assert.ok(quick.includes("open('live-auto',false)"));
 assert.ok(quick.includes('voice_preload'));
 assert.ok(quick.includes("mode:'toggle'"));
 assert.equal(quick.includes("mode:'handsfree'"),false);
 assert.ok(quick.includes("await run('voice_start',{kind:'dictation',mode:'toggle'});if(!error)void open('live-auto',false)"));
 for(const legacy of ['Crosshair','Zap','qualityName','changeQuality','Rapide · base','Précise · small']){
  assert.equal(quick.includes(legacy),false,legacy+' remains in quick');
  assert.equal(settings.includes(legacy),false,legacy+' remains in settings');
 }
 const voiceWindow=tauri.app.windows.find(w=>w.label==='quick');
 assert.equal(voiceWindow.width,244);
 assert.equal(voiceWindow.minWidth,244);
 assert.equal(voiceWindow.maxWidth,244);
});
test('notes surfaces use the same toggle dictation and dock note has quick search',()=>{
 const main=readFileSync(new URL('../routes/+page.svelte',import.meta.url),'utf8');
 const dockNote=readFileSync(new URL('./DockNote.svelte',import.meta.url),'utf8');
 assert.ok(main.includes("voice_start', { kind: 'dictation', mode: 'toggle' }"));
 assert.equal(main.includes("voice_start', { kind: 'dictation', mode: 'handsfree' }"),false);
 assert.ok(dockNote.includes('id="dock-note-search"'));
 assert.ok(dockNote.includes('visibleNotes'));
 assert.ok(dockNote.includes('Chercher une note…'));
});

test('translation quick actions and capture resume stay directly accessible',()=>{
 const translation=readFileSync(new URL('./TranslatePanel.svelte',import.meta.url),'utf8');
 const dock=readFileSync(new URL('../routes/dock-panels/+page.svelte',import.meta.url),'utf8');
 assert.ok(translation.includes('async function paste()'));
 assert.ok(translation.includes('function clearInput()'));
 assert.ok(translation.includes('>Coller</button>'));
 assert.ok(translation.includes('>Effacer</button>'));
 assert.ok(dock.includes("menu('resume-capture')"));
 assert.ok(dock.includes("if(action==='resume-capture'){await invoke('capture_resume');return}"));
 assert.ok(dock.includes('Reprendre la dernière capture'));
});
test('settings show engine state immediately',()=>{
 const settings=readFileSync(new URL('../routes/settings/+page.svelte',import.meta.url),'utf8');
 assert.ok(settings.includes("invoke<VoiceState>('voice_get')"));
 assert.ok(settings.includes('engineText'));
});
test('automatic live preview temporarily takes priority over large persisted blocks',()=>{
 const dock=readFileSync(new URL('../routes/dock-panels/+page.svelte',import.meta.url),'utf8');
 assert.ok(dock.includes("return[live,...list.filter(b=>b.id!=='live')]"));
 assert.ok(dock.includes('modulesEl?.scrollTo({top:0'));
 assert.ok(dock.includes('<div class="modules" bind:this={modulesEl}>'));
});
