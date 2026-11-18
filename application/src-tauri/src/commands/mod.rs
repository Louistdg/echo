// Commandes IPC exposées à l'UI (§8.2 du plan).
// Principe d'honnêteté : ce qui n'est pas implémenté retourne une erreur
// explicite plutôt qu'un succès factice.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::notes;
use crate::recovery::{Entry, Journal};
use crate::transcription::TranscriptionEngine;

/// Statut des modules — affiché dans l'UI pour être transparent sur l'avancement.
#[derive(Serialize)]
pub struct ModuleStatus {
    pub capture_audio: &'static str,
    pub transcription: &'static str,
    pub notes: &'static str,
    /// Résumé de la dernière récupération de journal (vide si tout va bien).
    pub recovery: String,
}

/// Dernier état de récupération, lisible par l'UI.
static RECOVERY_STATUS: std::sync::LazyLock<std::sync::Mutex<String>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(String::new()));

#[tauri::command]
pub fn get_status() -> ModuleStatus {
    ModuleStatus {
        capture_audio: "capture OK (M2) — WAV 16 kHz mono",
        transcription: "moteur whisper.cpp prêt si binaire + modèle configurés dans les réglages",
        notes: "note du jour + journal de récupération (rejeu au démarrage)",
        recovery: RECOVERY_STATUS
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default(),
    }
}

/// Paramètres persistés dans le répertoire applicatif (settings.json).
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub notes_dir: Option<PathBuf>,
    pub whisper_bin: Option<PathBuf>,
    pub whisper_model: Option<PathBuf>,
    /// Raccourci global qui affiche/masque la dictée (ex. « Ctrl+Alt+Space »).
    pub shortcut_dictation: Option<String>,
    /// Raccourci global qui ouvre la capture d'écran (ex. « Ctrl+Alt+S »).
    pub shortcut_capture: Option<String>,
}

/// Résultat d'un ajout : l'UI n'affiche « Enregistré » que si cette valeur est
/// renvoyée, donc seulement après journal + écriture dans la note.
#[derive(Serialize)]
pub struct AppendResult {
    pub id: String,
    pub path: String,
    pub state: String,
    pub text: String,
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

/// Lit un fichier de réglages en tolérant un BOM UTF-8 en tête.
///
/// Défaut corrigé le 19/10/2026 : un `settings.json` écrit par un éditeur
/// Windows (BOM) faisait échouer `serde_json` ; l'erreur était avalée et
/// l'utilisateur voyait ses réglages disparaître sans message.
pub fn read_settings_file(path: &Path) -> Settings {
    let Ok(bytes) = fs::read(path) else {
        return Settings::default();
    };
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).unwrap_or_default()
}

fn load_settings(app: &AppHandle) -> Settings {
    match settings_path(app) {
        Some(path) => read_settings_file(&path),
        None => Settings::default(),
    }
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app).ok_or("répertoire de configuration indisponible")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Jamais de BOM à l'écriture.
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

fn journal(app: &AppHandle) -> Result<Journal, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("répertoire applicatif indisponible : {e}"))?;
    Ok(Journal::new(dir.join("recovery.log")))
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    load_settings(&app)
}

/// (dictée, capture) effectifs — réglages ou valeurs par défaut.
pub fn reglages_courants(app: &AppHandle) -> (String, String) {
    let s = load_settings(app);
    (
        s.shortcut_dictation
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| crate::shortcuts::DICTEE_DEFAUT.to_string()),
        s.shortcut_capture
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| crate::shortcuts::CAPTURE_DEFAUT.to_string()),
    )
}

#[tauri::command]
pub fn get_shortcuts(app: AppHandle) -> (String, String) {
    reglages_courants(&app)
}

/// Enregistre de nouveaux raccourcis après les avoir validés.
#[tauri::command]
pub fn set_shortcuts(app: AppHandle, dictee: String, capture: String) -> Result<(String, String), String> {
    let dictee = dictee.trim().to_string();
    let capture = capture.trim().to_string();
    // validation avant d'écrire : un raccourci refusé ne doit pas être persisté
    crate::shortcuts::analyser_dictee(&dictee)?;
    crate::shortcuts::analyser(&capture)?;
    let mut s = load_settings(&app);
    s.shortcut_dictation = Some(dictee.clone());
    s.shortcut_capture = Some(capture.clone());
    save_settings(&app, &s)?;
    crate::shortcuts::enregistrer(&app, &dictee, &capture)?;
    Ok((dictee, capture))
}

