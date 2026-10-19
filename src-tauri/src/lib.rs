// Écho — point d'entrée de la bibliothèque Tauri.
// Structure conforme au plan d'architecture (§8.1) :
// commands / capture / transcription / notes / watcher / tray / shortcuts.
//
// Squelette : certains modules volontairement non branchés (dead_code autorisé
// pour les stubs capture/transcription/watcher en attendant la v1).
#![allow(dead_code)]

mod atomic;
mod capture;
mod commands;
mod notes;
mod recovery;
mod shortcuts;
mod transcription;
mod tray;
mod watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            tray::init(app.handle())?;
            shortcuts::init(app.handle())?;
            // Rejeu du journal de récupération avant toute interaction : une
            // entrée confirmée à l'écran avant un arrêt brutal revient dans sa
            // note ici, sans doublon.
            match commands::replay_pending(app.handle()) {
                Ok(0) => {}
                Ok(n) => eprintln!("journal de récupération : {n} entrée(s) rejouée(s)"),
                Err(e) => eprintln!("rejeu du journal impossible : {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::select_notes_folder,
            commands::append_to_today_note,
            commands::start_capture,
            commands::stop_capture,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Écho");
}
