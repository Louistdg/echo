//! Portable settings and one durable Markdown workspace. All file mutations are serialized.
use std::{collections::BTreeMap, fs, path::{Component, Path, PathBuf}, sync::{LazyLock, Mutex, OnceLock}};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static SETTINGS: LazyLock<Mutex<Option<Settings>>> = LazyLock::new(|| Mutex::new(None));
// Native preference changes can dispatch work to the UI thread. Serialize them
// separately from SETTINGS so callbacks may still read the active workspace.
static PREFERENCE_WRITES: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static FILES: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
const MAX_NOTE: u64 = 8 * 1024 * 1024;
const THEME_IDS: &[&str] = &["clair","nuit","ocean","foret","violet","rose","ambre","aurore","voie-lactee","andromede","nebuleuse-cuivre","nuit-boreale","horizon-io","ciel-orage","orbite-rose","poussiere-solaire","eclipse-marine","ocre-sienne","bleu-prusse","terre-volcanique","malachite","garance","safran-noir","indigo-japonais","vert-vessie","cobalt-vif","basalte","dune-sculptee","schiste-bleu","canyon-rouge","ardoise-cuivree","craie-marine","lave-refroidie","sillon-forestier","roche-lunaire","perle-ormeau","opale-nuit","soie-champagne","brume-jade","quartz-fume","perle-peche","nacre-encre","coquillage-violet","lune-argentee"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(flatten)]
    pub extra: BTreeMap<String,serde_json::Value>,
    pub version: u32,
    pub workspace: String,
    pub recent_workspaces: Vec<String>,
    pub active_note: Option<String>,
    pub theme: String,
    pub large_controls: bool,
    pub reduce_motion: bool,
    pub theme_favorites: Vec<String>,
    pub theme_intensity: f64,
    pub personal_words: Vec<String>,
    pub quick_width: f64,
    pub quick_height: f64,
    pub quick_x: Option<i32>,
    pub quick_y: Option<i32>,
    pub python: String,
    pub model: String,
    pub language: String,
    pub voice_mode: String,
    pub destination: String,
    pub keep_audio: bool,
    pub vad_silence_ms: u64,
    pub input_device: Option<String>,
    pub shortcut_dictation: String,
    pub shortcut_capture: String,
    pub always_on_top: bool,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { extra: BTreeMap::new(), version: 30, workspace: "donnees".into(), recent_workspaces: vec![], active_note: None,
            theme: "nuit-boreale".into(), large_controls: false, reduce_motion: false, theme_favorites: vec![], theme_intensity: 0.45, personal_words: vec![],
            quick_width: 680.0, quick_height: 360.0, quick_x: None, quick_y: None, python: default_python(), model: "small".into(), language: "fr".into(),
            voice_mode: "toggle".into(), destination: "note".into(), keep_audio: false, vad_silence_ms: 600,
            input_device: None, shortcut_dictation: "Ctrl+Alt+Space".into(),
            shortcut_capture: "Ctrl+Alt+S".into(), always_on_top: true, autostart: false }
    }
}

fn default_python() -> String {
    let local = portable_root().join("moteur/python/python.exe");
    if local.is_file() { return local.to_string_lossy().into(); }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let installed = PathBuf::from(profile).join(".dsh/dsh-runtimes/dsh-primary-runtime/dependencies/python/python.exe");
        if installed.is_file() { return installed.to_string_lossy().into(); }
    }
    "python".into()
}

pub fn portable_root() -> PathBuf {
    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        for p in exe.ancestors().skip(1) {
            if p.join("echo-v3.portable").is_file() { return p.to_path_buf(); }
        }
        exe.parent().unwrap_or(Path::new(".")).to_path_buf()
    }).clone()
}

