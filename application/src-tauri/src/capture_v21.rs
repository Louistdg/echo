//! Screen selection uses a frozen bitmap; annotations live beside an immutable source.
use std::{fs, io::Cursor, path::{Path, PathBuf}, sync::{LazyLock, Mutex}, time::Duration};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};
use base64::{Engine, engine::general_purpose::STANDARD};

static SESSION: LazyLock<Mutex<Option<Session>>> = LazyLock::new(|| Mutex::new(None));
static BUSY: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureDocument { pub id: String, pub source: String, pub width: u32, pub height: u32, pub workspace: String, pub document: Option<String> }
struct Session { doc: CaptureDocument, selection: bool, restore: Vec<String>, export_path: Option<PathBuf> }
#[derive(Serialize)]
pub struct SavedCapture { pub path: String }
#[derive(Serialize, Deserialize)]
struct Meta { id: String, workspace: String, width: u32, height: u32, export: Option<String> }

fn data_url(bytes: &[u8]) -> String { format!("data:image/png;base64,{}", STANDARD.encode(bytes)) }
fn encode(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut data = Cursor::new(Vec::new()); img.write_to(&mut data, image::ImageFormat::Png).map_err(|e| e.to_string())?; Ok(data.into_inner())
}
fn decode(data: &str) -> Result<image::RgbaImage, String> {
    if data.len() > 180_000_000 { return Err("Cette capture est trop volumineuse.".into()); }
    let raw = data.strip_prefix("data:image/png;base64,").unwrap_or(data);
    let bytes = STANDARD.decode(raw).map_err(|e| e.to_string())?;
    let reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let image = reader.decode().map_err(|e| format!("Image invalide : {e}"))?.to_rgba8();
    if image.width() == 0 || image.height() == 0 || u64::from(image.width())*u64::from(image.height()) > 60_000_000 { return Err("Dimensions de capture invalides ou trop grandes.".into()); }
    Ok(image)
}
fn session_dir(doc: &CaptureDocument) -> PathBuf { Path::new(&doc.workspace).join(".echo/captures").join(&doc.id) }
fn source_path(doc: &CaptureDocument) -> PathBuf { session_dir(doc).join("source.png") }
fn validate_scene(raw: &str) -> Result<(), String> {
    if raw.len() > 8_000_000 { return Err("Les annotations dépassent la taille autorisée.".into()); }
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("Annotations invalides : {e}"))?;
    if !value.is_object() { return Err("Annotations invalides".into()); } Ok(())
}
fn persist_meta(s: &Session) -> Result<(), String> {
    let meta = Meta { id:s.doc.id.clone(), workspace:s.doc.workspace.clone(), width:s.doc.width, height:s.doc.height, export:s.export_path.as_ref().map(|p| crate::workspace::display_path(p)) };
    let bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    crate::atomic::write_atomic(&session_dir(&s.doc).join("meta.json"), &bytes).map_err(|e| e.to_string())?;
    let config = crate::workspace::portable_root().join("configuration/current-capture.json");
    crate::atomic::write_atomic(&config, &bytes).map_err(|e| e.to_string())
}
fn save_source(s: &Session) -> Result<(), String> {
    let dir = session_dir(&s.doc); fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let raw = s.doc.source.strip_prefix("data:image/png;base64,").ok_or("Source invalide")?;
    crate::atomic::write_atomic(&source_path(&s.doc), &STANDARD.decode(raw).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    persist_meta(s)
}

#[cfg(windows)]
fn desktop_bounds() -> Result<(i32,i32,i32,i32),String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics,SM_XVIRTUALSCREEN,SM_YVIRTUALSCREEN,SM_CXVIRTUALSCREEN,SM_CYVIRTUALSCREEN};
    let (x,y,w,h) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN),GetSystemMetrics(SM_YVIRTUALSCREEN),GetSystemMetrics(SM_CXVIRTUALSCREEN),GetSystemMetrics(SM_CYVIRTUALSCREEN)) };
    if w <= 0 || h <= 0 || i64::from(w)*i64::from(h)>60_000_000 { return Err("La surface des écrans est trop grande pour cette capture.".into()); }
    Ok((x,y,w,h))
}
#[cfg(not(windows))]
fn desktop_bounds() -> Result<(i32,i32,i32,i32),String> { Err("Capture disponible sous Windows.".into()) }

