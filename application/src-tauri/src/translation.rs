//! Local translation and explicit screen-zone following. No capture is written to disk.
use std::{collections::hash_map::DefaultHasher,hash::{Hash,Hasher},io::{BufRead,BufReader,Read,Write},path::PathBuf,process::{Child,ChildStdin,Command,Stdio},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU64,Ordering},mpsc::{self,Receiver}},time::{Duration,Instant}};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use tauri::{AppHandle,Emitter,Manager,PhysicalPosition,PhysicalSize,WebviewUrl,WebviewWindow};
use base64::Engine as _;
const MAX_LINE:usize=4_000_000;
static EPOCH:AtomicU64=AtomicU64::new(1);
static WORKER:Mutex<Option<Arc<Worker>>>=Mutex::new(None);
static SERIAL:Mutex<()>=Mutex::new(());
static SELECT:Mutex<Option<(String,Bounds)>>=Mutex::new(None);
static REGION:Mutex<Option<Region>>=Mutex::new(None);
static CACHE:Mutex<Option<(String,u64,String,String,String,Value)>>=Mutex::new(None);
static STARTED:AtomicBool=AtomicBool::new(false);
struct Worker{#[cfg(windows)] _job:OwnedJob,child:Mutex<Child>,input:Mutex<ChildStdin>,output:Mutex<Receiver<Value>>,alive:AtomicBool,last:Mutex<Instant>}

#[cfg(windows)]
struct OwnedJob(isize);
#[cfg(windows)]
impl OwnedJob{
 fn new(child:&Child)->Result<Self,String>{
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::System::JobObjects::{SetInformationJobObject,AssignProcessToJobObject,JobObjectExtendedLimitInformation,JOBOBJECT_EXTENDED_LIMIT_INFORMATION,JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
  #[link(name="kernel32")]unsafe extern "system"{#[link_name="CreateJobObjectW"]fn create_job(attrs:*const std::ffi::c_void,name:*const u16)->*mut std::ffi::c_void;}
  unsafe{
   let handle=create_job(std::ptr::null(),std::ptr::null());if handle.is_null(){return Err("Isolation du moteur local impossible".into())}
   let job=Self(handle as isize);let mut info:JOBOBJECT_EXTENDED_LIMIT_INFORMATION=std::mem::zeroed();
   info.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
   if SetInformationJobObject(handle,JobObjectExtendedLimitInformation,&info as *const _ as _,std::mem::size_of_val(&info) as u32)==0||AssignProcessToJobObject(handle,child.as_raw_handle() as _)==0{return Err("Isolation du moteur et de son OCR refusée par Windows".into())}
   Ok(job)
  }
 }
}
#[cfg(windows)]impl Drop for OwnedJob{fn drop(&mut self){unsafe{windows_sys::Win32::Foundation::CloseHandle(self.0 as _);}}}

impl Worker{fn kill(&self){self.alive.store(false,Ordering::SeqCst);#[cfg(windows)]unsafe{windows_sys::Win32::System::JobObjects::TerminateJobObject(self._job.0 as _,1);}if let Ok(mut c)=self.child.lock(){let _=c.kill();let _=c.wait();}}}
impl Drop for Worker{fn drop(&mut self){if let Ok(c)=self.child.get_mut(){let _=c.kill();let _=c.wait();}}}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
pub struct Region{pub x:i32,pub y:i32,pub width:u32,pub height:u32,pub token:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Bounds{pub x:i32,pub y:i32,pub width:u32,pub height:u32}
fn allowed(window:&WebviewWindow)->Result<(),String>{if window.label()=="dock-panels"{Ok(())}else{Err("Ouvre le bloc Traduire depuis la barre Écho.".into())}}
fn text_allowed(label:&str)->Result<(),String>{if ["main","dock-panels"].contains(&label){Ok(())}else{Err("Ouvre une note Écho pour traduire son texte.".into())}}
#[cfg(test)]mod note_window_tests{
 use super::text_allowed;
 #[test]fn text_translation_is_available_only_in_note_windows(){
  assert!(text_allowed("main").is_ok());assert!(text_allowed("dock-panels").is_ok());
  for label in ["settings","quick","translation-select",""]{assert!(text_allowed(label).is_err());}
 }
}
fn script(app:&AppHandle)->Result<PathBuf,String>{
 let mut dirs=vec![];
 if let Ok(exe)=std::env::current_exe(){if let Some(p)=exe.parent(){dirs.extend(p.ancestors().take(4).map(|p|p.to_path_buf()));}}
 if let Ok(p)=app.path().resource_dir(){dirs.push(p);}
 dirs.into_iter().map(|p|p.join("moteur/echo_translate.py")).find(|p|p.is_file()).ok_or("Le moteur de traduction local est absent de cette installation.".into())
}
fn ensure(app:&AppHandle)->Result<Arc<Worker>,String>{
 let mut slot=WORKER.lock().map_err(|_|"Traduction verrouillée")?;
 if let Some(w)=slot.as_ref(){if w.alive.load(Ordering::SeqCst){return Ok(w.clone())}}
 if let Some(w)=slot.take(){w.kill()}
 let script=script(app)?;
 let local=script.parent().unwrap().join("python/python.exe");
 
 let user=std::env::var_os("USERPROFILE").map(PathBuf::from).map(|p|p.join(".dsh/dsh-runtimes/dsh-primary-runtime/dependencies/python/python.exe"));
 let python=if local.is_file(){local}else{user.filter(|p|p.is_file()).ok_or("Python local de traduction introuvable. Réinstalle le moteur Écho.".to_string())?};
 let mut command=Command::new(python);
 command.args(["-X","utf8","-u"]).arg(script).arg("--server").env("PYTHONUTF8","1").env("PYTHONIOENCODING","utf-8").env("HF_HUB_OFFLINE","1").env("OMP_NUM_THREADS","2").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
 #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
 let mut child=command.spawn().map_err(|e|format!("Moteur de traduction indisponible : {e}"))?;
 #[cfg(windows)]let job=match OwnedJob::new(&child){Ok(j)=>j,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e)}};
 let input=child.stdin.take().ok_or("Entrée traduction absente")?;
 let output=child.stdout.take().ok_or("Sortie traduction absente")?;
 let(tx,rx)=mpsc::sync_channel(2);
 let w=Arc::new(Worker{#[cfg(windows)] _job:job,child:Mutex::new(child),input:Mutex::new(input),output:Mutex::new(rx),alive:AtomicBool::new(true),last:Mutex::new(Instant::now())});
 let weak=Arc::downgrade(&w);
 std::thread::spawn(move||{let mut reader=BufReader::new(output);loop{
  let mut bytes=Vec::new();let count=reader.by_ref().take((MAX_LINE+1) as u64).read_until(b'\n',&mut bytes);
  if count.is_err()||bytes.is_empty()||bytes.len()>MAX_LINE{break}
  let Ok(v)=serde_json::from_slice::<Value>(&bytes)else{break};
  if tx.send(v).is_err(){break}
 }if let Some(w)=weak.upgrade(){w.alive.store(false,Ordering::SeqCst);}});
 *slot=Some(w.clone());Ok(w)
}
fn request(app:&AppHandle,mut value:Value,epoch:u64)->Result<Value,String>{
 if EPOCH.load(Ordering::SeqCst)!=epoch{return Err("Traduction arrêtée".into())}
 if voice_busy(){return Err("Traduction en pause pendant la dictée.".into())}
 let w=ensure(app)?;
 if EPOCH.load(Ordering::SeqCst)!=epoch{w.kill();return Err("Traduction arrêtée".into())}
 let id=uuid::Uuid::new_v4().to_string();value["id"]=json!(id);
 let mut bytes=serde_json::to_vec(&value).map_err(|e|e.to_string())?;bytes.push(b'\n');
 let (tx,rx)=mpsc::channel();let writer=w.clone();
 std::thread::spawn(move||{let result=writer.input.lock().map_err(|_|"Entrée verrouillée".to_string()).and_then(|mut i|i.write_all(&bytes).and_then(|_|i.flush()).map_err(|e|e.to_string()));let _=tx.send(result);});
 if !matches!(rx.recv_timeout(Duration::from_secs(3)),Ok(Ok(()))){w.kill();return Err("Connexion au moteur interrompue".into())}
 let started=Instant::now();let output=w.output.lock().map_err(|_|"Sortie verrouillée")?;
 loop{
  if EPOCH.load(Ordering::SeqCst)!=epoch{return Err("Traduction arrêtée".into())}
  if voice_busy(){w.kill();return Err("Traduction en pause pendant la dictée.".into())}
  if started.elapsed()>Duration::from_secs(60){w.kill();return Err("La traduction a dépassé une minute. Réessaie avec moins de texte.".into())}
  match output.recv_timeout(Duration::from_millis(100)){
   Ok(v)=>{if EPOCH.load(Ordering::SeqCst)!=epoch{return Err("Traduction arrêtée".into())}if v["id"].as_str()!=Some(id.as_str()){w.kill();return Err("Réponse de traduction périmée".into())}
    *w.last.lock().map_err(|_|"Horloge indisponible")?=Instant::now();
    if v["ok"]!=true{return Err(v["error"].as_str().unwrap_or("Traduction impossible").to_string())}
    return Ok(v)
   },
   Err(mpsc::RecvTimeoutError::Timeout)=>{},
   Err(_)=>{w.kill();return Err("Le moteur local de traduction s’est arrêté.".into())}
  }
 }
}
pub fn stop(){EPOCH.fetch_add(1,Ordering::SeqCst);if let Ok(mut slot)=WORKER.lock(){if let Some(w)=slot.take(){w.kill();}}}
pub fn shutdown(app:&AppHandle){stop();let cancelled=SELECT.lock().map(|mut s|s.take().is_some()).unwrap_or(false);if let Some(w)=app.get_webview_window("translation-select"){let _=w.hide();}if cancelled{let _=app.emit("translation:selection-cancelled",());}}
pub fn init(app:&AppHandle){
 if STARTED.swap(true,Ordering::SeqCst){return}
 let app=app.clone();
 std::thread::spawn(move||loop{std::thread::sleep(Duration::from_secs(5));
  if let Ok(_busy)=SERIAL.try_lock(){if let Ok(mut slot)=WORKER.lock(){if slot.as_ref().is_some_and(|w|w.last.lock().is_ok_and(|t|t.elapsed()>Duration::from_secs(45))){if let Some(w)=slot.take(){w.kill();}}}}
  if app.get_webview_window("quick").is_none(){break}
 });
}
fn language(code:&str)->bool{["fr","en","nl","de","es","it"].contains(&code)}
fn validate_text(text:&str,from:&str,to:&str)->Result<(),String>{
 if !language(from)||!language(to){return Err("Choisis une des six langues proposées.".into())}
 if text.trim().is_empty(){return Err("Ajoute du texte à traduire.".into())}
 if text.chars().count()>10000{return Err("Limite : 10 000 caractères par traduction.".into())}Ok(())
}
#[tauri::command]pub async fn translation_catalog(app:AppHandle,window:WebviewWindow)->Result<Value,String>{
 allowed(&window)?;let epoch=EPOCH.load(Ordering::SeqCst);
 tauri::async_runtime::spawn_blocking(move||{let _guard=SERIAL.try_lock().map_err(|_|"Une traduction est déjà en cours")?;request(&app,json!({"op":"catalog"}),epoch)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub async fn translate_text(app:AppHandle,window:WebviewWindow,text:String,from:String,to:String)->Result<Value,String>{
 text_allowed(window.label())?;validate_text(&text,&from,&to)?;let epoch=EPOCH.load(Ordering::SeqCst);
 tauri::async_runtime::spawn_blocking(move||{let _guard=SERIAL.try_lock().map_err(|_|"Une traduction est déjà en cours")?;request(&app,json!({"op":"translate","text":text,"from":from,"to":to}),epoch)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]pub fn translation_stop(window:WebviewWindow)->Result<(),String>{text_allowed(window.label())?;stop();Ok(())}
fn bounds()->Bounds{
 #[cfg(windows)]{use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics,SM_XVIRTUALSCREEN,SM_YVIRTUALSCREEN,SM_CXVIRTUALSCREEN,SM_CYVIRTUALSCREEN};unsafe{Bounds{x:GetSystemMetrics(SM_XVIRTUALSCREEN),y:GetSystemMetrics(SM_YVIRTUALSCREEN),width:GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as u32,height:GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as u32}}}
 #[cfg(not(windows))]{Bounds{x:0,y:0,width:1920,height:1080}}
}
#[tauri::command]pub async fn translation_select_region(app:AppHandle,window:WebviewWindow)->Result<(),String>{
 allowed(&window)?;stop();let b=bounds();let token=uuid::Uuid::new_v4().to_string();
 *SELECT.lock().map_err(|_|"Sélection indisponible")?=Some((token.clone(),b.clone()));
 let w=if let Some(w)=app.get_webview_window("translation-select"){w}else{
 tauri::WebviewWindowBuilder::new(&app,"translation-select",WebviewUrl::App("/translation-select".into())).title("Écho — zone à traduire").decorations(false).shadow(false).transparent(true).always_on_top(true).skip_taskbar(true).resizable(false).visible(false).build().map_err(|e|e.to_string())?};
 w.set_position(PhysicalPosition::new(b.x,b.y)).map_err(|e|e.to_string())?;
 w.set_size(PhysicalSize::new(b.width,b.height)).map_err(|e|e.to_string())?;
 w.show().map_err(|e|e.to_string())?;w.set_focus().map_err(|e|e.to_string())?;let _=app.emit("translation:selection-opened",json!({"token":token,"bounds":b}));
 let a=app.clone();std::thread::spawn(move||{std::thread::sleep(Duration::from_secs(45));if let Ok(mut s)=SELECT.lock(){if s.as_ref().is_some_and(|(t,_)|t==&token){*s=None;if let Some(w)=a.get_webview_window("translation-select"){let _=w.hide();}let _=a.emit("translation:selection-cancelled",());}}});
 Ok(())
}
#[tauri::command]pub fn translation_selection_state(window:WebviewWindow)->Result<Value,String>{
 if window.label()!="translation-select"{return Err("Sélection indisponible".into())}
 let s=SELECT.lock().map_err(|_|"Sélection indisponible")?;let(t,b)=s.as_ref().ok_or("Sélection expirée")?;Ok(json!({"token":t,"bounds":b}))
}
fn region_from_fraction(b:&Bounds,token:String,x:f64,y:f64,width:f64,height:f64)->Result<Region,String>{
 if ![x,y,width,height].iter().all(|n|n.is_finite())||x<0.0||y<0.0||width<=0.0||height<=0.0||x+width>1.001||y+height>1.001{return Err("Zone invalide".into())}
 let r=Region{x:b.x+(x*b.width as f64).round() as i32,y:b.y+(y*b.height as f64).round() as i32,width:(width*b.width as f64).round() as u32,height:(height*b.height as f64).round() as u32,token};
 if r.width<12||r.height<12{return Err("Sélectionne une zone un peu plus grande.".into())}
 if u64::from(r.width)*u64::from(r.height)>4_000_000{return Err("Sélectionne une zone de texte plus petite (4 millions de pixels maximum).".into())}Ok(r)
}
#[tauri::command]pub fn translation_commit_region(app:AppHandle,window:WebviewWindow,token:String,x:f64,y:f64,width:f64,height:f64)->Result<(),String>{
 if window.label()!="translation-select"{return Err("Sélection indisponible".into())}
 let mut s=SELECT.lock().map_err(|_|"Sélection indisponible")?;let(t,b)=s.as_ref().ok_or("Sélection expirée")?;
 if t!=&token{return Err("Sélection expirée".into())}let r=region_from_fraction(b,token,x,y,width,height)?;
 *REGION.lock().map_err(|_|"Zone indisponible")?=Some(r.clone());*CACHE.lock().map_err(|_|"Cache indisponible")?=None;*s=None;window.hide().map_err(|e|e.to_string())?;
 app.emit("translation:region",r).map_err(|e|e.to_string())
}
#[tauri::command]pub fn translation_cancel_selection(app:AppHandle,window:WebviewWindow)->Result<(),String>{
 if window.label()!="translation-select"{allowed(&window)?}
 let existed=SELECT.lock().map_err(|_|"Sélection indisponible")?.take().is_some();if let Some(w)=app.get_webview_window("translation-select"){let _=w.hide();}
 if existed{app.emit("translation:selection-cancelled",()).map_err(|e|e.to_string())}else{Ok(())}
}
#[tauri::command]pub async fn translation_screen(app:AppHandle,window:WebviewWindow,region:Region,from:String,to:String,force:bool)->Result<Value,String>{
 allowed(&window)?;if !language(&from)||!language(&to){return Err("Langue inconnue".into())}
 if REGION.lock().map_err(|_|"Zone indisponible")?.as_ref()!=Some(&region){return Err("Sélectionne de nouveau la zone à traduire.".into())}
 let epoch=EPOCH.load(Ordering::SeqCst);
 tauri::async_runtime::spawn_blocking(move||{
  let _guard=SERIAL.try_lock().map_err(|_|"Une traduction est déjà en cours")?;
  std::thread::sleep(Duration::from_millis(150));
  if EPOCH.load(Ordering::SeqCst)!=epoch{return Err("Traduction arrêtée".into())}
  if voice_busy(){return Err("Traduction en pause pendant la dictée.".into())}
  let pixels=crate::capture_ecran::copier_zone(region.x,region.y,region.width as i32,region.height as i32)?;
  let mut hasher=DefaultHasher::new();pixels.hash(&mut hasher);let hash=hasher.finish();
  let previous=CACHE.lock().map_err(|_|"Cache indisponible")?.clone();
  if !force{if let Some((token,h,f,t,_,value))=&previous{if token==&region.token&&*h==hash&&f==&from&&t==&to{let mut v=value.clone();v["changed"]=json!(false);if EPOCH.load(Ordering::SeqCst)!=epoch||voice_busy(){return Err("Traduction arrêtée ou en pause".into())}return Ok(v)}}}
  let image=image::RgbaImage::from_raw(region.width,region.height,pixels).ok_or("Image invalide")?;
  let mut png=std::io::Cursor::new(Vec::new());image::DynamicImage::ImageRgba8(image).write_to(&mut png,image::ImageFormat::Png).map_err(|e|e.to_string())?;
  let ocr=request(&app,json!({"op":"ocr","language":match from.as_str(){"fr"=>"fr-FR","en"=>"en-US","nl"=>"nl-NL","de"=>"de-DE","es"=>"es-ES",_=>"it-IT"},"image_base64":base64::engine::general_purpose::STANDARD.encode(png.into_inner())}),epoch)?;
  let source=ocr["text"].as_str().unwrap_or("").trim().to_string();
  if source.is_empty(){if EPOCH.load(Ordering::SeqCst)!=epoch||voice_busy(){return Err("Traduction arrêtée ou en pause".into())}let value=json!({"ok":true,"changed":false,"source":"","text":"","message":"Aucun texte lisible dans cette zone.","ocr_language":ocr["language"],"ocr_warnings":ocr["warnings"]});*CACHE.lock().map_err(|_|"Cache indisponible")?=Some((region.token,hash,from,to,String::new(),value.clone()));return Ok(value)}
  validate_text(&source,&from,&to)?;
  let mut result=if let Some((token,_,f,t,old,value))=&previous{if !force&&token==&region.token&&f==&from&&t==&to&&old==&source{let mut v=value.clone();v["changed"]=json!(false);v}else{request(&app,json!({"op":"translate","text":source,"from":from,"to":to}),epoch)?}}else{request(&app,json!({"op":"translate","text":source,"from":from,"to":to}),epoch)?};
  result["source"]=json!(source);if result.get("changed").is_none(){result["changed"]=json!(true)}
  result["ocr_language"]=ocr["language"].clone();result["ocr_warnings"]=ocr["warnings"].clone();
  if EPOCH.load(Ordering::SeqCst)!=epoch||voice_busy(){return Err("Traduction arrêtée ou en pause".into())}
  *CACHE.lock().map_err(|_|"Cache indisponible")?=Some((region.token,hash,from,to,source,result.clone()));Ok(result)
 }).await.map_err(|e|e.to_string())?
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn languages_and_inputs_are_bounded(){assert!(validate_text("Été 42","fr","en").is_ok());assert!(validate_text("","fr","en").is_err());assert!(validate_text("Bonjour","auto","en").is_err());assert!(validate_text(&"a".repeat(12001),"fr","en").is_err());}
 #[test]fn region_maps_negative_monitor_origin_and_refuses_bad_dimensions(){let b=Bounds{x:-1920,y:-100,width:3840,height:1080};let r=region_from_fraction(&b,"id".into(),0.25,0.5,0.25,0.25).unwrap();assert_eq!((r.x,r.y,r.width,r.height),(-960,440,960,270));assert!(region_from_fraction(&b,"id".into(),f64::NAN,0.0,0.1,0.1).is_err());assert!(region_from_fraction(&b,"id".into(),0.9,0.0,0.2,0.5).is_err());assert!(region_from_fraction(&b,"id".into(),0.0,0.0,0.001,0.001).is_err());}
}

fn voice_busy()->bool{crate::voice_v21::is_busy()}