fn config_path() -> PathBuf { portable_root().join("configuration/settings.json") }
fn persist_settings(value: &Settings) -> Result<(), String> {
    let p = config_path();
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut disk = value.clone();
    if let Ok(rel) = Path::new(&disk.workspace).strip_prefix(portable_root()) {
        disk.workspace = rel.to_string_lossy().into();
    }
    crate::atomic::write_atomic(&p, &serde_json::to_vec_pretty(&disk).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Réglages non enregistrés : {e}"))
}

fn prepare_workspace(p: &Path) -> Result<String, String> {
    if !p.is_dir() { return Err("Choisis un dossier existant pour tes notes.".into()); }
    let p = p.canonicalize().map_err(|e| format!("Dossier inaccessible : {e}"))?;
    for dir in ["captures", ".echo/audio", ".echo/trash", ".echo/conflits", ".echo/captures"] {
        fs::create_dir_all(p.join(dir)).map_err(|e| format!("Ce dossier n’est pas accessible en écriture : {e}"))?;
    }
    let check = p.join(".echo").join(format!("write-test-{}.tmp", uuid::Uuid::new_v4()));
    crate::atomic::write_atomic(&check, b"echo").map_err(|e| format!("Impossible d’écrire dans ce dossier : {e}"))?;
    fs::remove_file(check).map_err(|e| e.to_string())?;
    Ok(display_path(&p))
}

pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let _ = APP.set(app.clone());
    let p = config_path();
    let fresh = !p.exists();
    let mut cfg: Settings = if fresh { Settings::default() } else {
        let raw = fs::read_to_string(&p).map_err(|e| format!("Réglages illisibles : {e}"))?;
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).map_err(|e| format!("Réglages invalides, fichier conservé : {e}"))?
    };
    cfg.model = "small".into(); // Migrate legacy quality choices to the single vocal engine.
    let chosen = if Path::new(&cfg.workspace).is_absolute() { PathBuf::from(&cfg.workspace) } else { portable_root().join(&cfg.workspace) };
    if fresh { fs::create_dir_all(&chosen).map_err(|e| e.to_string())?; }
    cfg.workspace = prepare_workspace(&chosen)?;
    if !cfg.recent_workspaces.contains(&cfg.workspace) { cfg.recent_workspaces.push(cfg.workspace.clone()); }
    if fresh {
        let name = "Bienvenue dans Écho.md";
        let welcome = "# Bienvenue dans Écho\n\nUn endroit pour tes idées, ta voix et tes captures.\n\n## Commence ici\n\nÉcris dans cette note : elle se sauvegarde automatiquement. Le dossier de destination reste visible en bas à gauche.\n\n- **Nouvelle note** crée un fichier Markdown indépendant.\n- **Dicter** écrit là où est ton curseur : clique dans n’importe quel champ de texte (une conversation, un document, cette note), puis appuie sur le raccourci pour démarrer et de nouveau pour arrêter. Aucune destination à choisir ; si le champ refuse l’écriture, le texte reste récupérable dans Écho.\n- **Capturer** permet de sélectionner une zone, de l’annoter et de la copier. « Copier et ranger » conserve aussi un PNG dans le dossier captures.\n\n## Tes fichiers restent à toi\n\nLes notes sont des fichiers .md. Les captures vont dans captures et les enregistrements dans .echo/audio. Tu peux choisir un autre dossier à tout moment.\n\nLe micro reste fermé tant que tu n’actives pas la dictée ou les mains libres.\n";
        let dest = chosen.join(name);
        if !dest.exists() { crate::atomic::write_atomic(&dest, welcome.as_bytes()).map_err(|e| e.to_string())?; }
        cfg.active_note = Some(name.into());
    }
    persist_settings(&cfg)?;
    *SETTINGS.lock().map_err(|_| "Réglages indisponibles")? = Some(cfg);
    Ok(())
}

pub fn settings() -> Result<Settings, String> {
    SETTINGS.lock().map_err(|_| "Réglages indisponibles")?.clone().ok_or_else(|| "Les réglages ne sont pas encore chargés.".into())
}

fn emit_settings(cfg: &Settings) { if let Some(app) = APP.get() { let _ = app.emit("settings:changed", cfg); } }
pub fn emit_notes_changed() { if let Some(app) = APP.get() { let _ = app.emit("notes:changed", ()); } }

#[tauri::command]
pub fn settings_get() -> Result<Settings, String> { settings() }

fn merge_settings_patch(current: &Settings, patch: &serde_json::Value) -> Result<Settings, String> {
    let mut json = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let allowed = ["large_controls", "reduce_motion", "theme_favorites", "theme_intensity", "personal_words", "quick_width", "quick_height", "quick_x", "quick_y", "theme", "python", "model", "language", "voice_mode", "destination", "keep_audio", "vad_silence_ms", "input_device", "shortcut_dictation", "shortcut_capture", "always_on_top", "autostart"];
    for (key, value) in patch.as_object().ok_or("Réglages invalides")? {
        if !allowed.contains(&key.as_str()) { return Err(format!("Réglage non reconnu : {key}")); }
        json[key] = value.clone();
    }
    let mut cfg: Settings = serde_json::from_value(json).map_err(|e| e.to_string())?;
    cfg.model = "small".into(); // Older clients cannot restore a degraded quality profile.
    if !["hold", "toggle", "handsfree"].contains(&cfg.voice_mode.as_str()) || !["note", "application"].contains(&cfg.destination.as_str()) { return Err("Mode de dictée invalide".into()); }
    if !THEME_IDS.contains(&cfg.theme.as_str()) { return Err("Thème inconnu".into()); }
    if cfg.theme_favorites.len() > THEME_IDS.len() || cfg.theme_favorites.iter().any(|id| !THEME_IDS.contains(&id.as_str())) { return Err("Favori de thème inconnu".into()); }
    if !cfg.theme_intensity.is_finite() || !(0.0..=1.0).contains(&cfg.theme_intensity) { return Err("L’intensité doit être comprise entre 0 et 100 %.".into()); }
    if cfg.personal_words.len() > 64 || cfg.personal_words.iter().any(|word| word.trim().is_empty() || word.chars().count() > 64 || word.contains(['\n', '\r'])) { return Err("Le lexique accepte 64 expressions de 64 caractères maximum.".into()); }
    if !cfg.quick_width.is_finite() || !cfg.quick_height.is_finite() || !(360.0..=2400.0).contains(&cfg.quick_width) || !(280.0..=1600.0).contains(&cfg.quick_height) { return Err("Taille de barre invalide".into()); }
    if !(350..=6000).contains(&cfg.vad_silence_ms) { return Err("Le délai de silence doit être compris entre 350 et 6000 ms.".into()); }
    if cfg.python.trim().is_empty() || cfg.model.trim().is_empty() || cfg.language.len() > 12 { return Err("Le moteur et la langue doivent être renseignés.".into()); }
    Ok(cfg)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativePreference { Shortcuts, Autostart, AlwaysOnTop }

impl NativePreference {
    fn label(self) -> &'static str {
        match self { Self::Shortcuts => "raccourcis", Self::Autostart => "démarrage avec Windows", Self::AlwaysOnTop => "barre au premier plan" }
    }
}