fn restore(app: &AppHandle, labels: &[String]) { for label in labels { if let Some(w)=app.get_webview_window(label) { let _=w.show(); } } }

#[tauri::command]
pub async fn capture_begin(app: AppHandle) -> Result<(), String> {
    {
        let mut busy=BUSY.lock().map_err(|_| "Capture indisponible")?;
        if *busy { return Err("Une capture est déjà en préparation.".into()); }
        if app.get_webview_window("capture").is_some_and(|w|w.is_visible().unwrap_or(false)) { return Err("Une sélection est déjà ouverte. Termine-la ou appuie sur Échap.".into()); }
        if app.get_webview_window("retouche").is_some_and(|w| w.is_visible().unwrap_or(false)) { return Err("Ferme d’abord la capture en cours pour en créer une autre.".into()); }
        *busy=true;
    }
    let a=app.clone();
    let result=tauri::async_runtime::spawn_blocking(move || {
        let cfg=crate::workspace::settings()?;
        let mut hidden=vec![];
        for label in ["main","quick","dock-panels","settings","retouche"] { if let Some(w)=a.get_webview_window(label) { if w.is_visible().unwrap_or(false) { w.hide().map_err(|e| e.to_string())?; hidden.push(label.to_string()); } } }
        std::thread::sleep(Duration::from_millis(160));
        let captured=(|| {
            let (x,y,w,h)=desktop_bounds()?;
            let pixels=crate::capture_ecran::copier_zone(x,y,w,h)?;
            let img=image::RgbaImage::from_raw(w as u32,h as u32,pixels).ok_or("Capture invalide")?;
            let bytes=encode(&img)?;
            let doc=CaptureDocument {id:uuid::Uuid::new_v4().to_string(),source:data_url(&bytes),width:w as u32,height:h as u32,workspace:cfg.workspace,document:None};
            *SESSION.lock().map_err(|_| "Capture indisponible")?=Some(Session{doc,selection:true,restore:hidden.clone(),export_path:None});
            let overlay=a.get_webview_window("capture").ok_or("Fenêtre de sélection indisponible")?;
            overlay.set_fullscreen(false).map_err(|e| e.to_string())?;
            overlay.set_position(PhysicalPosition::new(x,y)).map_err(|e| e.to_string())?;
            overlay.set_size(PhysicalSize::new(w as u32,h as u32)).map_err(|e| e.to_string())?;
            overlay.emit("capture:ready",()).map_err(|e| e.to_string())?;
            overlay.show().map_err(|e| e.to_string())?; overlay.set_focus().map_err(|e| e.to_string())?; Ok(())
        })();
        if captured.is_err() { restore(&a,&hidden); }
        captured
    }).await.map_err(|e| e.to_string());
    if let Ok(mut busy)=BUSY.lock() { *busy=false; }
    result?
}

#[tauri::command]
pub fn capture_current() -> Result<Option<CaptureDocument>,String> { Ok(SESSION.lock().map_err(|_| "Capture indisponible")?.as_ref().map(|s| s.doc.clone())) }

pub fn crop_bounds(x:u32,y:u32,w:u32,h:u32,source_w:u32,source_h:u32)->Result<(),String> {
    if w<2 || h<2 || x.checked_add(w).is_none_or(|v|v>source_w) || y.checked_add(h).is_none_or(|v|v>source_h) { return Err("La sélection dépasse l’image ou est trop petite.".into()); } Ok(())
}

