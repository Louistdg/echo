use tauri::{AppHandle,Manager,LogicalSize};
#[tauri::command]pub fn show_window(app:AppHandle,label:String)->Result<(),String>{
 if !["main","quick","settings"].contains(&label.as_str()){return Err("Fenêtre inconnue".into())}
 if label=="quick"{show_quick_quiet(&app);return Ok(())}
 let w=app.get_webview_window(&label).ok_or("Fenêtre indisponible")?;w.unminimize().map_err(|e|e.to_string())?;w.show().map_err(|e|e.to_string())?;w.set_focus().map_err(|e|e.to_string())
}
pub fn show_quick_quiet(app:&AppHandle){follow_active_desktop(app);if let Some(w)=app.get_webview_window("quick"){
 #[cfg(windows)]{if let Ok(handle)=w.hwnd(){unsafe{windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(handle.0 as _,windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);}}}
 #[cfg(not(windows))]{let _=w.show();}
}}
// Move only our own companion windows to the desktop where the user is typing.
// Never switch Windows desktops or activate a window while the user is typing.
fn follow_active_desktop(app:&AppHandle){
 #[cfg(windows)]{
  use windows::Win32::{System::Com::{CoInitializeEx,CoUninitialize,CoCreateInstance,COINIT_APARTMENTTHREADED,CLSCTX_ALL},UI::Shell::{IVirtualDesktopManager,VirtualDesktopManager}};
  let anchor=unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()};
  if anchor.0.is_null(){return}
  unsafe{
   let initialized=CoInitializeEx(None,COINIT_APARTMENTTHREADED).is_ok();
   // An existing COM apartment remains usable even if its mode differs.
   if let Ok(manager)=CoCreateInstance::<_,IVirtualDesktopManager>(&VirtualDesktopManager,None,CLSCTX_ALL){
    if let Ok(desktop)=manager.GetWindowDesktopId(anchor){
     for label in ["quick","dock-panels"]{
      if let Some(window)=app.get_webview_window(label){if let Ok(handle)=window.hwnd(){let _=manager.MoveWindowToDesktop(windows::Win32::Foundation::HWND(handle.0),&desktop);}}
     }
    }
   }
   if initialized{CoUninitialize();}
  }
 }
 #[cfg(not(windows))]let _=app;
}
// Compatibility for older callers: the base always stays compact.
#[tauri::command]pub fn quick_resize(app:AppHandle,preset:Option<String>)->Result<(),String>{
 let _=preset;let w=app.get_webview_window("quick").ok_or("Socle indisponible")?;
 w.set_size(LogicalSize::new(crate::dock::BASE_W,crate::dock::BASE_H)).map_err(|e|e.to_string())
}
static QUIT:std::sync::LazyLock<std::sync::Mutex<Option<std::collections::HashSet<String>>>>=std::sync::LazyLock::new(||std::sync::Mutex::new(None));
pub fn request_quit(app:&AppHandle){
 use tauri::Emitter;crate::translation::shutdown(app);
 let pending=["main","retouche","dock-panels"].into_iter().filter(|label|app.get_webview_window(label).is_some()).map(str::to_string).collect();
 if let Ok(mut q)=QUIT.lock(){if q.is_some(){return}*q=Some(pending);}
 let _=app.emit("app:quit-request",());
}
#[tauri::command]pub fn app_request_quit(app:AppHandle){request_quit(&app);}
#[tauri::command]pub async fn app_quit_ready(app:AppHandle,window:tauri::WebviewWindow)->Result<(),String>{let should_exit={let mut q=QUIT.lock().map_err(|_|"Fermeture indisponible")?;if let Some(pending)=q.as_mut(){pending.remove(window.label());pending.is_empty()}else{false}};if should_exit{tauri::async_runtime::spawn_blocking(crate::voice_v21::shutdown).await.map_err(|e|e.to_string())?;app.exit(0);}Ok(())}

#[tauri::command]pub fn app_quit_abort(app:AppHandle){use tauri::Emitter;if let Ok(mut q)=QUIT.lock(){*q=None;}let _=app.emit("app:quit-cancelled",());}
