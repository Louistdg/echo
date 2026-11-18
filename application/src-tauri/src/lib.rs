//! Écho V3 — isolated portable application.
mod atomic;
mod dock;
mod translation;
mod capture;
mod capture_ecran;
mod capture_v21;
mod dictation_target;
mod shortcuts;
mod tray;
mod ui;
mod proofreading;
mod voice_v21;
mod workspace;
use tauri::{Listener,Manager};
#[cfg_attr(mobile,tauri::mobile_entry_point)]
pub fn run(){
 let app=tauri::Builder::default()
 .plugin(tauri_plugin_single_instance::init(|app,_,_|{let _=ui::show_window(app.clone(),"quick".into());}))
 .plugin(tauri_plugin_opener::init())
 .plugin(tauri_plugin_global_shortcut::Builder::new().build())
 .plugin(tauri_plugin_dialog::init())
 .plugin(tauri_plugin_autostart::Builder::new().build())
 .setup(|app|{
  workspace::init(app.handle()).map_err(std::io::Error::other)?;
  // WebViews can invoke commands as soon as they load. Create them only after
  // the portable settings and workspace are available to every surface.
  for config in &app.config().app.windows {
   if config.label!="dock-panels"&&config.label!="translation-select"{tauri::WebviewWindowBuilder::from_config(app.handle(),config)?.build()?;}
  }
  if let Some(bar)=app.get_webview_window("quick"){bar.set_always_on_top(workspace::settings().map_err(std::io::Error::other)?.always_on_top)?;}
  dock::init(app.handle()).map_err(std::io::Error::other)?;translation::init(app.handle());
  dictation_target::init();voice_v21::init(app.handle());capture_v21::init();tray::init(app.handle())?;shortcuts::init(app.handle());
  let a=app.handle().clone();app.listen("voice:state",move|event|{if let Ok(v)=serde_json::from_str::<serde_json::Value>(event.payload()){if let Some(tray)=a.tray_by_id("echo-v3-tray"){let phase=v["phase"].as_str().unwrap_or("idle");let text=if v["microphone_open"].as_bool().unwrap_or(false)||["arming","listening","recording"].contains(&phase){"Écho V3 — microphone ouvert"}else{"Écho V3 — microphone fermé"};let _=tray.set_tooltip(Some(text));}}});
  // V3: charger le moteur à la demande ; le bouton des réglages permet un préchargement volontaire.
  Ok(())
 })
 .invoke_handler(tauri::generate_handler![
  dock::dock_get,dock::dock_sync,dock::dock_hide,dock::dock_reset,dock::dock_pin,
  translation::translation_catalog,translation::translate_text,translation::translation_stop,translation::translation_select_region,translation::translation_selection_state,translation::translation_commit_region,translation::translation_cancel_selection,translation::translation_screen,
  workspace::settings_get,workspace::settings_update,workspace::workspace_choose,workspace::workspace_open,
  workspace::notes_list,workspace::note_read,workspace::note_create,workspace::note_save,workspace::note_rename,workspace::note_trash,workspace::note_activate,
  workspace::assets_list,workspace::asset_preview,workspace::asset_open,workspace::clipboard_copy,
  capture_v21::capture_begin,capture_v21::capture_current,capture_v21::capture_select,capture_v21::capture_draft,capture_v21::capture_save,capture_v21::capture_close,capture_v21::capture_load,capture_v21::capture_resume,
  voice_v21::voice_get,voice_v21::voice_devices,voice_v21::voice_start,voice_v21::voice_stop,voice_v21::voice_cancel,voice_v21::voice_retry,voice_v21::voice_resolve,voice_v21::voice_correct_result,voice_v21::voice_preload,voice_v21::voice_preview_interest,
  dictation_target::get_dictation_target,dictation_target::insert_dictation_text,shortcuts::shortcuts_status,shortcuts::shortcuts_retry,
  proofreading::check_spelling,ui::show_window,ui::quick_resize,ui::app_request_quit,ui::app_quit_ready,ui::app_quit_abort
 ])
 .build(tauri::generate_context!()).expect("Écho V3 n’a pas pu démarrer");
 app.run(|app,event|{if let tauri::RunEvent::Exit=event{translation::shutdown(app);voice_v21::shutdown();}});
}
