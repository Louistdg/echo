// Raccourcis globaux (§7.1 du plan).
// Ctrl+Alt+Espace : affiche/masque la fenêtre flottante de dictée.
// Les raccourcis sont enregistrés côté Rust — aucune permission
// global-shortcut n'est donc nécessaire côté JavaScript.

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let toggle_dictation = Shortcut::new(
        Some(Modifiers::CONTROL | Modifiers::ALT),
        Code::Space,
    );

    app.global_shortcut()
        .on_shortcut(toggle_dictation, |app, _shortcut, event| {
            // Push-to-talk maintenu : on ne réagit qu'au relâchement pour v1.
            // (L'enregistrement audio arrive avec le module capture — TODO v1.)
            if event.state == tauri_plugin_global_shortcut::ShortcutState::Released {
                toggle_quick_window(app);
            }
        })
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;

    Ok(())
}

fn toggle_quick_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("quick") {
        match win.is_visible() {
            Ok(true) => {
                let _ = win.hide();
            }
            _ => {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
}
