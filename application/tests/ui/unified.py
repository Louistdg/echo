import os, json, threading, functools, base64
from pathlib import Path
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[2]/'build'
OUT=Path(os.environ.get('ECHO_TEST_OUTPUT',str(ROOT.parent/'test-results')))
OUT.mkdir(parents=True,exist_ok=True)
class Handler(SimpleHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self):
  if self.path.split('?')[0] in ('/paste','/quick'): self.path='/index.html'
  super().do_GET()
server=ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Handler,directory=str(ROOT)))
threading.Thread(target=server.serve_forever,daemon=True).start()
base=f'http://127.0.0.1:{server.server_port}'
MOCK=r'''
window.isTauri=true; window.__calls=[]; window.__dir=localStorage.getItem('__mock_dir')||'D:\\Notes Écho'; window.__chosen='D:\\Autres notes';
window.__callbacks={}; window.__cb=0;
window.__TAURI_INTERNALS__={transformCallback:(fn)=>{window.__callbacks[++window.__cb]=fn;return window.__cb;},unregisterCallback:()=>{},metadata:{currentWindow:{label:location.pathname.includes('paste')?'paste':'quick'},currentWebview:{label:location.pathname.includes('paste')?'paste':'quick'}},invoke:async(cmd,args={})=>{
 window.__calls.push({cmd,args});
 if(cmd==='start_capture')return null;
 if(cmd==='get_dictation_target')return 'chatgpt-window';
 if(cmd==='insert_dictation_text'){if(window.__failInsert)throw 'Fenêtre cible fermée';window.__inserted=(window.__inserted||[]).concat(args.text);return null;}
 if(cmd==='stop_capture')return {path:'D:\\Notes Écho\\.echo\\audio\\dictation-'+(window.__take=(window.__take||0)+1)+'.wav',duration_secs:3,samples:48000};
 if(cmd==='transcribe_recording') {
  if(window.__failTranscription)throw 'Moteur temporairement indisponible';
  if(window.__holdTranscription)await new Promise(r=>window.__releaseTranscription=r);
  return window.__transcript || 'Ce genre de note reste intact.';
 }
 if(cmd==='pick_shelf_item')return args.folder?'D:\\Dossier à partager':'D:\\document.pdf';
 if(cmd==='validate_shelf_item')return args.path;
 if(cmd==='set_notes_folder'){window.__dir=args.path;localStorage.setItem('__mock_dir',window.__dir);return args.path;}
 if(cmd==='get_drag_icon')return 'D:\\icon.png';
 if(cmd==='get_settings')return {notes_dir:window.__dir};
 if(cmd==='get_status')return {capture_audio:'prêt',transcription:'prêt',notes:'prêt',recovery:''};
 if(cmd==='list_recordings')return [];
 if(cmd==='select_notes_folder') {if(window.__holdFolder)await new Promise(r=>window.__releaseFolder=r);window.__dir=window.__chosen;localStorage.setItem('__mock_dir',window.__dir);return window.__dir;}
 if(cmd==='save_pasted_note') {
  if(args.notesDir!==window.__dir)throw 'Dossier modifié';
  const files=JSON.parse(localStorage.getItem('__test_files')||'{}');const key=args.notesDir+'\\reponse-'+args.entryId+'.md';
  if(files[key]!==undefined&&files[key]!==args.text&&files[key]!==args.expectedText)throw 'Conflit externe';
  files[key]=args.text;localStorage.setItem('__test_files',JSON.stringify(files));
  if(window.__holdReply)await new Promise(r=>window.__releaseReply=r);
  return key;
 }
 return null;
}};
'''