fn rollback_preferences<F>(error: String, applied: &[NativePreference], old: &Settings, apply: &mut F) -> String
where F: FnMut(NativePreference, &Settings) -> Result<(), String> {
    let mut failures = vec![];
    for preference in applied.iter().rev() {
        if let Err(problem) = apply(*preference, old) { failures.push(format!("{} : {problem}", preference.label())); }
    }
    if failures.is_empty() { error } else { format!("{error}\nRestauration incomplète des réglages natifs : {}", failures.join(" ; ")) }
}

// Commit memory/disk only after all native changes succeed. A failed native
// setter may already have changed its target; restore it as well as earlier
// changes. Shortcut registration already rolls back its own partial failure.
fn apply_preferences<F, C>(old: &Settings, next: &Settings, mut apply: F, commit: C) -> Result<Settings, String>
where F: FnMut(NativePreference, &Settings) -> Result<(), String>, C: FnOnce() -> Result<Settings, String> {
    let changes = [
        (NativePreference::Shortcuts, old.shortcut_dictation != next.shortcut_dictation || old.shortcut_capture != next.shortcut_capture),
        (NativePreference::Autostart, old.autostart != next.autostart),
        (NativePreference::AlwaysOnTop, old.always_on_top != next.always_on_top),
    ];
    let mut applied = vec![];
    for (preference, changed) in changes {
        if !changed { continue; }
        if let Err(error) = apply(preference, next) {
            if preference != NativePreference::Shortcuts { applied.push(preference); }
            return Err(rollback_preferences(error, &applied, old, &mut apply));
        }
        applied.push(preference);
    }
    commit().map_err(|error| rollback_preferences(error, &applied, old, &mut apply))
}

fn apply_native_preference(app: &AppHandle, preference: NativePreference, cfg: &Settings) -> Result<(), String> {
    use tauri::Manager;
    use tauri_plugin_autostart::ManagerExt;
    match preference {
        NativePreference::Shortcuts => crate::shortcuts::configure(app, &cfg.shortcut_dictation, &cfg.shortcut_capture),
        NativePreference::Autostart => (if cfg.autostart { app.autolaunch().enable() } else { app.autolaunch().disable() }).map_err(|e| e.to_string()),
        NativePreference::AlwaysOnTop => { for label in ["quick","dock-panels"] { if let Some(w)=app.get_webview_window(label) { w.set_always_on_top(cfg.always_on_top).map_err(|e|e.to_string())?; } } Ok(()) },
    }
}

#[tauri::command]
pub async fn settings_update(app: AppHandle, patch: serde_json::Value) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _transaction = PREFERENCE_WRITES.lock().map_err(|_| "Modification des réglages indisponible")?;
        let old = settings()?;
        let candidate = merge_settings_patch(&old, &patch)?;
        let saved = apply_preferences(&old, &candidate, |preference, cfg| apply_native_preference(&app, preference, cfg), || {
            let mut guard = SETTINGS.lock().map_err(|_| "Réglages indisponibles")?;
            // Workspace and active-note commands may have committed while the
            // native calls ran. Apply only this patch to their latest state.
            let next = merge_settings_patch(guard.as_ref().ok_or("Réglages indisponibles")?, &patch)?;
            persist_settings(&next)?;
            *guard = Some(next.clone());
            Ok(next)
        })?;
        emit_settings(&saved);
        Ok(saved)
    }).await.map_err(|e| e.to_string())?
}