/// Donne à la fenêtre une forme circulaire (mode bulle) : le carré du fond
/// WebView2 disparaît, il ne reste que le rond. En dehors de la bulle, la forme
/// est retirée (fenêtre rectangulaire normale).
fn appliquer_forme_bulle(window: &tauri::WebviewWindow, bulle: bool) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        type Hwnd = *mut c_void;
        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn CreateRoundRectRgn(g: i32, h: i32, d: i32, b: i32, l: i32, hh: i32) -> *mut c_void;
            fn DeleteObject(obj: *mut c_void) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetWindowRgn(hwnd: Hwnd, rgn: *mut c_void, redessiner: i32) -> i32;
            fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
            fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
            fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
        }
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct Rect { gauche: i32, haut: i32, droite: i32, bas: i32 }
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct Point { x: i32, y: i32 }
        if let Ok(poignee) = window.hwnd() {
            let hwnd = poignee.0 as Hwnd;
            if bulle {
                let mut client = Rect::default();
                let mut fenetre = Rect::default();
                let mut coin = Point::default();
                unsafe {
                    GetClientRect(hwnd, &mut client);
                    GetWindowRect(hwnd, &mut fenetre);
                    ClientToScreen(hwnd, &mut coin);
                }
                let l = client.droite - client.gauche;
                let h = client.bas - client.haut;
                // SetWindowRgn attend des coordonnées de la FENÊTRE : on décale
                // du coin client, sinon le rond est décalé et laisse un croissant.
                let dx = coin.x - fenetre.gauche;
                let dy = coin.y - fenetre.haut;
                if l > 0 && h > 0 {
                    unsafe {
                        let rgn = CreateRoundRectRgn(dx, dy, dx + l + 1, dy + h + 1, l, h);
                        if !rgn.is_null() {
                            SetWindowRgn(hwnd, rgn, 1);
                            DeleteObject(rgn);
                        }
                    }
                }
            } else {
                unsafe { SetWindowRgn(hwnd, std::ptr::null_mut(), 1); }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, bulle);
    }
}

#[derive(Serialize)]
pub struct CaptureInfo {
    pub name: String,
    pub path: String,
    pub taille: u64,
    pub modifie: String,
}

