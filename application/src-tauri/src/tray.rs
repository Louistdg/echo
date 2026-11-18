use tauri::{AppHandle,Emitter,menu::{Menu,MenuItem},tray::{TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState}};
pub fn init(app:&AppHandle)->tauri::Result<()>{
 let main=MenuItem::with_id(app,"main","Ouvrir les notes",true,None::<&str>)?;
 let quick=MenuItem::with_id(app,"quick","Afficher le socle",true,None::<&str>)?;
 let stop=MenuItem::with_id(app,"stop","Arrêter le microphone",true,None::<&str>)?;
 let capture=MenuItem::with_id(app,"capture","Capturer l’écran",true,None::<&str>)?;
 let resume=MenuItem::with_id(app,"resume","Reprendre la dernière capture",true,None::<&str>)?;
 let settings=MenuItem::with_id(app,"settings","Réglages",true,None::<&str>)?;
 let quit=MenuItem::with_id(app,"quit","Quitter Écho V3",true,None::<&str>)?;
 let menu=Menu::with_items(app,&[&main,&quick,&stop,&capture,&resume,&settings,&quit])?;
 let mut tray=TrayIconBuilder::with_id("echo-v3-tray").tooltip("Écho V3 — notes, voix et captures").menu(&menu).show_menu_on_left_click(false);
 if let Some(icon)=app.default_window_icon(){tray=tray.icon(icon.clone())}
 tray.on_tray_icon_event(|tray,event|{if matches!(event,TrayIconEvent::DoubleClick{button:MouseButton::Left,..}|TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}){let _=crate::ui::show_window(tray.app_handle().clone(),"quick".into());}})
 .on_menu_event(|app,event|{let id=event.id.as_ref();match id{
 "main"|"quick"|"settings"=>{let _=crate::ui::show_window(app.clone(),id.to_string());},
 "capture"=>{let a=app.clone();tauri::async_runtime::spawn(async move{if let Err(e)=crate::capture_v21::capture_begin(a.clone()).await{let _=a.emit("app:notice",e);}});},
 "resume"=>{let a=app.clone();tauri::async_runtime::spawn(async move{let _=crate::capture_v21::capture_resume(a).await;});},
 "stop"=>{let a=app.clone();tauri::async_runtime::spawn(async move{let _=crate::voice_v21::voice_stop(a).await;});},
 "quit"=>crate::ui::request_quit(app),_=>{}
 }}).build(app)?;Ok(())
}