// Keep retry and update under the same transaction lock. Waiting for that lock
// must happen off the UI thread, where shortcut registration is dispatched.
pub async fn retry_shortcuts(app: AppHandle) -> Result<crate::shortcuts::Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _transaction = PREFERENCE_WRITES.lock().map_err(|_| "Modification des réglages indisponible")?;
        let cfg = settings()?;
        crate::shortcuts::configure(&app, &cfg.shortcut_dictation, &cfg.shortcut_capture)?;
        Ok(crate::shortcuts::shortcuts_status())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_choose(app: AppHandle) -> Result<Option<Settings>, String> {
    let before = settings()?;
    let a = app.clone();
    let initial = before.workspace.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || a.dialog().file().set_directory(initial).set_title("Dossier des notes, captures et enregistrements").blocking_pick_folder()).await.map_err(|e| e.to_string())?;
    let Some(file) = picked else { return Ok(None); };
    let p = file.into_path().map_err(|e| e.to_string())?;
    let chosen = tauri::async_runtime::spawn_blocking(move || prepare_workspace(&p)).await.map_err(|e| e.to_string())??;
    let mut guard = SETTINGS.lock().map_err(|_| "Réglages indisponibles")?;
    let mut cfg = guard.clone().ok_or("Réglages indisponibles")?;
    if cfg.workspace != chosen { cfg.active_note = None; }
    cfg.workspace = chosen;
    if !cfg.recent_workspaces.contains(&cfg.workspace) { cfg.recent_workspaces.push(cfg.workspace.clone()); }
    persist_settings(&cfg)?; *guard = Some(cfg.clone()); drop(guard);
    emit_settings(&cfg); emit_notes_changed();
    Ok(Some(cfg))
}

#[tauri::command]
pub fn workspace_open(app: AppHandle) -> Result<(), String> { app.opener().open_path(settings()?.workspace, None::<&str>).map_err(|e| e.to_string()) }

fn trusted_workspace(requested: Option<&str>) -> Result<PathBuf, String> {
    let cfg = settings()?;
    let chosen = requested.unwrap_or(&cfg.workspace);
    if chosen != cfg.workspace && !cfg.recent_workspaces.iter().any(|p| p == chosen) { return Err("Ce dossier n’a pas été choisi dans Écho.".into()); }
    let root = PathBuf::from(chosen).canonicalize().map_err(|e| format!("Dossier indisponible : {e}"))?;
    if !root.is_dir() { return Err("Dossier indisponible".into()); }
    Ok(root)
}

pub fn note_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = relative.replace('\\', "/");
    if normalized.contains(':') || normalized.contains('\0') || normalized.split('/').any(|s| s.is_empty() || s == ".echo" || s.starts_with('.')) { return Err("Nom de note invalide".into()); }
    let rel = Path::new(&normalized);
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) || !rel.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) { return Err("La note doit être un fichier Markdown du dossier choisi.".into()); }
    let full = root.join(rel);
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let check = if full.exists() { full.canonicalize() } else { full.parent().unwrap().canonicalize() }.map_err(|e| e.to_string())?;
    if !check.starts_with(&canonical_root) { return Err("Cette note se trouve hors du dossier choisi.".into()); }
    Ok(full)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteTarget { pub workspace: String, pub path: String }
#[derive(Clone, Debug, Serialize)]
pub struct NoteSummary { pub path: String, pub title: String, pub preview: String, pub modified: String, pub bytes: u64 }
#[derive(Clone, Debug, Serialize)]
pub struct NoteDocument { pub path: String, pub title: String, pub text: String, pub modified: String, pub workspace: String }

fn read_text(path: &Path) -> Result<String, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_NOTE { return Err("Cette note dépasse 8 Mo. Ouvre-la dans ton éditeur habituel.".into()); }
    fs::read_to_string(path).map(|t| t.trim_start_matches('\u{feff}').to_owned()).map_err(|e| format!("Note illisible, fichier conservé : {e}"))
}
fn title(text: &str, path: &str) -> String {
    text.lines().find_map(|l| l.strip_prefix("# ")).map(str::trim).filter(|t| !t.is_empty()).unwrap_or_else(|| Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Note")).chars().take(160).collect()
}
fn modified(path: &Path) -> String { fs::metadata(path).and_then(|m| m.modified()).map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()).unwrap_or_default() }
fn document(root: &Path, path: &str) -> Result<NoteDocument, String> {
    let full = note_path(root, path)?; let text = read_text(&full)?;
    Ok(NoteDocument { title: title(&text, path), path: path.into(), text, modified: modified(&full), workspace: display_path(root) })
}

#[tauri::command]
pub async fn notes_list() -> Result<Vec<NoteSummary>, String> {
    let root = trusted_workspace(None)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut items = vec![];
        fn scan(root: &Path, dir: &Path, depth: usize, out: &mut Vec<NoteSummary>) -> Result<(), String> {
            if depth > 6 || out.len() >= 5000 { return Ok(()); }
            for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?; let path = entry.path(); let name = entry.file_name(); let name = name.to_string_lossy();
                if name.starts_with('.') || name == "captures" || entry.file_type().map_err(|e| e.to_string())?.is_symlink() { continue; }
                if path.is_dir() { scan(root, &path, depth+1, out)?; continue; }
                if !path.extension().is_some_and(|s| s.eq_ignore_ascii_case("md")) { continue; }
                let relative = path.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
                let text = read_text(&path)?;
                let preview: String = text.lines().filter(|l| !l.starts_with('#') && !l.starts_with("<!--") && !l.trim().is_empty()).collect::<Vec<_>>().join(" ").chars().take(170).collect();
                out.push(NoteSummary { title: title(&text, &relative), path: relative, preview, modified: modified(&path), bytes: fs::metadata(&path).map_err(|e| e.to_string())?.len() });
                if out.len() >= 5000 { break; }
            }
            Ok(())
        }
        scan(&root, &root, 0, &mut items)?; items.sort_by(|a,b| b.modified.cmp(&a.modified)); Ok(items)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn note_read(path: String, workspace: Option<String>) -> Result<NoteDocument, String> {
    let root = trusted_workspace(workspace.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || document(&root, &path)).await.map_err(|e| e.to_string())?
}

