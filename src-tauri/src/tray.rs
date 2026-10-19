// Icône de zone de notification (tray).
// Conformément au plan (§2.2), le tray est créé côté Rust et NON déclaré
// dans tauri.conf.json (les deux ne doivent pas coexister).

use tauri::AppHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let ouvrir = MenuItem::with_id(app, "ouvrir", "Ouvrir Écho", true, None::<&str>)?;
    let dictee = MenuItem::with_id(app, "dictee", "Nouvelle dictée", true, None::<&str>)?;
    let quitter = MenuItem::with_id(app, "quitter", "Quitter", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&ouvrir, &dictee, &quitter])?;

    TrayIconBuilder::with_id("echo-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Écho — notes vocales")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "ouvrir" => show_window(app, "main"),
            "dictee" => show_window(app, "quick"),
            "quitter" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn show_window(app: &AppHandle, label: &str) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
