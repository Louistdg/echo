//! Two linked, bounded native windows. The compact microphone never scrolls.
use std::{sync::{LazyLock,Mutex,atomic::{AtomicBool,AtomicU64,Ordering}},time::Duration};
use serde::{Deserialize,Serialize};
use tauri::{AppHandle,Emitter,Manager,LogicalSize,PhysicalPosition,WebviewUrl,WebviewWindow,WebviewWindowBuilder};
pub(crate) const BASE_W:f64=244.0;
pub(crate) const BASE_H:f64=64.0;
static STATE:LazyLock<Mutex<DockState>>=LazyLock::new(||Mutex::new(DockState::default()));
static PANEL_CREATE:Mutex<()>=Mutex::new(());
static SAVE_PENDING:AtomicBool=AtomicBool::new(false);
static SAVE_EPOCH:AtomicU64=AtomicU64::new(0);

#[derive(Clone,Debug,Serialize)]
pub struct DockState {pub width:f64,pub height:f64,pub visible:bool,pub action:Option<String>,pub revision:u64}
impl Default for DockState {fn default()->Self{Self{width:360.0,height:260.0,visible:false,action:None,revision:0}}}
#[derive(Deserialize)]
pub struct DockRequest {pub width:f64,pub height:f64,pub visible:bool,pub focus:Option<bool>,pub action:Option<String>}
#[derive(Serialize,Deserialize)]
struct Position {x:i32,y:i32}
#[derive(Clone,Copy,Debug)]
struct Area {left:i32,top:i32,right:i32,bottom:i32}