fn safe_stem(value: &str) -> String {
    let s: String = value.chars().filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c)).take(80).collect();
    let trimmed = s.trim().trim_matches('.').trim();
    let stem = if trimmed.is_empty() { "Nouvelle note" } else { trimmed };
    if ["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.to_uppercase().as_str()) { format!("Note {stem}") } else { stem.into() }
}

#[tauri::command]
pub async fn note_create(title: Option<String>) -> Result<NoteDocument, String> {
    let root = trusted_workspace(None)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _lock = FILES.lock().map_err(|_| "Écriture indisponible")?;
        let value = title.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "Nouvelle note".into()); let stem = safe_stem(&value);
        let mut name = format!("{stem}.md"); let mut n = 2;
        while root.join(&name).exists() { name = format!("{stem} {n}.md"); n += 1; }
        crate::atomic::write_atomic(&note_path(&root, &name)?, format!("# {}\n\n", value.replace(['\r','\n'], " ").trim()).as_bytes()).map_err(|e| e.to_string())?;
        document(&root, &name)
    }).await.map_err(|e| e.to_string())??;
    emit_notes_changed(); Ok(result)
}

pub fn save_note_at(root: &Path, path: &str, text: &str, expected: Option<&str>) -> Result<NoteDocument, String> {
    if text.len() as u64 > MAX_NOTE { return Err("La note dépasse 8 Mo. Ton brouillon reste conservé.".into()); }
    let _lock = FILES.lock().map_err(|_| "Écriture indisponible")?;
    let full = note_path(root, path)?;
    let current = if full.exists() { Some(read_text(&full)?) } else { None };
    if current.as_deref() == Some(text) { return document(root, path); }
    if current.as_deref() != expected {
        let conflict_dir = root.join(".echo/conflits"); fs::create_dir_all(&conflict_dir).map_err(|e| e.to_string())?;
        let conflict = conflict_dir.join(format!("{}-{}.md", safe_stem(path), uuid::Uuid::new_v4()));
        crate::atomic::write_atomic(&conflict, text.as_bytes()).map_err(|e| format!("Conflit et copie impossible : {e}"))?;
        return Err(format!("Cette note a changé ailleurs. Ton texte est conservé dans le brouillon et dans {}. Recharge la note avant de fusionner.", display_path(&conflict)));
    }
    crate::atomic::write_atomic(&full, text.as_bytes()).map_err(|e| format!("Note non enregistrée : {e}"))?;
    document(root, path)
}

#[tauri::command]
pub async fn note_save(path: String, text: String, expected: Option<String>, workspace: String) -> Result<NoteDocument, String> {
    let root = trusted_workspace(Some(&workspace))?;
    let result = tauri::async_runtime::spawn_blocking(move || save_note_at(&root, &path, &text, expected.as_deref())).await.map_err(|e| e.to_string())??;
    emit_notes_changed(); Ok(result)
}

#[tauri::command]
pub fn note_activate(path: String, workspace: String) -> Result<(), String> {
    let root = trusted_workspace(Some(&workspace))?; let _ = note_path(&root, &path)?;
    let mut guard = SETTINGS.lock().map_err(|_| "Réglages indisponibles")?;
    let mut cfg = guard.clone().ok_or("Réglages indisponibles")?;
    if display_path(&root) != cfg.workspace { return Err("Le dossier actif a changé.".into()); }
    cfg.active_note = Some(path); persist_settings(&cfg)?; *guard = Some(cfg.clone()); drop(guard); emit_settings(&cfg); Ok(())
}

#[tauri::command]
pub async fn note_rename(path: String, title: String, workspace: String) -> Result<NoteDocument, String> {
    let root = trusted_workspace(Some(&workspace))?; let oldpath = path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _lock = FILES.lock().map_err(|_| "Écriture indisponible")?;
        let old = note_path(&root, &path)?; let mut text = read_text(&old)?;
        let name = format!("{}.md", safe_stem(&title));
        let parent = Path::new(&path).parent().unwrap_or(Path::new("")); let relative = parent.join(name).to_string_lossy().replace('\\', "/");
        let new = note_path(&root, &relative)?;
        if new != old && new.exists() { return Err("Une note porte déjà ce nom. Choisis un autre titre.".into()); }
        if text.starts_with("# ") { let rest = text.find('\n').map(|i| &text[i..]).unwrap_or("\n"); text = format!("# {}{}", title.replace(['\n','\r'], " ").trim(), rest); }
        // Save before rename: a failed rename leaves the complete document at its original path.
        crate::atomic::write_atomic(&old, text.as_bytes()).map_err(|e| e.to_string())?;
        if new != old { fs::rename(&old, &new).map_err(|e| format!("Renommage impossible : {e}"))?; }
        document(&root, &relative)
    }).await.map_err(|e| e.to_string())??;
    if settings()?.active_note.as_deref() == Some(&oldpath) { note_activate(result.path.clone(), result.workspace.clone())?; }
    emit_notes_changed(); Ok(result)
}