checks=[]
try:
 with sync_playwright() as p:
  browser=p.chromium.launch(executable_path=r'C:\Program Files\Google\Chrome\Application\chrome.exe',headless=True)
  ctx=browser.new_context(viewport={'width':360,'height':590});ctx.add_init_script(MOCK)
  page=ctx.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
  page.goto(base+'/quick');page.get_by_role('button',name='Ouvrir le bloc-notes',exact=True).wait_for()
  page.get_by_role('button',name='Ouvrir le bloc-notes',exact=True).click()
  page.locator('textarea:visible:enabled').wait_for()
  page.locator('textarea').fill('Texte existant')
  page.get_by_role('button',name='Enregistrer dans le dossier',exact=True).click()
  page.get_by_text('Enregistré dans ton dossier',exact=True).wait_for()
  page.get_by_role('button',name='Bloc-notes',exact=True).click()
  checks.append('Le bouton sous la note confirme une écriture réussie dans le dossier')
  page.wait_for_function('Object.keys(JSON.parse(localStorage.__test_files||"{}")).length===1')
  assert page.get_by_role('button',name='Fichier de la note, glisser vers une conversation',exact=True).is_visible()
  page.get_by_role('button',name='Fichier de la note, glisser vers une conversation',exact=True).dispatch_event('pointerdown',{'button':0})
  page.wait_for_function('window.__calls.some(c=>c.cmd==="plugin:drag|start_drag")')
  assert page.evaluate('window.__calls.find(c=>c.cmd==="plugin:drag|start_drag").args.item[0].endsWith(".md")')
  checks.append('La tuile de fichier utilise le chemin Markdown réellement sauvegardé pour le glissement (IPC simulé)')
  page.get_by_role('button',name='Fichiers à glisser',exact=True).click()
  page.get_by_role('button',name='+ Fichier',exact=True).click()
  page.get_by_role('button',name='+ Dossier',exact=True).click()
  page.get_by_role('button',name='Glisser document.pdf',exact=True).wait_for()
  page.get_by_role('button',name='Glisser Dossier à partager',exact=True).wait_for()
  page.evaluate('''() => { const call=window.__calls.find(c=>c.cmd==='plugin:event|listen' && c.args.event==='tauri://drag-drop'); if(!call) throw Error('drag listener missing'); window.__callbacks[call.args.handler]({payload:{paths:['D:/Déposé depuis Explorer'],position:{x:20,y:20}}}); }''')
  page.get_by_role('button',name='Glisser Déposé depuis Explorer',exact=True).wait_for()
  checks.append('Un dépôt venu de l’Explorateur rejoint le plateau et reste disponible (événement natif simulé)')
  page.get_by_role('button',name='Fichiers à glisser',exact=True).click()
  checks.append('Fichier et dossier conservés dans le plateau à gauche du micro')
  page.evaluate('window.__holdTranscription=true')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).wait_for()
  assert page.get_by_role('button',name='Changer de dossier',exact=True).is_disabled()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.wait_for_function('!!window.__releaseTranscription')
  page.get_by_role('button',name='Choisir le thème',exact=True).click()
  page.get_by_role('button',name='Thème Océan',exact=True).click()
  assert page.locator('main.ocean').count()==1
  page.evaluate('window.__releaseTranscription()')
  page.wait_for_function('document.querySelector("textarea").value.includes("Ce genre de note reste intact.")')
  page.wait_for_function('(expected)=>Object.values(JSON.parse(localStorage.__test_files))[0]===expected',arg='Texte existant\n\nCe genre de note reste intact.')
  assert not page.evaluate('window.__calls.some(c=>c.cmd==="transcribe_and_append")')
  checks.append('Micro → arrêt → transcription → ajout au texte existant → sauvegarde, sans passage par une note audio (IPC simulé)')
  checks.append('Interface réactive pendant la transcription ; dossier verrouillé et mot « genre » conservé')
  page.evaluate('window.__holdTranscription=false; window.__failTranscription=true')
  page.get_by_role('button',name='Dicter du texte',exact=True).click();page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.get_by_role('button',name='Réessayer la transcription',exact=True).wait_for()
  assert page.get_by_role('button',name='Dicter du texte',exact=True).is_disabled()
  assert page.evaluate('!!localStorage.getItem("echo.dictation.pending.v1")')
  page.reload();page.get_by_role('button',name='Réessayer la transcription',exact=True).wait_for()
  page.get_by_role('button',name='Réessayer la transcription',exact=True).click()
  page.locator('textarea:visible').wait_for()
  page.wait_for_function('document.querySelector("textarea").value.split("Ce genre").length===3')
  checks.append('Échec conservé, reprise après rechargement et réessai sans perdre le texte précédent')
  page.get_by_role('button',name='Replier le bloc-notes',exact=True).click();assert not page.locator('textarea').is_visible()
  page.get_by_role('button',name='Ouvrir le bloc-notes',exact=True).click();assert 'Texte existant' in page.locator('textarea').input_value()
  assert not page.evaluate('window.__calls.some(c=>c.cmd==="open_paste_window")')
  checks.append('Une seule fenêtre ; replier et déplier garde le brouillon et le micro dans le même panneau')
  for w,h in [(280,340),(360,590),(640,480)]:
   page.set_viewport_size({'width':w,'height':h})
   assert page.evaluate('document.documentElement.scrollWidth')<=w
   assert page.evaluate('document.documentElement.scrollHeight')<=h
   note=page.locator('.bloc').bounding_box();micro=page.locator('.commandes').bounding_box()
   assert note['y']+note['height']<=micro['y']+1
  checks.append('Trois tailles développées sans débordement ni chevauchement du micro')
  page.get_by_role('button',name='Détacher du premier plan',exact=True).click()
  page.get_by_role('button',name='Réduire Écho',exact=True).click()
  page.get_by_role('button',name='Rouvrir Écho',exact=True).wait_for()
  assert not page.locator('textarea').is_visible()
  assert not page.get_by_role('button',name='Dicter du texte',exact=True).is_visible()
  page.set_viewport_size({'width':48,'height':48})
  page.screenshot(path=str(OUT/'echo-bouton-flottant.png'),omit_background=True)
  button=page.get_by_role('button',name='Rouvrir Écho',exact=True)
  button.dispatch_event('pointerdown',{'button':0,'screenX':100,'screenY':100})
  button.dispatch_event('pointermove',{'buttons':1,'screenX':115,'screenY':100})
  button.dispatch_event('click',{'detail':1})
  assert page.locator('main.bulle').count()==1
  assert page.evaluate('window.__calls.some(c=>c.cmd.includes("start_dragging"))')
  button.press('Enter')
  page.set_viewport_size({'width':360,'height':590})
  assert page.locator('textarea').is_visible()
  assert page.get_by_role('button',name='Dicter du texte',exact=True).is_visible()
  page.get_by_role('button',name='Replier le micro',exact=True).click()
  page.get_by_role('button',name='Replier le bloc-notes',exact=True).click()
  page.get_by_role('button',name='Rouvrir Écho',exact=True).click()
  assert page.locator('textarea').is_visible()
  assert not page.get_by_role('button',name='Dicter du texte',exact=True).is_visible()
  page.get_by_role('button',name='Afficher le micro',exact=True).click()
  page.get_by_role('button',name='Replier le bloc-notes',exact=True).click()
  page.get_by_role('button',name='Replier le micro',exact=True).click()
  page.reload()
  page.get_by_role('button',name='Rouvrir Écho',exact=True).click()
  assert not page.locator('textarea').is_visible()
  assert page.get_by_role('button',name='Dicter du texte',exact=True).is_visible()
  page.get_by_role('button',name='Ouvrir le bloc-notes',exact=True).click()
  checks.append('Bouton 48 px thématisé ; glissement sans réouverture ; restauration note seule, micro seul et les deux, y compris après rechargement')
  checks.append('Épingle unique ; réduction conserve le panneau en bouton flottant (IPC simulé)')
  page.set_viewport_size({'width':360,'height':590})
  for name in ['Aurore','Nuit','Océan','Forêt','Violet','Rose','Clair','Ambre']:
   page.get_by_role('button',name='Choisir le thème',exact=True).click()
   page.get_by_role('button',name='Thème '+name,exact=True).click()
   panel_color=page.locator('main').first.evaluate('(el)=>getComputedStyle(el).backgroundImage')
   page.get_by_role('button',name='Réduire Écho',exact=True).click()
   assert page.locator('main').first.evaluate('(el)=>getComputedStyle(el).backgroundImage')==panel_color
   page.get_by_role('button',name='Rouvrir Écho',exact=True).click()
  checks.append('Les huit thèmes conservent la même couleur dans le bouton flottant')
  page.evaluate('window.__holdTranscription=true')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.wait_for_function('!!window.__releaseTranscription')
  page.get_by_role('button',name='Réduire Écho',exact=True).click()
  page.evaluate('window.__releaseTranscription()')
  page.wait_for_function('!localStorage.getItem("echo.dictation.pending.v1")')
  assert page.get_by_role('button',name='Rouvrir Écho',exact=True).is_visible()
  page.get_by_role('button',name='Rouvrir Écho',exact=True).click()
  checks.append('La transcription terminée en miniature conserve le bouton et le texte')
  for number in range(7):
   page.evaluate('(number)=>window.__chosen=`D:\\Dossier ${number}`',number)
   page.get_by_role('button',name='Changer de dossier',exact=True).click()
   page.wait_for_function('(number)=>window.__dir===`D:\\Dossier ${number}`',arg=number)
  page.get_by_role('button',name='Dossiers récents',exact=True).click()
  assert page.locator('.recent button').count()==6
  page.locator('.recent button').nth(1).click()
  page.wait_for_function('window.__dir.includes("Dossier 5")')
  page.reload()
  page.get_by_role('button',name='Fichiers à glisser',exact=True).click()
  assert page.get_by_role('button',name='Glisser document.pdf',exact=True).is_visible()
  checks.append('Historique limité aux six derniers dossiers, réutilisable ; plateau conservé après relance')
  page.get_by_role('button',name='Fichiers à glisser',exact=True).click()
  page.get_by_text('Enregistré dans ton dossier',exact=True).wait_for()
  page.screenshot(path=str(OUT/'echo-bloc-unique.png'),omit_background=True)
  page.evaluate('window.__take=20;window.__holdTranscription=false;window.__transcript="bonjour je corrige ce prompt"')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.wait_for_function('document.querySelector("textarea").value.endsWith("Bonjour je corrige ce prompt.")')
  checks.append('Dictée ajoutée après arrêt, avec majuscule et point final sans retirer de mots')
  page.locator('textarea').evaluate('''(el)=>{ const start=el.value.lastIndexOf('corrige'); el.focus(); el.setSelectionRange(start,start+7); el.dispatchEvent(new Event('select',{bubbles:true})); }''')
  page.evaluate('window.__transcript="modifie"')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.wait_for_function('document.querySelector("textarea").value.includes("Bonjour je modifie ce prompt.")')
  checks.append('Une dictée remplace le passage sélectionné dans un prompt, sans ajouter de paragraphe')
  page.get_by_role('button',name='Application',exact=True).click()
  before=page.locator('textarea').input_value()
  page.evaluate('window.__transcript="bonjour dans chatgpt"')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.wait_for_function('window.__inserted?.at(-1)==="Bonjour dans chatgpt."')
  assert page.locator('textarea').input_value()==before
  assert page.evaluate('window.__calls.some(c=>c.cmd==="insert_dictation_text"&&c.args.target==="chatgpt-window")')
  checks.append('Destination Application : insertion dans la conversation ciblée, sans modifier le bloc-notes (IPC simulé)')
  page.evaluate('window.__failInsert=true;window.__transcript="texte non perdu"')
  page.get_by_role('button',name='Dicter du texte',exact=True).click()
  page.get_by_role('button',name='Arrêter et transcrire',exact=True).click()
  page.get_by_role('button',name='Insérer le texte conservé dans l\'application',exact=True).wait_for()
  assert page.evaluate('localStorage.getItem("echo.dictation.text.pending.v1")==="Texte non perdu."')
  assert page.locator('textarea').input_value()==before
  page.get_by_role('button',name='Garder le texte conservé dans le bloc-notes',exact=True).click()
  page.wait_for_function('document.querySelector("textarea").value.toLowerCase().includes("texte non perdu.")')
  checks.append('Cible fermée : texte conservé et transférable dans la note, sans perte (IPC simulé)')
  assert not errors,errors
  ctx.close();browser.close()
finally:
 server.shutdown();server.server_close()
(OUT/'verification-bloc-unique.json').write_text(json.dumps(checks,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(checks,ensure_ascii=True))