#[tauri::command]
pub async fn capture_select(app:AppHandle,id:String,x:u32,y:u32,width:u32,height:u32)->Result<(),String> {
    let a=app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot=SESSION.lock().map_err(|_|"Capture indisponible")?; let s=slot.as_mut().ok_or("Aucune capture")?;
        if s.doc.id!=id || !s.selection { return Err("Cette sélection n’est plus active.".into()); }
        crop_bounds(x,y,width,height,s.doc.width,s.doc.height)?;
        let original=decode(&s.doc.source)?; let img=image::imageops::crop_imm(&original,x,y,width,height).to_image();
        let mut candidate=Session{doc:s.doc.clone(),selection:false,restore:s.restore.clone(),export_path:None};
        candidate.doc.source=data_url(&encode(&img)?);candidate.doc.width=width;candidate.doc.height=height;
        save_source(&candidate)?;
        let labels=candidate.restore.clone();*s=candidate;drop(slot);
        if let Some(w)=a.get_webview_window("capture") { let _=w.hide(); }
        restore(&a,&labels);
        let editor=a.get_webview_window("retouche").ok_or("Éditeur indisponible")?;
        editor.emit("capture:ready",()).map_err(|e| e.to_string())?; editor.unminimize().map_err(|e| e.to_string())?; editor.show().map_err(|e| e.to_string())?; editor.set_focus().map_err(|e| e.to_string())?; Ok(())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn capture_draft(id:String,document:String)->Result<(),String> {
    validate_scene(&document)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot=SESSION.lock().map_err(|_|"Capture indisponible")?; let s=slot.as_mut().ok_or("Aucune capture")?;
        if s.doc.id!=id || s.selection { return Err("Cette capture n’est plus active.".into()); }
        crate::atomic::write_atomic(&session_dir(&s.doc).join("scene.json"),document.as_bytes()).map_err(|e|e.to_string())?;
        s.doc.document=Some(document); persist_meta(s)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn capture_save(app:AppHandle,id:String,image:String,document:String,copy:bool)->Result<SavedCapture,String> {
    validate_scene(&document)?;
    let result=tauri::async_runtime::spawn_blocking(move || {
        let img=decode(&image)?;
        let mut slot=SESSION.lock().map_err(|_|"Capture indisponible")?; let s=slot.as_mut().ok_or("Aucune capture")?;
        if s.doc.id!=id || s.selection { return Err("Cette capture n’est plus active.".into()); }
        crate::atomic::write_atomic(&session_dir(&s.doc).join("scene.json"),document.as_bytes()).map_err(|e|e.to_string())?; s.doc.document=Some(document);
        let path=s.export_path.clone().unwrap_or_else(|| Path::new(&s.doc.workspace).join("captures").join(format!("{}-{}.png",chrono::Local::now().format("%Y-%m-%d-%H%M%S"),s.doc.id)));
        let bytes=encode(&img)?;
        crate::atomic::write_atomic(&path,&bytes).map_err(|e|format!("Capture non enregistrée : {e}"))?;
        s.export_path=Some(path.clone()); persist_meta(s)?;
        if copy {
            let mut clipboard=arboard::Clipboard::new().map_err(|e|format!("Capture rangée, mais presse-papiers inaccessible : {e}"))?;
            clipboard.set_image(arboard::ImageData{width:img.width() as usize,height:img.height() as usize,bytes:img.into_raw().into()}).map_err(|e|format!("Capture rangée ; copie à réessayer : {e}"))?;
        }
        Ok(SavedCapture{path:crate::workspace::display_path(&path)})
    }).await.map_err(|e|e.to_string())?;
    let _=app.emit("notes:changed",()); result
}

#[tauri::command]
pub fn capture_close(app:AppHandle,window:tauri::WebviewWindow)->Result<(),String> {
    window.hide().map_err(|e|e.to_string())?;
    let mut slot=SESSION.lock().map_err(|_|"Capture indisponible")?;
    if window.label()=="capture" { if let Some(s)=slot.as_ref() { if s.selection { restore(&app,&s.restore); *slot=None; } } }
    Ok(())
}

fn load_meta(meta:Meta)->Result<Session,String> {
    if uuid::Uuid::parse_str(&meta.id).is_err() { return Err("Identifiant de capture invalide".into()); }
    let cfg=crate::workspace::settings()?;
    if meta.workspace!=cfg.workspace && !cfg.recent_workspaces.contains(&meta.workspace) { return Err("Le dossier de cette capture n’est pas disponible.".into()); }
    let mut doc=CaptureDocument{id:meta.id,source:String::new(),width:meta.width,height:meta.height,workspace:meta.workspace,document:None};
    let bytes=fs::read(source_path(&doc)).map_err(|e|e.to_string())?; doc.source=data_url(&bytes);
    let p=session_dir(&doc).join("scene.json"); if p.exists(){ let raw=fs::read_to_string(p).map_err(|e|e.to_string())?; validate_scene(&raw)?; doc.document=Some(raw); }
    let export_path=meta.export.map(PathBuf::from);
    // A persisted sidecar may only point inside its own workspace's captures directory.
    if let Some(p)=&export_path { if !p.starts_with(Path::new(&doc.workspace).join("captures")) { return Err("Destination de capture invalide.".into()); } }
    Ok(Session{doc,selection:false,restore:vec![],export_path})
}

pub fn init() {
    let p=crate::workspace::portable_root().join("configuration/current-capture.json");
    if let Ok(raw)=fs::read_to_string(p) { if let Ok(meta)=serde_json::from_str(&raw) { if let Ok(s)=load_meta(meta) { if let Ok(mut slot)=SESSION.lock(){ *slot=Some(s); } } } }
}

#[tauri::command]
pub async fn capture_resume(app:AppHandle)->Result<(),String> {
    let missing={let slot=SESSION.lock().map_err(|_|"Capture indisponible")?;if slot.as_ref().is_some_and(|s|s.selection){return Err("Termine la sélection ouverte ou appuie sur Échap.".into());}slot.is_none()};
    if missing {
        let raw=fs::read_to_string(crate::workspace::portable_root().join("configuration/current-capture.json")).map_err(|_|"Aucune capture à reprendre.")?;
        let meta=serde_json::from_str(&raw).map_err(|e|format!("Dernière capture illisible : {e}"))?;
        let recovered=load_meta(meta)?;
        let mut slot=SESSION.lock().map_err(|_|"Capture indisponible")?;
        if slot.is_none(){*slot=Some(recovered);}
    }
    let w=app.get_webview_window("retouche").ok_or("Éditeur indisponible")?; w.emit("capture:ready",()).map_err(|e|e.to_string())?; w.show().map_err(|e|e.to_string())?; w.set_focus().map_err(|e|e.to_string())
}

#[tauri::command]
pub async fn capture_load(app:AppHandle,path:String)->Result<(),String> {
    if app.get_webview_window("retouche").is_some_and(|w|w.is_visible().unwrap_or(false)){return Err("Ferme d’abord la capture en cours pour en ouvrir une autre.".into());}
    let p=crate::workspace::validated_asset(&path)?;
    if !p.extension().is_some_and(|e|e.eq_ignore_ascii_case("png")) { return Err("Choisis une capture PNG.".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        let root=crate::workspace::settings()?.workspace;
        let canonical=p.canonicalize().map_err(|e|e.to_string())?;
        let mut found=None;
        for entry in fs::read_dir(Path::new(&root).join(".echo/captures")).map_err(|e|e.to_string())?.flatten() {
            if let Ok(raw)=fs::read_to_string(entry.path().join("meta.json")) { if let Ok(meta)=serde_json::from_str::<Meta>(&raw) { if meta.export.as_ref().is_some_and(|v|Path::new(v).canonicalize().ok().as_ref()==Some(&canonical)) { found=Some(load_meta(meta)?); break; } } }
        }
        let session=if let Some(s)=found { s } else {
            let img=image::open(&p).map_err(|e|e.to_string())?.to_rgba8();
            let doc=CaptureDocument{id:uuid::Uuid::new_v4().to_string(),source:data_url(&encode(&img)?),width:img.width(),height:img.height(),workspace:root,document:None};
            let s=Session{doc,selection:false,restore:vec![],export_path:None}; save_source(&s)?; s
        };
        persist_meta(&session)?; *SESSION.lock().map_err(|_|"Capture indisponible")?=Some(session); Ok::<(),String>(())
    }).await.map_err(|e|e.to_string())??;
    capture_resume(app).await
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn bounds_reject_overflow_and_empty_crop(){ assert!(crop_bounds(0,0,0,5,100,100).is_err());assert!(crop_bounds(u32::MAX,0,3,3,100,100).is_err());assert!(crop_bounds(98,99,3,2,100,100).is_err());assert!(crop_bounds(0,0,100,100,100,100).is_ok()); }
    #[test] fn png_roundtrip_preserves_source_pixels(){let img=image::RgbaImage::from_pixel(3,3,image::Rgba([24,56,78,255]));let bytes=encode(&img).unwrap();let out=decode(&data_url(&bytes)).unwrap();assert_eq!(out,img);}
}