#[tauri::command]
pub async fn note_trash(path: String, workspace: String) -> Result<(), String> {
    let root = trusted_workspace(Some(&workspace))?; let original = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = FILES.lock().map_err(|_| "Écriture indisponible")?; let p = note_path(&root, &path)?;
        let trash = root.join(".echo/trash"); fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let target = trash.join(format!("{}-{}.md", safe_stem(&path), uuid::Uuid::new_v4()));
        fs::rename(p, target).map_err(|e| format!("Déplacement vers la corbeille impossible : {e}"))
    }).await.map_err(|e| e.to_string())??;
    let mut guard = SETTINGS.lock().map_err(|_| "Réglages indisponibles")?;
    if let Some(cfg) = guard.as_mut() { if cfg.active_note.as_deref() == Some(&original) && cfg.workspace == workspace { cfg.active_note = None; persist_settings(cfg)?; emit_settings(cfg); } }
    drop(guard); emit_notes_changed(); Ok(())
}

pub fn freeze_note() -> Result<NoteTarget, String> {
    let cfg = settings()?;
    let path = cfg.active_note.filter(|s| Path::new(&cfg.workspace).join(s).is_file()).unwrap_or_else(|| format!("{}.md", chrono::Local::now().format("%Y-%m-%d")));
    let _ = note_path(Path::new(&cfg.workspace), &path)?;
    Ok(NoteTarget { workspace: cfg.workspace, path })
}

pub fn append_transcript(target: &NoteTarget, text: &str, operation_id: &str) -> Result<String, String> {
    if text.trim().is_empty() { return Err("La transcription est vide.".into()); }
    if operation_id.is_empty() || operation_id.len() > 120 || !operation_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return Err("Identifiant de dictée invalide".into()); }
    let root = trusted_workspace(Some(&target.workspace))?;
    let _lock = FILES.lock().map_err(|_| "Écriture indisponible")?;
    let p = note_path(&root, &target.path)?;
    let mut body = if p.exists() { read_text(&p)? } else { format!("# {}\n", Path::new(&target.path).file_stem().unwrap_or_default().to_string_lossy()) };
    let marker = format!("<!-- echo-v3:{operation_id} -->");
    if body.contains(&marker) { return Ok(display_path(&p)); }
    body.push_str(&format!("\n\n{}\n{}\n", text.trim(), marker));
    if body.len() as u64 > MAX_NOTE { return Err("La note est trop volumineuse ; le texte reste dans Écho.".into()); }
    crate::atomic::write_atomic(&p, body.as_bytes()).map_err(|e| format!("Dictée non enregistrée : {e}"))?;
    drop(_lock); emit_notes_changed(); Ok(display_path(&p))
}

#[derive(Clone, Serialize)]
pub struct Asset { pub path: String, pub name: String, pub modified: String, pub bytes: u64 }
#[tauri::command]
pub async fn assets_list(kind: String) -> Result<Vec<Asset>, String> {
    let root = trusted_workspace(None)?;
    let (folder, ext) = match kind.as_str() { "captures" => ("captures", "png"), "audio" => (".echo/audio", "wav"), _ => return Err("Médiathèque inconnue".into()) };
    tauri::async_runtime::spawn_blocking(move || {
        let mut items = vec![];
        for entry in fs::read_dir(root.join(folder)).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?; let p = entry.path();
            if !p.extension().is_some_and(|s| s.eq_ignore_ascii_case(ext)) || !p.is_file() { continue; }
            items.push(Asset { name: entry.file_name().to_string_lossy().into(), path: display_path(&p), modified: modified(&p), bytes: fs::metadata(&p).map_err(|e| e.to_string())?.len() });
        }
        items.sort_by(|a,b| b.modified.cmp(&a.modified)); items.truncate(2000); Ok(items)
    }).await.map_err(|e| e.to_string())?
}

pub fn validated_asset(path: &str) -> Result<PathBuf, String> {
    let root = trusted_workspace(None)?;
    let p = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
    let allowed = p.starts_with(root.join("captures")) && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) || p.starts_with(root.join(".echo/audio")) && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !allowed || !p.is_file() { return Err("Ce média n’appartient pas au dossier actif.".into()); } Ok(p)
}

#[tauri::command]
pub async fn asset_preview(path: String) -> Result<String, String> {
    let p = validated_asset(&path)?;
    if !p.extension().is_some_and(|s| s.eq_ignore_ascii_case("png")) { return Err("Ce fichier n’est pas une image.".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        use base64::Engine;
        let img = image::ImageReader::open(p).map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?.thumbnail(480,320);
        let mut out = std::io::Cursor::new(Vec::new()); img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner())))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn asset_open(app: AppHandle, path: String) -> Result<(), String> { app.opener().open_path(display_path(&validated_asset(&path)?), None::<&str>).map_err(|e| e.to_string()) }