/// Captures récentes du dossier de notes, de la plus récente à la plus ancienne.
#[tauri::command]
pub fn lister_captures(app: AppHandle) -> Vec<CaptureInfo> {
    let Some(racine) = load_settings(&app).notes_dir else {
        return Vec::new();
    };
    let dossier = racine.join("captures");
    let Ok(lecture) = fs::read_dir(&dossier) else {
        return Vec::new();
    };
    let mut liste: Vec<CaptureInfo> = lecture
        .flatten()
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
        .filter_map(|e| {
            let metadonnees = e.metadata().ok()?;
            let modifie = metadonnees
                .modified()
                .ok()
                .map(|d| {
                    chrono::DateTime::<chrono::Local>::from(d)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            Some(CaptureInfo {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path().to_string_lossy().into_owned(),
                taille: metadonnees.len(),
                modifie,
            })
        })
        .collect();
    liste.sort_by(|a, b| b.name.cmp(&a.name));
    liste.truncate(60);
    liste
}

/// Capture une zone et renvoie l'image en base64 (data URL), pour l'éditeur.
/// Le calque est masqué le temps de la copie puis réaffiché.
#[tauri::command]
pub async fn capturer_zone_base64(
    app: AppHandle,
    x: f64,
    y: f64,
    largeur: f64,
    hauteur: f64,
    echelle: f64,
) -> Result<String, String> {
    // La fenêtre reste affichée mais la page se vide (transparente) : cacher la
    // fenêtre suspendrait la WebView et l'await ne reviendrait jamais.
    std::thread::sleep(std::time::Duration::from_millis(220));
    let echelle = if echelle > 0.0 { echelle } else { 1.0 };
    let px = (x * echelle).round() as i32;
    let py = (y * echelle).round() as i32;
    let pl = (largeur * echelle).round() as i32;
    let ph = (hauteur * echelle).round() as i32;
    let pixels = crate::capture_ecran::copier_zone(px, py, pl, ph)?;

    use base64::Engine;
    let image = image::RgbaImage::from_raw(pl as u32, ph as u32, pixels)
        .ok_or_else(|| "dimensions d'image invalides".to_string())?;
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| format!("encodage PNG impossible : {e}"))?;
    let code = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    Ok(format!("data:image/png;base64,{code}"))
}

/// Enregistre l'image annotée dans `<notes>/captures/` puis ferme le calque.
#[tauri::command]
pub async fn enregistrer_capture_base64(app: AppHandle, image: String) -> Result<String, String> {
    let racine = load_settings(&app)
        .notes_dir
        .ok_or("choisis un dossier de notes avant de capturer")?;
    let chemin = crate::capture_ecran::enregistrer_png_base64(&racine, &image)?;
    let texte = chemin.to_string_lossy().into_owned();
    if let Some(fenetre) = app.get_webview_window("capture") {
        let _ = fenetre.hide();
    }
    use tauri::Emitter;
    let _ = app.emit("capture:enregistree", texte.clone());
    Ok(texte)
}

/// Copie l'image annotée dans le presse-papiers Windows.
#[tauri::command]
pub fn copier_capture_base64(image: String) -> Result<(), String> {
    crate::capture_ecran::copier_image_base64(&image)
}

/// Ouvre le calque plein écran de capture d'écran.
#[tauri::command]
pub async fn ouvrir_capture(app: AppHandle) -> Result<(), String> {
    let fenetre = app.get_webview_window("capture").ok_or("fenêtre de capture introuvable")?;
    fenetre.unminimize().map_err(|e| e.to_string())?;
    fenetre.show().map_err(|e| e.to_string())?;
    fenetre.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn fermer_capture(app: AppHandle) -> Result<(), String> {
    if let Some(fenetre) = app.get_webview_window("capture") {
        fenetre.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Enregistre la zone dessinée (coordonnées CSS du calque) en PNG dans
/// `<notes>/captures/`. Le calque est masqué avant la copie pour ne pas le
/// capturer lui-même.
#[tauri::command]
pub async fn enregistrer_capture(
    app: AppHandle,
    x: f64,
    y: f64,
    largeur: f64,
    hauteur: f64,
    echelle: f64,
) -> Result<String, String> {
    let racine = load_settings(&app)
        .notes_dir
        .ok_or("choisis un dossier de notes avant de capturer")?;
    if let Some(fenetre) = app.get_webview_window("capture") {
        let _ = fenetre.hide();
    }
    std::thread::sleep(std::time::Duration::from_millis(160));
    let echelle = if echelle > 0.0 { echelle } else { 1.0 };
    let px = (x * echelle).round() as i32;
    let py = (y * echelle).round() as i32;
    let pl = (largeur * echelle).round() as i32;
    let ph = (hauteur * echelle).round() as i32;
    let pixels = crate::capture_ecran::copier_zone(px, py, pl, ph)?;
    let chemin = crate::capture_ecran::enregistrer_png(&racine, pl as u32, ph as u32, &pixels)?;
    let texte = chemin.to_string_lossy().into_owned();
    use tauri::Emitter;
    let _ = app.emit("capture:enregistree", texte.clone());
    Ok(texte)
}

/// Le bloc-notes et le micro partagent une seule fenêtre native.
struct PanelLayout { mode: usize, width: f64, heights: [f64; 3], bubble_position: Option<(i32, i32)> }
static PANEL_LAYOUT: std::sync::Mutex<PanelLayout> = std::sync::Mutex::new(
    PanelLayout { mode: 0, width: 320.0, heights: [150.0, 300.0, 400.0], bubble_position: None }
);

/// Rend la fenêtre non-activante (WS_EX_NOACTIVATE), comme la saisie vocale de
/// Windows : cliquer le micro ne vole plus le focus au champ où l'on écrit.
/// On l'enlève quand le bloc-notes est visible, sinon la frappe serait impossible.
fn appliquer_non_activante(window: &tauri::WebviewWindow, non_activante: bool) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        type Hwnd = *mut c_void;
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
            fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, valeur: isize) -> isize;
            fn SetWindowPos(hwnd: Hwnd, apres: Hwnd, x: i32, y: i32, cx: i32, cy: i32, drapeaux: u32) -> i32;
        }
        const GWL_EXSTYLE: i32 = -20;
        const WS_EX_NOACTIVATE: isize = 0x0800_0000;
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOZORDER: u32 = 0x0004;
        const SWP_FRAMECHANGED: u32 = 0x0020;

        if let Ok(poignee) = window.hwnd() {
            let hwnd = poignee.0 as Hwnd;
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let nouveau = if non_activante { style | WS_EX_NOACTIVATE } else { style & !WS_EX_NOACTIVATE };
                if nouveau != style {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, nouveau);
                    SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, non_activante);
    }
}