#[cfg(windows)]
fn work_area(w:&WebviewWindow)->Option<Area>{
 #[repr(C)]#[derive(Default)]struct Rect{left:i32,top:i32,right:i32,bottom:i32}
 #[repr(C)]struct Info{size:u32,monitor:Rect,work:Rect,flags:u32}
 #[link(name="user32")]unsafe extern "system" {fn MonitorFromWindow(hwnd:*mut std::ffi::c_void,flags:u32)->*mut std::ffi::c_void;fn GetMonitorInfoW(monitor:*mut std::ffi::c_void,info:*mut Info)->i32;}
 let hwnd=w.hwnd().ok()?.0;
 let mut info=Info{size:std::mem::size_of::<Info>() as u32,monitor:Rect::default(),work:Rect::default(),flags:0};
 unsafe{let monitor=MonitorFromWindow(hwnd as _,2);if GetMonitorInfoW(monitor,&mut info)==0{return None}}
 Some(Area{left:info.work.left,top:info.work.top,right:info.work.right,bottom:info.work.bottom})
}
#[cfg(not(windows))]
fn work_area(_w:&WebviewWindow)->Option<Area>{None}
fn area(w:&WebviewWindow)->Area{work_area(w).or_else(||w.current_monitor().ok().flatten().map(|m|Area{left:m.position().x,top:m.position().y,right:m.position().x+m.size().width as i32,bottom:m.position().y+m.size().height as i32})).unwrap_or(Area{left:0,top:0,right:1920,bottom:1040})}
fn bounded_position(a:Area,x:i32,y:i32,scale:f64)->Position{
 let max_x=(a.right-(BASE_W*scale).ceil() as i32).max(a.left);
 let max_y=(a.bottom-(BASE_H*scale).ceil() as i32).max(a.top);
 Position{x:x.clamp(a.left,max_x),y:y.clamp(a.top,max_y)}
}
fn bounded_size(width:f64,height:f64,a:Area,base_y:i32,scale:f64)->(f64,f64){
 let max_w=((a.right-a.left) as f64/scale).max(1.0).min(640.0);
 let max_h=((base_y-a.top) as f64/scale-8.0).max(1.0).min(1000.0);
 (width.clamp(280.0_f64.min(max_w),max_w),height.clamp(64.0_f64.min(max_h),max_h))
}
fn position_path()->std::path::PathBuf{crate::workspace::portable_root().join("configuration/dock-position.json")}
fn save_position(app:&AppHandle){
 SAVE_EPOCH.fetch_add(1,Ordering::SeqCst);
 if SAVE_PENDING.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){return}
 let a=app.clone();
 std::thread::spawn(move||{loop{
  let epoch=SAVE_EPOCH.load(Ordering::SeqCst);
  std::thread::sleep(Duration::from_millis(350));
  if SAVE_EPOCH.load(Ordering::SeqCst)!=epoch{continue}
  if let Some(w)=a.get_webview_window("quick"){if let Ok(p)=w.outer_position(){let path=position_path();if let Some(parent)=path.parent(){let _=std::fs::create_dir_all(parent);}if let Ok(raw)=serde_json::to_vec_pretty(&Position{x:p.x,y:p.y}){let _=crate::atomic::write_atomic(&path,&raw);}}}
  SAVE_PENDING.store(false,Ordering::SeqCst);
  if SAVE_EPOCH.load(Ordering::SeqCst)!=epoch&&SAVE_PENDING.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_ok(){continue}
  break
 }});
}
fn align(app:&AppHandle)->Result<(),String>{
 let base=app.get_webview_window("quick").ok_or("Socle indisponible")?;let Some(panel)=app.get_webview_window("dock-panels")else{return Ok(())};
 let state=STATE.lock().map_err(|_|"Disposition indisponible")?.clone();
 if !state.visible{return Ok(())}
 let scale=base.scale_factor().map_err(|e|e.to_string())?;let a=area(&base);let pos=base.outer_position().map_err(|e|e.to_string())?;
 let minimum_y=a.top+(180.0*scale).round() as i32;
 let pos=bounded_position(a,pos.x,pos.y.max(minimum_y),scale);
 if let Ok(old)=base.outer_position(){if old.x!=pos.x||old.y!=pos.y{base.set_position(PhysicalPosition::new(pos.x,pos.y)).map_err(|e|e.to_string())?;}}
 let (width,height)=bounded_size(state.width,state.height,a,pos.y,scale);
 panel.set_size(LogicalSize::new(width,height)).map_err(|e|e.to_string())?;
 let right=pos.x+(BASE_W*scale).round() as i32;
 let x=(right-(width*scale).round() as i32).clamp(a.left,(a.right-(width*scale).round() as i32).max(a.left));
 panel.set_position(PhysicalPosition::new(x,pos.y-((height+8.0)*scale).round() as i32)).map_err(|e|e.to_string())?;
 Ok(())
}
fn show_panel(window:&WebviewWindow,focus:bool)->Result<(),String>{
 if focus{window.show().map_err(|e|e.to_string())?;return window.set_focus().map_err(|e|e.to_string())}
 #[cfg(windows)]{let handle=window.hwnd().map_err(|e|e.to_string())?;unsafe{windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(handle.0 as _,windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);}}
 #[cfg(not(windows))]{window.show().map_err(|e|e.to_string())?;}
 Ok(())
}
fn create_panel(app:&AppHandle)->Result<WebviewWindow,String>{
 let _creation=PANEL_CREATE.lock().map_err(|_|"Ouverture des blocs indisponible")?;
 if let Some(w)=app.get_webview_window("dock-panels"){return Ok(w)}
 let pinned=crate::workspace::settings()?.always_on_top;
 WebviewWindowBuilder::new(app,"dock-panels",WebviewUrl::App("/dock-panels".into()))
 .title("Écho V3 — Blocs").inner_size(360.0,260.0).decorations(false).transparent(true).shadow(false).resizable(false)
 .visible(false).focused(false).skip_taskbar(true).always_on_top(pinned).build().map_err(|e|e.to_string())
}
pub fn init(app:&AppHandle)->Result<(),String>{
 let base=app.get_webview_window("quick").ok_or("Socle indisponible")?;
 // Tao 0.37.1: native shadow on a hidden frameless window adds a caption at first resize.
 // Use the CSS shadow and disable it before changing client-size constraints.
 base.set_shadow(false).map_err(|e|e.to_string())?;
 base.set_min_size(Some(LogicalSize::new(BASE_W,BASE_H))).map_err(|e|e.to_string())?;
 base.set_max_size(Some(LogicalSize::new(BASE_W,BASE_H))).map_err(|e|e.to_string())?;
 base.set_resizable(false).map_err(|e|e.to_string())?;
 base.set_size(LogicalSize::new(BASE_W,BASE_H)).map_err(|e|e.to_string())?;
 let a=area(&base);let scale=base.scale_factor().map_err(|e|e.to_string())?;
 let saved=std::fs::read(position_path()).ok().and_then(|raw|serde_json::from_slice::<Position>(&raw).ok());
 let p=saved.unwrap_or(Position{x:a.right-((BASE_W+20.0)*scale).round() as i32,y:a.bottom-((BASE_H+16.0)*scale).round() as i32});
 let p=bounded_position(a,p.x,p.y,scale);base.set_position(PhysicalPosition::new(p.x,p.y)).map_err(|e|e.to_string())?;
 let handle=app.clone();
 base.on_window_event(move|event|{if matches!(event,tauri::WindowEvent::Moved(_)|tauri::WindowEvent::ScaleFactorChanged{..}){let _=align(&handle);save_position(&handle);}});
 base.show().map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]pub fn dock_get()->Result<DockState,String>{STATE.lock().map(|s|s.clone()).map_err(|_|"Disposition indisponible".into())}