#[tauri::command]
pub fn clipboard_copy(text: String) -> Result<(), String> { arboard::Clipboard::new().map_err(|e| e.to_string())?.set_text(text).map_err(|e| format!("Copie impossible : {e}")) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn apply_fake_native(state: &RefCell<Settings>, preference: NativePreference, cfg: &Settings) {
        let mut state = state.borrow_mut();
        match preference {
            NativePreference::Shortcuts => { state.shortcut_dictation = cfg.shortcut_dictation.clone(); state.shortcut_capture = cfg.shortcut_capture.clone(); }
            NativePreference::Autostart => state.autostart = cfg.autostart,
            NativePreference::AlwaysOnTop => state.always_on_top = cfg.always_on_top,
        }
    }

    #[test]
    fn single_voice_profile_normalizes_legacy_choice_and_preserves_preferences() {
        let mut old = Settings::default();
        assert_eq!(old.voice_mode, "toggle");
        assert_eq!(old.vad_silence_ms, 600);
        old.model = "base".into();
        old.workspace = "existing-workspace".into();
        old.active_note = Some("existing.md".into());
        old.personal_words = vec!["Écho".into()];
        let merged = merge_settings_patch(&old, &serde_json::json!({"model":"base","theme":"nuit"})).unwrap();
        assert_eq!(merged.model, "small");
        assert_eq!(merged.workspace, old.workspace);
        assert_eq!(merged.active_note, old.active_note);
        assert_eq!(merged.personal_words, old.personal_words);
        assert_eq!(merged.theme, "nuit");
    }

    #[test]
    fn patches_preserve_other_preferences_and_latest_workspace() {
        let old = Settings::default();
        let mut latest = merge_settings_patch(&old, &serde_json::json!({"theme":"nuit"})).unwrap();
        latest.workspace = "second-workspace".into();
        latest.recent_workspaces.push(latest.workspace.clone());
        latest.active_note = Some("nouvelle.md".into());
        let merged = merge_settings_patch(&latest, &serde_json::json!({"destination":"application"})).unwrap();
        assert_eq!(merged.theme, "nuit");
        assert_eq!(merged.destination, "application");
        assert_eq!(merged.workspace, latest.workspace);
        assert_eq!(merged.active_note, latest.active_note);
        assert_eq!(merged.recent_workspaces, latest.recent_workspaces);
        assert!(merge_settings_patch(&latest, &serde_json::json!({"workspace":"stale-workspace"})).is_err());
    }

    #[test]
    fn material_settings_and_geometry_preserve_existing_preferences() {
        let old = Settings::default();
        for id in THEME_IDS {
            let next = merge_settings_patch(&old, &serde_json::json!({"theme":id,"theme_favorites":[id],"theme_intensity":0.75,"quick_width":720,"quick_height":460,"personal_words":["Ouvrane","Écho"]})).unwrap();
            assert_eq!(next.theme,id.to_string()); assert_eq!(next.workspace,old.workspace);
            assert_eq!(next.python,old.python); assert_eq!(next.active_note,old.active_note);
        }
        for patch in [serde_json::json!({"theme":"unknown"}),serde_json::json!({"theme_intensity":1.1}),serde_json::json!({"theme_favorites":["unknown"]}),serde_json::json!({"quick_width":120}),serde_json::json!({"personal_words":[""]})] {
            assert!(merge_settings_patch(&old,&patch).is_err());
        }
        let restored: Settings = serde_json::from_str(r#"{"theme":"nuit","model":"small","workspace":"original"}"#).unwrap();
        assert_eq!(restored.theme,"nuit"); assert_eq!(restored.quick_width,680.0); assert!(restored.theme_favorites.is_empty());
    }

    #[test]
    fn settings_roundtrip_and_patch_preserve_unknown_preferences(){
        let original:Settings=serde_json::from_value(serde_json::json!({"workspace":"existing","theme":"clair","future_option":{"enabled":true,"name":"à conserver"}})).unwrap();
        let next=merge_settings_patch(&original,&serde_json::json!({"theme":"voie-lactee","theme_intensity":0.25})).unwrap();
        let disk=serde_json::to_value(next).unwrap();
        assert_eq!(disk["future_option"]["name"],"à conserver");assert_eq!(disk["future_option"]["enabled"],true);
        assert_eq!(disk["workspace"],"existing");assert_eq!(disk["theme"],"voie-lactee");
    }

    #[test]
    fn failed_persistence_restores_all_native_preferences() {
        let old = Settings::default();
        let next = merge_settings_patch(&old, &serde_json::json!({"shortcut_dictation":"Ctrl+Shift+Space","autostart":true,"always_on_top":false})).unwrap();
        let native = RefCell::new(old.clone());
        let attempted_commit = Cell::new(false);
        let error = apply_preferences(&old, &next, |preference, cfg| {
            apply_fake_native(&native, preference, cfg); Ok(())
        }, || { attempted_commit.set(true); Err("disk full".into()) }).unwrap_err();
        assert_eq!(error, "disk full");
        assert!(attempted_commit.get());
        assert_eq!(serde_json::to_value(&*native.borrow()).unwrap(), serde_json::to_value(&old).unwrap());
    }

    #[test]
    fn failed_native_change_restores_partial_effect_and_earlier_changes() {
        let old = Settings::default();
        let next = merge_settings_patch(&old, &serde_json::json!({"shortcut_dictation":"Ctrl+Shift+Space","autostart":true})).unwrap();
        let native = RefCell::new(old.clone());
        let attempted_commit = Cell::new(false);
        let error = apply_preferences(&old, &next, |preference, cfg| {
            apply_fake_native(&native, preference, cfg);
            if preference == NativePreference::Autostart && cfg.autostart { Err("registry failure".into()) } else { Ok(()) }
        }, || { attempted_commit.set(true); Ok(next.clone()) }).unwrap_err();
        assert_eq!(error, "registry failure");
        assert!(!attempted_commit.get());
        assert_eq!(serde_json::to_value(&*native.borrow()).unwrap(), serde_json::to_value(&old).unwrap());
    }

    #[test]
    fn failed_rollback_is_reported_and_does_not_skip_other_restorations() {
        let old = Settings::default();
        let next = merge_settings_patch(&old, &serde_json::json!({"shortcut_dictation":"Ctrl+Shift+Space","autostart":true})).unwrap();
        let native = RefCell::new(old.clone());
        let error = apply_preferences(&old, &next, |preference, cfg| {
            if preference == NativePreference::Autostart && !cfg.autostart { return Err("access denied".into()); }
            apply_fake_native(&native, preference, cfg); Ok(())
        }, || Err("disk full".into())).unwrap_err();
        assert!(error.contains("disk full"));
        assert!(error.contains("Restauration incomplète"));
        assert!(error.contains("access denied"));
        assert_eq!(native.borrow().shortcut_dictation, old.shortcut_dictation);
        assert!(native.borrow().autostart);
    }

    fn root() -> PathBuf { let p = std::env::temp_dir().join(format!("echo-v3-notes-{}", uuid::Uuid::new_v4())); fs::create_dir_all(&p).unwrap(); p }
    #[test] fn note_cannot_escape_or_read_internal_data() { let r=root(); for p in ["../secret.md", "..\\secret.md", ".echo/state.md", "C:\\secret.md", "note.txt", "/secret.md"] { assert!(note_path(&r,p).is_err(), "{p}"); } fs::remove_dir_all(r).unwrap(); }
    #[test] fn conflicting_edit_preserves_both_versions() { let r=root(); fs::write(r.join("note.md"),"external edit").unwrap(); assert!(save_note_at(&r,"note.md","my draft",Some("old")).is_err()); assert_eq!(fs::read_to_string(r.join("note.md")).unwrap(),"external edit"); let entries:Vec<_>=fs::read_dir(r.join(".echo/conflits")).unwrap().collect(); assert_eq!(entries.len(),1); assert_eq!(fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap(),"my draft"); fs::remove_dir_all(r).unwrap(); }
    #[test] fn first_save_cannot_overwrite_existing_file() { let r=root(); fs::write(r.join("note.md"),"original").unwrap(); assert!(save_note_at(&r,"note.md","new",None).is_err()); assert_eq!(fs::read_to_string(r.join("note.md")).unwrap(),"original"); fs::remove_dir_all(r).unwrap(); }
    #[test] fn saved_revision_can_be_updated_and_retried() { let r=root(); save_note_at(&r,"note.md","a",None).unwrap(); save_note_at(&r,"note.md","b",Some("a")).unwrap(); save_note_at(&r,"note.md","b",Some("a")).unwrap(); assert_eq!(fs::read_to_string(r.join("note.md")).unwrap(),"b"); fs::remove_dir_all(r).unwrap(); }
    #[test]
    fn accessibility_roundtrip_preserves_lexicon_and_accepts_slow_speech_pauses() {
        let mut old=Settings::default();
        old.personal_words=vec!["Ouvrane".into()];
        old.extra.insert("unknown_future_pref".into(),serde_json::json!(42));
        let next=merge_settings_patch(&old,&serde_json::json!({"large_controls":true,"reduce_motion":true,"voice_mode":"handsfree","vad_silence_ms":6000})).unwrap();
        let restored:Settings=serde_json::from_value(serde_json::to_value(&next).unwrap()).unwrap();
        assert!(restored.large_controls && restored.reduce_motion);
        assert_eq!(restored.vad_silence_ms,6000);
        assert_eq!(restored.personal_words,old.personal_words);
        assert_eq!(restored.extra["unknown_future_pref"],42);
        assert!(merge_settings_patch(&next,&serde_json::json!({"vad_silence_ms":6001})).is_err());
        assert!(merge_settings_patch(&next,&serde_json::json!({"vad_silence_ms":349})).is_err());
    }

}