#[tauri::command]
pub async fn set_panel_layout(app: AppHandle, note_visible: bool, micro_visible: bool, bubble: bool, pinned: bool) -> Result<(), String> {
    if !note_visible && !micro_visible { return Err("aucune partie à restaurer".into()); }
    let window = app.get_webview_window("quick").ok_or("panneau introuvable")?;
    let mut layout = PANEL_LAYOUT.lock().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let mode = if bubble { 3 } else if note_visible && micro_visible { 2 } else if note_visible { 1 } else { 0 };
    let previous = layout.mode;
    if previous == 3 { layout.bubble_position = Some((position.x, position.y)); }
    else { layout.width = size.width as f64 / scale; layout.heights[previous] = size.height as f64 / scale; }
    let minimum = match mode { 3 => 48.0, 2 => 220.0, 1 => 180.0, _ => 130.0 };
    let mut width = if bubble { 48.0 } else { layout.width.max(260.0) };
    let mut height = if bubble { 48.0 } else { layout.heights[mode].max(minimum) };
    let monitor = window.current_monitor().map_err(|e| e.to_string())?;
    let (mut x, mut y) = (position.x, position.y);
    if let Some(ref monitor) = monitor {
        let area = monitor.work_area();
        width = width.min(area.size.width as f64 / scale);
        height = height.min(area.size.height as f64 / scale);
        if bubble {
            (x, y) = layout.bubble_position.unwrap_or((area.position.x + area.size.width as i32 - (64.0 * scale) as i32, position.y));
        } else if previous == 3 {
            x = position.x + size.width as i32 - (width * scale) as i32;
            y = position.y - (height * scale / 2.0) as i32;
        } else if previous != mode && mode != 1 {
            y += size.height as i32 - (height * scale) as i32;
        }
        x = x.clamp(area.position.x, (area.position.x + area.size.width as i32 - (width * scale) as i32).max(area.position.x));
        y = y.clamp(area.position.y, (area.position.y + area.size.height as i32 - (height * scale) as i32).max(area.position.y));
    }
    window.set_min_size(Some(tauri::LogicalSize::new(if bubble {48.0} else {260.0}, minimum))).map_err(|e| e.to_string())?;
    window.set_resizable(!bubble).map_err(|e| e.to_string())?;
    if bubble {
        // La WebView du mode bulle travaille en pixels physiques (dpr = 1) :
        // on dimensionne la fenêtre en physique pour que le rond remplisse tout.
        window.set_size(tauri::PhysicalSize::new(48, 48)).map_err(|e| e.to_string())?;
    } else {
        window.set_size(tauri::LogicalSize::new(width, height)).map_err(|e| e.to_string())?;
    }
    window.set_position(tauri::PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    window.set_skip_taskbar(bubble).map_err(|e| e.to_string())?;
    window.set_always_on_top(bubble || pinned).map_err(|e| e.to_string())?;
    // Mode dictée (sans bloc-notes) : la fenêtre ne prend JAMAIS le focus, comme
    // la saisie vocale de Windows (Win+H). Cliquer le micro ne coupe donc plus
    // le champ de texte où l'utilisateur écrit. Dès que le bloc-notes est
    // visible, la fenêtre redevient activable pour permettre la frappe.
    let _ = window.set_focusable(note_visible);
    appliquer_non_activante(&window, !note_visible);
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    // Le redimensionnement Windows n'est pas encore appliqué : on pose la forme
    // ronde tout de suite, puis de nouveau après 250 ms, sinon le masque garde
    // l'ancienne taille (et une forme ronde resterait sur le panneau).
    appliquer_forme_bulle(&window, bubble);
    {
        let fenetre = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(250));
            appliquer_forme_bulle(&fenetre, bubble);
        });
    }
    layout.mode = mode;
    Ok(())
}

#[tauri::command]
pub async fn open_paste_window(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let window = app.get_webview_window("quick").ok_or("panneau introuvable")?;
    // Le bloc-notes s'ouvre : la fenêtre doit redevenir activable pour la frappe.
    let _ = window.set_focusable(true);
    appliquer_non_activante(&window, false);
    window.emit("notes:open", ()).map_err(|e| e.to_string())?;
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

/// Transcription seule : le texte rejoint le bloc-notes, sans note vocale à gérer.
/// Le processus externe tourne hors du fil de l'interface.
#[tauri::command]
pub async fn transcribe_recording(app: AppHandle, path: String) -> Result<String, String> {
    let settings = load_settings(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let root = settings.notes_dir.ok_or("choisis un dossier avant de dicter")?;
        let directory = crate::capture::recordings_dir(&root).canonicalize().map_err(|e| e.to_string())?;
        let wav = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
        if !wav.starts_with(&directory) || wav.extension().and_then(|s| s.to_str()) != Some("wav") {
            return Err("ce fichier n'est pas une dictée d'Écho".into());
        }
        let engine = crate::transcription::WhisperCli::new(
            settings.whisper_bin.unwrap_or_default(), settings.whisper_model.unwrap_or_default(),
        );
        // Ne pas appliquer le nettoyage qui retire des mots tels que « genre ».
        let text = engine.transcribe(&wav)?;
        if text.trim().is_empty() { return Err("Aucun texte reconnu. Tu peux réessayer la transcription.".into()); }
        Ok(text)
    }).await.map_err(|e| format!("transcription interrompue : {e}"))?
}

