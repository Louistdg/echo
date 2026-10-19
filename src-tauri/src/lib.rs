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
            // Une capture interrompue par un arrêt brutal laisse un `.part` :
            // on le purge. Les WAV finalisés, eux, ne sont jamais touchés.
            let purges = commands::purge_recording_parts(app.handle());
            if purges > 0 {
                eprintln!("capture : {purges} fichier(s) interrompu(s) purgé(s)");
            }
            // Rejeu du journal de récupération avant toute interaction : une
            // entrée confirmée à l'écran avant un arrêt brutal revient dans sa
            // note ici, sans doublon.
            match commands::replay_pending(app.handle()) {
                Ok(r) => {
                    if r.rejouees > 0 {
                        eprintln!("journal de récupération : {} entrée(s) rejouée(s)", r.rejouees);
                    }
                    let resume = r.resume();
                    if !resume.is_empty() {
                        eprintln!("journal de récupération : {resume}");
                    }
                }
                Err(e) => eprintln!("rejeu du journal impossible : {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::select_notes_folder,
            commands::append_to_today_note,
            commands::transcribe_and_append,
            commands::start_capture,
            commands::stop_capture,
            commands::pause_capture,
            commands::resume_capture,
            commands::list_recordings,
            commands::open_recording,
            commands::open_recordings_folder,
            commands::open_notes_folder,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Écho");
}