#[tauri::command]pub async fn dock_sync(app:AppHandle,request:DockRequest)->Result<DockState,String>{
 if !request.width.is_finite()||!request.height.is_finite(){return Err("Taille de bloc invalide".into())}
 if let Some(action)=request.action.as_deref(){if !["add","menu","live-auto","toggle-live","notes","controls","translation"].contains(&action){return Err("Bloc inconnu".into())}}
 let panel=if request.visible{Some(create_panel(&app)?)}else{app.get_webview_window("dock-panels")};
 let was_visible={
  let mut state=STATE.lock().map_err(|_|"Disposition indisponible")?;
  let before=state.visible;
  state.width=request.width.clamp(280.0,640.0);state.height=request.height.clamp(64.0,1000.0);state.visible=request.visible;
  if request.action.is_some(){state.action=request.action;state.revision=state.revision.wrapping_add(1);}
  before
 };
 if request.visible{
  // Opening at the very top of a monitor would leave no usable room above.
  if let Some(base)=app.get_webview_window("quick"){let a=area(&base);let scale=base.scale_factor().map_err(|e|e.to_string())?;let p=base.outer_position().map_err(|e|e.to_string())?;
   let minimum_y=a.top+(180.0*scale).round() as i32;
   if p.y<minimum_y{let bounded=bounded_position(a,p.x,minimum_y,scale);base.set_position(PhysicalPosition::new(bounded.x,bounded.y)).map_err(|e|e.to_string())?;}
  }
  align(&app)?;if let Some(w)=panel{show_panel(&w,request.focus.unwrap_or(false))?;}
 }else{
  if was_visible{crate::translation::shutdown(&app);let _=app.emit("dock:hidden",());}
  if let Some(w)=panel{w.hide().map_err(|e|e.to_string())?;}
 }
 let state=dock_get()?;let _=app.emit("dock:changed",&state);Ok(state)
}
#[tauri::command]pub fn dock_hide(app:AppHandle)->Result<(),String>{
 {let mut state=STATE.lock().map_err(|_|"Disposition indisponible")?;state.visible=false;state.action=None;state.revision=state.revision.wrapping_add(1);}
 crate::translation::shutdown(&app);
 let _=app.emit("dock:hidden",());let _=app.emit("dock:changed",dock_get()?);
 for label in ["dock-panels","quick"]{if let Some(w)=app.get_webview_window(label){w.hide().map_err(|e|e.to_string())?;}}Ok(())
}
#[tauri::command]pub fn dock_reset(app:AppHandle)->Result<(),String>{
 let _=std::fs::remove_file(position_path());let base=app.get_webview_window("quick").ok_or("Socle indisponible")?;let a=area(&base);let scale=base.scale_factor().map_err(|e|e.to_string())?;
 base.set_position(PhysicalPosition::new(a.right-((BASE_W+20.0)*scale).round() as i32,a.bottom-((BASE_H+16.0)*scale).round() as i32)).map_err(|e|e.to_string())?;align(&app)
}
#[tauri::command]pub async fn dock_pin(app:AppHandle,pinned:bool)->Result<crate::workspace::Settings,String>{
 crate::workspace::settings_update(app,serde_json::json!({"always_on_top":pinned})).await
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn geometry_respects_secondary_monitor_and_taskbar(){let a=Area{left:-1920,top:0,right:0,bottom:1040};let p=bounded_position(a,600,2000,1.0);assert_eq!((p.x,p.y),(-244,976));let (w,h)=bounded_size(1000.0,2000.0,a,p.y,1.0);assert_eq!((w,h),(640.0,968.0));}
 #[test]fn geometry_scales_and_does_not_cross_work_area(){let a=Area{left:0,top:40,right:1280,bottom:720};let p=bounded_position(a,2000,-2,1.5);assert_eq!((p.x,p.y),(914,40));assert_eq!(bounded_size(360.0,600.0,a,580,1.5),(360.0,352.0));}
}