/// Sauvegarde le collage dans le dossier affiché, jamais dans un autre dossier
/// choisi entre-temps depuis une autre fenêtre. L’écriture disque est déportée.
#[tauri::command]
pub async fn save_pasted_note(
    app: AppHandle,
    entry_id: String,
    text: String,
    expected_text: Option<String>,
    notes_dir: String,
) -> Result<String, String> {
    let root = load_settings(&app).notes_dir.ok_or("choisis un dossier de notes avant d’enregistrer")?;
    if root != PathBuf::from(&notes_dir) {
        return Err("le dossier de notes a changé ; choisis à nouveau le dossier de cette réponse".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::pasted_notes::save_pasted_note(&root, &entry_id, &text, expected_text.as_deref())
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("sauvegarde interrompue : {e}"))?
}

/// Ouvre un dialogue natif de choix du dossier de notes et persiste le choix.
/// Le dialogue bloquant tourne sur un thread dédié (spawn_blocking) pour ne
/// jamais bloquer le thread principal.
#[tauri::command]
pub async fn select_notes_folder(app: AppHandle) -> Result<String, String> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("erreur interne : {e}"))?;
    let Some(picked) = picked else {
        return Err("aucun dossier sélectionné".into());
    };
    let dir = picked.simplified().into_path().map_err(|e| e.to_string())?;
    let mut settings = load_settings(&app);
    settings.notes_dir = Some(dir.clone());
    save_settings(&app, &settings)?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn set_notes_folder(app: AppHandle, path: String) -> Result<String, String> {
    let dir = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if !dir.is_dir() { return Err("le dossier n'existe plus".into()); }
    let mut settings = load_settings(&app);
    settings.notes_dir = Some(dir.clone());
    save_settings(&app, &settings)?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn pick_shelf_item(app: AppHandle, folder: bool) -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        if folder { app.dialog().file().blocking_pick_folder() }
        else { app.dialog().file().blocking_pick_file() }
    }).await.map_err(|e| e.to_string())?;
    let Some(item) = selected else { return Ok(None); };
    let path = item.simplified().into_path().map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn validate_shelf_item(path: String) -> Result<String, String> {
    let path = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if !path.is_file() && !path.is_dir() { return Err("élément introuvable".into()); }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn open_shelf_item(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    let folder = if path.is_dir() { path } else { path.parent().ok_or("dossier introuvable")?.to_path_buf() };
    app.opener().open_path(folder.to_string_lossy().into_owned(), None::<&str>).map_err(|e| e.to_string())
}

/// Ajoute une entrée horodatée à la note du jour, avec journal de récupération.
///
/// Ordre imposé : (1) journal + fsync, (2) note en écriture atomique,
/// (3) marquage `done`. Une coupure entre (1) et (2) laisse un `pending` que le
/// démarrage suivant rejoue ; entre (2) et (3), le marqueur d'idempotence dans la
/// note empêche tout doublon.
/// Cœur d'ajout, partagé par la frappe et la dictée : journal → note → done.
/// L'identifiant est stable : rejouer ou retranscrire ne duplique pas.
pub fn append_text(app: &AppHandle, text: &str, id: &str) -> Result<AppendResult, String> {
    let settings = load_settings(app);
    let root = settings.notes_dir.ok_or(
        "aucun dossier de notes configuré — utilise « Choisir le dossier » d'abord",
    )?;

    let now = chrono::Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();

    let journal = journal(app)?;
    let entry = Entry::new(
        id,
        &root.to_string_lossy(),
        &date_str,
        &time_str,
        text,
    );

    journal
        .append(&entry)
        .map_err(|e| format!("journal non écrit, rien n'a été ajouté : {e}"))?;

    let path = notes::append_entry(&root, &date_str, &time_str, text, id).map_err(|e| {
        format!(
            "note non écrite, entrée conservée au journal et réessayée au prochain démarrage : {e}"
        )
    })?;

    let state = match journal.mark_done(&entry) {
        Ok(()) => "enregistré".to_string(),
        Err(_) => "enregistré (journal en attente de rejeu)".to_string(),
    };

    Ok(AppendResult {
        id: id.to_string(),
        path: path.to_string_lossy().into_owned(),
        state,
        text: text.to_string(),
    })
}

#[tauri::command]
pub fn append_to_today_note(
    app: AppHandle,
    text: String,
    entry_id: Option<String>,
) -> Result<AppendResult, String> {
    // Identifiant d'opération : l'interface le conserve entre deux tentatives
    // pour qu'un réessai après échec ne crée pas une seconde entrée.
    let id = entry_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    append_text(&app, &text, &id)
}

/// Transcrit un enregistrement puis l'ajoute à la note du jour.
/// Le texte suit le même chemin sûr que la frappe (journal → note → done),
/// avec un identifiant dérivé du nom du fichier : retranscrire ne duplique pas.
#[tauri::command]
pub fn transcribe_and_append(app: AppHandle, path: String) -> Result<AppendResult, String> {
    let settings = load_settings(&app);
    let root = settings.notes_dir.ok_or(
        "aucun dossier de notes configuré — utilise « Choisir le dossier » d'abord",
    )?;
    let rec_dir = crate::capture::recordings_dir(&root);
    let wav = PathBuf::from(&path);
    if !wav.starts_with(&rec_dir) {
        return Err("ce fichier n'est pas un enregistrement d'Écho".into());
    }

    let engine = crate::transcription::WhisperCli::new(
        settings.whisper_bin.unwrap_or_default(),
        settings.whisper_model.unwrap_or_default(),
    );
    let texte = crate::transcription::clean_text(&engine.transcribe(&wav)?);
    if texte.is_empty() {
        return Err("transcription vide — le moteur n'a rien entendu".into());
    }

    let id = format!(
        "trans-{}",
        wav.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    append_text(&app, &texte, &id)
}

/// Résultat d'un rejeu : combien d'entrées, et ce qui n'a pas pu être lu.
#[derive(Debug, Default, Serialize)]
pub struct ReplayReport {
    pub rejouees: usize,
    pub lignes_invalides: usize,
    pub fin_reparee: bool,
    pub compaction_refusee: bool,
    pub erreur: Option<String>,
}

impl ReplayReport {
    pub fn resume(&self) -> String {
        if let Some(e) = &self.erreur {
            return format!("journal illisible — récupération à faire : {e}");
        }
        if self.lignes_invalides > 0 {
            return format!(
                "{} ligne(s) de journal illisible(s) — compaction bloquée, récupération à faire",
                self.lignes_invalides
            );
        }
        if self.compaction_refusee {
            return "compaction du journal refusée — récupération à faire".into();
        }
        if self.fin_reparee {
            return "journal réparé (écriture interrompue) — sauvegarde conservée".into();
        }
        String::new()
    }
}

/// Rejoue au démarrage les entrées `pending`, puis compacte le journal.
pub fn replay_pending(app: &AppHandle) -> Result<ReplayReport, String> {
    let Some(root) = load_settings(app).notes_dir else {
        return Ok(ReplayReport::default());
    };
    let rapport = replay_into(&root, &journal(app)?)?;
    let resume = rapport.resume();
    if let Ok(mut slot) = RECOVERY_STATUS.lock() {
        *slot = resume;
    }
    Ok(rapport)
}

/// Cœur du rejeu, sans dépendance à Tauri (donc testable).
///
/// Chaque entrée est rejouée **dans le dossier qu'elle a journalisé**, pas dans
/// le dossier configuré au moment du redémarrage. Idempotent :
/// `notes::append_entry` ne réécrit pas une entrée dont le marqueur est présent.
pub fn replay_into(root: &Path, journal: &Journal) -> Result<ReplayReport, String> {
    let mut rapport = ReplayReport::default();

    if journal.reparer_fin_tronquee().unwrap_or(false) {
        rapport.fin_reparee = true;
    }

    let lecture = journal.read_report();
    if let Some(e) = lecture.erreur.clone() {
        rapport.erreur = Some(e);
        return Ok(rapport);
    }
    rapport.lignes_invalides = lecture.lignes_invalides;

    for entry in Journal::pending_of(&lecture) {
        let destination = if entry.dir.trim().is_empty() {
            root.to_path_buf()
        } else {
            PathBuf::from(&entry.dir)
        };
        notes::append_entry(
            &destination,
            &entry.note,
            &entry.time,
            &entry.text,
            &entry.id,
        )
        .map_err(|e| format!("rejeu impossible pour {} : {e}", entry.id))?;
        let _ = journal.mark_done(&entry);
        rapport.rejouees += 1;
    }

    // Compaction : 30 jours de `done`, tous les `pending` conservés. Refusée si
    // le journal est incomplet — l'erreur est remontée, pas avalée.
    let cutoff = (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    if let Err(e) = journal.compact(&cutoff) {
        rapport.compaction_refusee = true;
        eprintln!("compaction refusée : {e}");
    }

    Ok(rapport)
}

/// Niveau sonore poussé vers l'UI (~30 Hz pendant l'enregistrement).
#[derive(Serialize, Clone)]
pub struct LevelPayload {
    pub rms: f32,
}

/// Démarre la capture micro. L'audio est rangé dans
/// `<dossier de notes>/.echo/audio/`, jamais dans un temporaire système.
#[tauri::command]
pub fn start_capture(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let root = load_settings(&app).notes_dir.ok_or(
        "aucun dossier de notes configuré — choisis-le d'abord, l'audio est rangé juste à côté",
    )?;
    let dir = crate::capture::recordings_dir(&root);
    let handle = app.clone();
    crate::capture::start(dir, move |rms| {
        let _ = handle.emit("capture:level", LevelPayload { rms });
    })
}

/// Un enregistrement, tel que listé dans l'interface.
#[derive(Serialize)]
pub struct RecordingInfo {
    pub name: String,
    pub path: String,
    pub duration_secs: f64,
    pub modified: String,
}

/// Liste les 20 enregistrements les plus récents.
#[tauri::command]
pub fn list_recordings(app: AppHandle) -> Result<Vec<RecordingInfo>, String> {
    let Some(root) = load_settings(&app).notes_dir else {
        return Ok(Vec::new());
    };
    let dir = crate::capture::recordings_dir(&root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut out: Vec<RecordingInfo> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
        })
        .map(|e| {
            let path = e.path();
            let meta = e.metadata().ok();
            let taille = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    chrono::DateTime::<chrono::Local>::from(t)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            RecordingInfo {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
                duration_secs: taille.saturating_sub(44) as f64 / 32_000.0,
                modified,
            }
        })
        .collect();

    out.sort_by(|a, b| b.name.cmp(&a.name));
    out.truncate(20);
    Ok(out)
}

/// Ouvre le dossier des enregistrements dans l'Explorateur.
#[tauri::command]
pub fn open_recordings_folder(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    let dir = crate::capture::recordings_dir(&root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let chemin = dir.to_string_lossy().into_owned();
    app.opener()
        .open_path(chemin.clone(), None::<&str>)
        .map_err(|e| format!("ouverture du dossier impossible : {e}"))?;
    Ok(chemin)
}

/// Ouvre le dossier des notes dans l'Explorateur.
#[tauri::command]
pub fn get_drag_icon(app: AppHandle) -> Result<String, String> {
    use tauri::Manager;
    let mut path = app.path().resource_dir().map_err(|e| e.to_string())?.join("icons/32x32.png");
    if !path.is_file() && cfg!(debug_assertions) { path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png"); }
    if !path.is_file() { return Err("icône de glissement indisponible".into()); }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn open_notes_folder(app: AppHandle, expected_notes_dir: Option<String>) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    if expected_notes_dir.as_ref().is_some_and(|expected| root != PathBuf::from(expected)) {
        return Err("le dossier de notes a changé ; le chemin du fichier sauvegardé reste affiché".into());
    }
    let chemin = root.to_string_lossy().into_owned();
    app.opener()
        .open_path(chemin.clone(), None::<&str>)
        .map_err(|e| format!("ouverture du dossier impossible : {e}"))?;
    Ok(chemin)
}

/// Ouvre un enregistrement (lecteur par défaut), uniquement s'il vient du
/// dossier d'enregistrements — un chemin arbitraire est refusé.
#[tauri::command]
pub fn open_recording(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let root = load_settings(&app)
        .notes_dir
        .ok_or("aucun dossier de notes configuré")?;
    let dir = crate::capture::recordings_dir(&root);
    let cible = PathBuf::from(&path);
    if !cible.starts_with(&dir) {
        return Err("ce fichier n'est pas dans le dossier d'enregistrements".into());
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("ouverture impossible : {e}"))
}

/// Purge les `.part` laissés par un arrêt brutal. Ne touche jamais aux WAV
/// finalisés : ce sont des données utilisateur.
pub fn purge_recording_parts(app: &AppHandle) -> usize {
    let Some(root) = load_settings(app).notes_dir else {
        return 0;
    };
    crate::capture::purge_parts(&crate::capture::recordings_dir(&root))
}

/// Arrête la capture, finalise le WAV et retourne son emplacement et sa durée.
#[tauri::command]
pub fn stop_capture() -> Result<crate::capture::CaptureInfo, String> {
    crate::capture::stop()
}

#[tauri::command]
pub fn pause_capture() -> Result<(), String> {
    crate::capture::pause()
}

#[tauri::command]
pub fn resume_capture() -> Result<(), String> {
    crate::capture::resume()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier_test(nom: &str, octets: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echo-settings-{nom}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        fs::write(&f, octets).unwrap();
        f
    }

    #[test]
    fn lit_un_settings_sans_bom() {
        let f = fichier_test("sans-bom", br#"{"notes_dir":"D:\\Notes"}"#);
        let s = read_settings_file(&f);
        assert_eq!(s.notes_dir, Some(PathBuf::from("D:\\Notes")));
    }

    #[test]
    fn lit_un_settings_avec_bom() {
        let mut octets = vec![0xEF, 0xBB, 0xBF];
        octets.extend_from_slice(br#"{"notes_dir":"D:\\Notes"}"#);
        let f = fichier_test("avec-bom", &octets);
        let s = read_settings_file(&f);
        assert_eq!(
            s.notes_dir,
            Some(PathBuf::from("D:\\Notes")),
            "un BOM ne doit plus faire perdre le dossier configuré"
        );
    }

    #[test]
    fn settings_absent_donne_des_valeurs_vides() {
        let s = read_settings_file(Path::new("D:\\chemin\\qui\\n'existe\\pas.json"));
        assert_eq!(s.notes_dir, None);
    }

    #[test]
    fn settings_corrompu_ne_panique_pas() {
        let f = fichier_test("corrompu", b"{pas du json");
        assert_eq!(read_settings_file(&f).notes_dir, None);
    }

    #[test]
    fn rejeu_recree_une_note_supprimee_de_l_exterieur() {
        let dir = std::env::temp_dir().join(format!("echo-rejeu-{}", uuid::Uuid::new_v4()));
        let notes_dir = dir.join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let journal = Journal::new(dir.join("recovery.log"));

        let e = Entry::new(
            "id-unique",
            &notes_dir.to_string_lossy(),
            "2026-10-19",
            "19:07",
            "entrée en attente",
        );
        journal.append(&e).unwrap();

        // La note n'existe pas : c'est le cas « supprimée pendant la coupure ».
        assert_eq!(replay_into(&notes_dir, &journal).unwrap().rejouees, 1);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert!(note.contains("entrée en attente"));
        assert!(note.contains("<!--echo:id-unique-->"));
        assert!(journal.pending().is_empty(), "l'entrée doit être marquée done");

        // Deuxième rejeu : rien à faire, et surtout aucune duplication.
        assert_eq!(replay_into(&notes_dir, &journal).unwrap().rejouees, 0);
        let note = std::fs::read_to_string(notes_dir.join("2026-10-19.md")).unwrap();
        assert_eq!(note.matches("entrée en attente").count(), 1);
    }

    #[test]
    fn le_rejeu_respecte_le_dossier_journalise_pas_le_dossier_courant() {
        let dir = std::env::temp_dir().join(format!("echo-dest-{}", uuid::Uuid::new_v4()));
        let dossier_a = dir.join("A");
        let dossier_b = dir.join("B");
        std::fs::create_dir_all(&dossier_a).unwrap();
        std::fs::create_dir_all(&dossier_b).unwrap();
        let journal = Journal::new(dir.join("recovery.log"));

        // Entrée destinée à A, alors que l'application est reconfigurée sur B.
        let e = Entry::new(
            "id-a",
            &dossier_a.to_string_lossy(),
            "2026-10-19",
            "19:07",
            "destinée à A",
        );
        journal.append(&e).unwrap();

        let rapport = replay_into(&dossier_b, &journal).unwrap();
        assert_eq!(rapport.rejouees, 1);
        assert!(
            dossier_a.join("2026-10-19.md").exists(),
            "l'entrée doit atterrir dans le dossier journalisé"
        );
        assert!(
            !dossier_b.join("2026-10-19.md").exists(),
            "elle ne doit pas atterrir dans le dossier courant"
        );
    }
}
