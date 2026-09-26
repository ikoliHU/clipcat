#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clips;
mod engine;
mod media;
use clips::Clip;
mod games;
mod i18n;
mod links;
mod logfile;
mod platform;
mod process;
mod resources;
mod settings;
mod updater;

use engine::Engine;
use i18n::{t, tf};
use serde::Serialize;
use serde_json::json;
use settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::PageLoadEvent,
    AppHandle, Emitter, Manager, WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const TRAY_ID: &str = "main";
const ICON_ACTIVE: &[u8] = include_bytes!("../icons/tray-active.png");
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
const TOAST_DURATION: Duration = Duration::from_millis(3800);
const TOAST_MARGIN: i32 = 24;
const RECOVER_COOLDOWN: Duration = Duration::from_secs(20);
const PTT_RELEASE_DELAY: Duration = Duration::from_millis(200);
const HOTKEY_DEBOUNCE: Duration = Duration::from_millis(250);

/// A push-to-talk szál ezekből olvas, hogy ne kelljen zárat vennie.
static MIC_MODE: AtomicU32 = AtomicU32::new(MIC_OFF);
static PTT_VK: AtomicU32 = AtomicU32::new(0);
const MIC_OFF: u32 = 0;
const MIC_PTT: u32 = 1;
const MIC_ALWAYS: u32 = 2;

#[derive(Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    obs_installed: bool,
    obs_running: bool,
    replay_enabled: bool,
    replay_active: bool,
    /// A puffer utolsó ürítése (Unix ms), 0 ha nem fut
    buffer_since: u64,
    recording: bool,
    /// A kézi felvétel kezdete (Unix ms), 0 ha nem fut
    recording_since: u64,
    error: Option<String>,
    encoder: String,
    buffer_seconds: u32,
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Record,
    OpenFolder,
    Gallery,
}

struct AppState {
    settings: Mutex<Settings>,
    operations: Mutex<()>,
    installing: AtomicBool,
    save_in_progress: AtomicBool,
    hotkeys_suspended: AtomicBool,
    hotkey_generation: AtomicU64,
    selected_folders: Mutex<HashSet<PathBuf>>,
    status: Mutex<Status>,
    shortcuts: Mutex<Vec<(Shortcut, Action)>>,
    /// A natív és a polling gyorsbillentyű-esemény ugyanazt a lenyomást ne futtassa kétszer.
    last_shortcut: Mutex<[Option<Instant>; 4]>,
    /// A tálcamenü állapotfüggő elemei: (felvétel, visszajátszás)
    tray_items: Mutex<Option<(MenuItem<Wry>, MenuItem<Wry>)>>,
    tray_labels: Mutex<Vec<(MenuItem<Wry>, &'static str)>>,
    engine: Mutex<Option<Engine>>,
    engine_error: Mutex<Option<String>>,
    /// A mentés kérésekor előtérben lévő játék mappája (a mentés csak később készül el)
    pending_folder: Mutex<Option<String>>,
    last_recover: Mutex<Option<Instant>>,
    quitting: AtomicBool,
    toast_generation: AtomicU64,
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn current_settings(app: &AppHandle) -> Settings {
    state(app).settings.lock().unwrap().clone()
}

/// "Alt+KeyZ" -> "Alt+Z", "Ctrl+Digit1" -> "Ctrl+1"
fn pretty_hotkey(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| part.strip_prefix("Key").or_else(|| part.strip_prefix("Digit")).unwrap_or(part))
        .collect::<Vec<_>>()
        .join("+")
}

// ---------- Rögzítőmotor ----------

fn engine_config(s: &Settings) -> engine::Config {
    let monitor = platform::primary_monitor();
    let base = monitor.as_ref().map_or((1920, 1080), |m| (m.width, m.height));
    let output = if s.resolution == "native" {
        base
    } else {
        settings::parse_resolution(&s.resolution).unwrap_or((1920, 1080))
    };
    // Ffmpeg nélkül (pl. sérült telepítés) a memóriás puffer marad, hogy a mentés működjön
    let disk = s.buffer_storage == "disk";
    if disk && !engine::disk_buffer_available() {
        logfile::write("Lemezes puffer beállítva, de nincs ffmpeg: memóriás puffer");
    }
    engine::Config {
        output_dir: s.output_dir.clone(),
        buffer_seconds: s.buffer_seconds,
        buffer_dir: (disk && engine::disk_buffer_available()).then(|| s.buffer_dir.clone()),
        base,
        output,
        fps: s.fps,
        bitrate_kbps: s.bitrate_mbps * 1000,
        hevc: s.codec == "hevc",
        capture_desktop: s.capture_desktop,
        monitor_id: monitor.map(|m| m.device_id).unwrap_or_default(),
        mic_device: s.mic_device.clone(),
        mic_enabled: s.mic_mode != "off",
    }
}

fn apply_mic_settings(s: &Settings) {
    let mode = match s.mic_mode.as_str() {
        "always" => MIC_ALWAYS,
        "ptt" => MIC_PTT,
        _ => MIC_OFF,
    };
    PTT_VK.store(s.mic_ptt_vk, Ordering::SeqCst);
    MIC_MODE.store(mode, Ordering::SeqCst);
}

/// Push-to-talk: a mikrofon csak a gomb nyomva tartása alatt (és utána egy kis ideig) szól.
fn start_mic_thread(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_pressed = Instant::now() - PTT_RELEASE_DELAY;
        while !state(&app).quitting.load(Ordering::SeqCst) {
            let muted = match MIC_MODE.load(Ordering::SeqCst) {
                MIC_ALWAYS => false,
                MIC_PTT => {
                    if platform::key_down(PTT_VK.load(Ordering::SeqCst)) {
                        last_pressed = Instant::now();
                    }
                    last_pressed.elapsed() > PTT_RELEASE_DELAY
                }
                _ => true,
            };
            engine::set_mic_muted(muted);
            std::thread::sleep(Duration::from_millis(if MIC_MODE.load(Ordering::SeqCst) == MIC_PTT {
                15
            } else {
                200
            }));
        }
    });
}

fn start_engine(app: &AppHandle) {
    let app_state = state(app);
    let operation = app_state.operations.lock().unwrap();
    if app_state.quitting.load(Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    engine::set_event_handler(move |event| on_engine_event(&handle, event));
    let settings = current_settings(app);
    let result = Engine::start(&engine_config(&settings), settings.replay_enabled);
    let st = state(app);
    match result {
        Ok(engine) => {
            *st.engine.lock().unwrap() = Some(engine);
            *st.engine_error.lock().unwrap() = None;
        }
        Err(e) => {
            logfile::write(&format!("A rögzítőmotor nem indult: {e}"));
            *st.engine_error.lock().unwrap() = Some(e.clone());
            show_toast(app, "error", &t("toast.captureNotStarted"), &e);
        }
    }
    drop(operation);
    refresh_status(app);
}

fn on_engine_event(app: &AppHandle, event: engine::Event) {
    match event {
        engine::Event::Saved(Some(path)) => {
            let app = app.clone();
            std::thread::spawn(move || {
                clear_buffer(&app);
                finish_save(&app, PathBuf::from(path));
            });
        }
        // A motor zárolása alatt érkezik, ezért külön szálon dolgozzuk fel
        engine::Event::Recorded(path) => {
            let app = app.clone();
            std::thread::spawn(move || finish_recording(&app, PathBuf::from(path)));
        }
        engine::Event::RecordingFailed(code) => {
            logfile::write(&format!("A felvétel hiba miatt leállt, kód: {code}"));
            show_toast(
                app,
                "error",
                &t("toast.recordingStopped"),
                &tf("toast.errorCode", &[("code", &code)]),
            );
        }
        engine::Event::Saved(None) => {
            state(app).save_in_progress.store(false, Ordering::SeqCst);
            show_toast(app, "error", &t("toast.saveFailed"), &t("toast.savedClipMissing"));
        }
        engine::Event::SaveFailed(e) => {
            state(app).save_in_progress.store(false, Ordering::SeqCst);
            logfile::write(&format!("Mentés sikertelen: {e}"));
            show_toast(app, "error", &t("toast.saveFailed"), &e);
        }
        engine::Event::Stopped(code) if code != 0 => {
            logfile::write(&format!("A rögzítés leállt, kód: {code}"));
            show_toast(app, "error", &t("toast.captureStopped"), &t("status.restarting"));
        }
        engine::Event::Stopped(_) => {}
    }
}

/// Mentés után üríti a puffert, így a következő klip nem ismétli meg a most mentett részt.
fn clear_buffer(app: &AppHandle) {
    let st = state(app);
    let operation = st.operations.lock().unwrap();
    if let Some(engine) = state(app).engine.lock().unwrap().as_mut() {
        if let Err(e) = engine.clear_replay() {
            logfile::write(&format!("A puffer nem üríthető: {e}"));
        }
    }
    drop(operation);
    refresh_status(app);
}

/// ShadowPlay-szerű, még nem létező fájlnév a játék mappájában.
fn clip_path(app: &AppHandle, folder: &str, ext: &str) -> PathBuf {
    let dir = PathBuf::from(current_settings(app).output_dir).join(folder);
    let _ = std::fs::create_dir_all(&dir);
    let base = format!("{folder} {}", platform::local_time("%Y.%m.%d - %H.%M.%S"));
    let mut target = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    target
}

/// A mentett klipet a játék mappájába helyezi, ShadowPlay-szerű névvel.
fn finish_save(app: &AppHandle, src: PathBuf) {
    let folder = state(app)
        .pending_folder
        .lock()
        .unwrap()
        .take()
        .unwrap_or_else(|| games::folder_for(platform::foreground_window()));
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
    let target = clip_path(app, &folder, &ext);

    // A muxer épp most zárta le a fájlt; ha még fogja, kicsit várunk
    let mut moved = false;
    for _ in 0..40 {
        if std::fs::rename(&src, &target).is_ok() {
            moved = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let final_path = if moved { target } else { src };
    logfile::write(&format!("Klip mentve: {}", final_path.display()));
    announce_clip(
        app,
        &final_path,
        &folder,
        &t("toast.clipSaved"),
        (!moved).then(|| t("toast.moveFailed")),
    );
    state(app).save_in_progress.store(false, Ordering::SeqCst);
}

fn finish_recording(app: &AppHandle, path: PathBuf) {
    let folder = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    announce_clip(app, &path, &folder, &t("toast.recordingSaved"), None);
    refresh_status(app);
}

/// Legutóbbi klipként megjegyzi, frissíti a galériát és értesítést mutat.
fn announce_clip(app: &AppHandle, path: &Path, folder: &str, title: &str, note: Option<String>) {
    let st = state(app);
    let operation = st.operations.lock().unwrap();
    let open_hotkey = {
        let mut s = st.settings.lock().unwrap();
        s.last_clip = Some(path.to_string_lossy().into_owned());
        let _ = settings::save(&s);
        pretty_hotkey(&s.hotkey_open_folder)
    };
    drop(operation);
    let _ = app.emit("clip-saved", json!({ "path": path.to_string_lossy(), "game": folder }));
    let detail = match note {
        Some(note) => format!("{folder} · {note}"),
        None if open_hotkey.is_empty() => folder.to_string(),
        None => format!("{folder} · {}", tf("toast.openFolderHint", &[("hotkey", &open_hotkey)])),
    };
    show_toast(app, "ok", title, &detail);
}

fn refresh_status(app: &AppHandle) {
    let st = state(app);
    let Ok(_operation) = st.operations.try_lock() else { return };
    if let Some(engine) = st.engine.lock().unwrap().as_mut() {
        if let Err(error) = engine.check_resources() {
            *st.engine_error.lock().unwrap() = Some(error);
        }
    }
    let snapshot = || {
        st.engine.lock().unwrap().as_ref().map(|e| {
            (
                e.replay_enabled(),
                e.replay_active(),
                e.buffer_since(),
                e.recording_active(),
                e.recording_since(),
            )
        })
    };
    let mut snap = snapshot();

    // Ha a puffer hiba miatt leállt (és nem kézzel állították le), újraindítjuk (nem túl sűrűn)
    let recover = st.settings.lock().unwrap().keep_obs_running;
    if snap.is_some_and(|(enabled, active, ..)| enabled && !active)
        && recover
        && !st.quitting.load(Ordering::SeqCst)
        && !st.installing.load(Ordering::SeqCst)
    {
        let mut last = st.last_recover.lock().unwrap();
        if last.is_none_or(|t| t.elapsed() > RECOVER_COOLDOWN) {
            *last = Some(Instant::now());
            drop(last);
            if let Some(engine) = st.engine.lock().unwrap().as_mut() {
                if let Err(e) = engine.restart() {
                    logfile::write(&format!("Újraindítás sikertelen: {e}"));
                    *st.engine_error.lock().unwrap() = Some(e);
                } else {
                    *st.engine_error.lock().unwrap() = None;
                }
            }
            snap = snapshot();
        }
    }

    let (replay_enabled, replay_active, buffer_since, recording, recording_since) = snap.unwrap_or_default();
    let (encoder, buffer_seconds) = st
        .engine
        .lock()
        .unwrap()
        .as_ref()
        .map(|e| (e.encoder_name().to_string(), e.effective_buffer_seconds()))
        .unwrap_or_default();
    let status = Status {
        obs_installed: engine::engine_available(),
        obs_running: snap.is_some(),
        replay_enabled,
        replay_active,
        buffer_since,
        recording,
        recording_since,
        error: st.engine_error.lock().unwrap().clone(),
        encoder,
        buffer_seconds,
    };
    let mut current = st.status.lock().unwrap();
    if *current != status {
        *current = status.clone();
        drop(current);
        update_tray(app, &status);
        let _ = app.emit("status", &status);
    }
}

fn start_status_thread(app: AppHandle) {
    std::thread::spawn(move || {
        while !state(&app).quitting.load(Ordering::SeqCst) {
            refresh_status(&app);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

// ---------- Műveletek ----------

fn request_save(app: &AppHandle) -> Result<(), String> {
    let st = state(app);
    let _operation = st.operations.lock().unwrap();
    if st.installing.load(Ordering::SeqCst) || st.quitting.load(Ordering::SeqCst) {
        return Err(t("update.busy"));
    }
    if st.save_in_progress.swap(true, Ordering::SeqCst) {
        return Err(t("engine.saveBusy"));
    }
    let status = st.status.lock().unwrap().clone();
    let result = if !status.obs_installed {
        Err(t("error.engineMissing"))
    } else {
        let folder = games::folder_for(platform::foreground_window());
        *st.pending_folder.lock().unwrap() = Some(folder.clone());
        match st.engine.lock().unwrap().as_mut() {
            Some(engine) => engine.save().map(|()| folder),
            None => Err(t("error.engineNotRunning")),
        }
    };
    if result.is_err() {
        st.save_in_progress.store(false, Ordering::SeqCst);
        st.pending_folder.lock().unwrap().take();
    }
    match &result {
        Ok(folder) => show_toast(app, "pending", &t("toast.clipSaving"), folder),
        Err(e) => {
            logfile::write(&format!("Mentés sikertelen: {e}"));
            show_toast(app, "error", &t("toast.saveFailed"), e);
        }
    }
    result.map(|_| ())
}

/// Kézi felvétel indítása vagy leállítása (a leállítás a fájl lezárásáig tart, ezért nem a fő szálon fut).
fn toggle_recording(app: &AppHandle) {
    let st = state(app);
    let operation = st.operations.lock().unwrap();
    if st.installing.load(Ordering::SeqCst) || st.quitting.load(Ordering::SeqCst) {
        return;
    }
    let result = {
        let mut engine = st.engine.lock().unwrap();
        match engine.as_mut() {
            None => Err(t("error.engineNotRunning")),
            Some(e) if e.recording_active() => {
                // A leállítás a fájl lezárásáig tart, ezért előtte jelezzük, hogy a mentés elkezdődött
                show_toast(app, "pending", &t("toast.recordingSaving"), "");
                e.stop_recording().map(|()| None)
            }
            Some(e) => {
                let folder = games::folder_for(platform::foreground_window());
                let path = clip_path(app, &folder, "mp4");
                e.start_recording(&path).map(|()| Some(folder))
            }
        }
    };
    match result {
        Ok(Some(folder)) => {
            let hotkey = pretty_hotkey(&current_settings(app).hotkey_record);
            let detail = if hotkey.is_empty() {
                folder
            } else {
                format!("{folder} · {}", tf("toast.stopHint", &[("hotkey", &hotkey)]))
            };
            show_toast(app, "ok", &t("toast.recordingStarted"), &detail);
        }
        Ok(None) => {}
        Err(e) => {
            logfile::write(&format!("Felvétel sikertelen: {e}"));
            show_toast(app, "error", &t("toast.recordFailed"), &e);
        }
    }
    drop(operation);
    refresh_status(app);
}

/// A választást megjegyzi: a következő indításkor is így indul.
fn set_replay(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let st = state(app);
    let operation = st.operations.lock().unwrap();
    if st.save_in_progress.load(Ordering::SeqCst) {
        return Err(t("engine.saveBusy"));
    }
    if st.installing.load(Ordering::SeqCst) || st.quitting.load(Ordering::SeqCst) {
        return Err(t("update.busy"));
    }
    let result = match st.engine.lock().unwrap().as_mut() {
        Some(engine) => engine.set_replay_enabled(enabled),
        None => Err(t("error.engineNotRunning")),
    };
    {
        let mut s = st.settings.lock().unwrap();
        if result.is_ok() {
            s.replay_enabled = enabled;
        }
        if let Err(e) = settings::save(&s) {
            logfile::write(&format!("A visszajátszás állapota nem menthető: {e}"));
        }
    }
    logfile::write(&format!(
        "Visszajátszás {}{}",
        if enabled { "elindítva" } else { "leállítva" },
        result.as_ref().err().map_or(String::new(), |e| format!(" – hiba: {e}"))
    ));
    drop(operation);
    refresh_status(app);
    result
}

fn open_last_folder(app: &AppHandle) {
    let s = current_settings(app);
    match s.last_clip.filter(|p| Path::new(p).exists()) {
        Some(clip) => platform::reveal(&clip),
        None => platform::open_path(&s.output_dir),
    }
}

fn show_main(app: &AppHandle, view: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = app.emit("open-view", view);
}

fn run_action(app: &AppHandle, action: Action) {
    logfile::write(match action {
        Action::Save => "Művelet: mentés",
        Action::Record => "Művelet: felvétel",
        Action::OpenFolder => "Művelet: mappa megnyitása",
        Action::Gallery => "Művelet: galéria",
    });
    match action {
        Action::Save => {
            let _ = request_save(app);
        }
        Action::Record => {
            let app = app.clone();
            std::thread::spawn(move || toggle_recording(&app));
        }
        Action::OpenFolder => open_last_folder(app),
        Action::Gallery => show_main(app, "gallery"),
    }
}

fn run_shortcut_action(app: &AppHandle, action: Action) -> bool {
    let index = match action {
        Action::Save => 0,
        Action::Record => 1,
        Action::OpenFolder => 2,
        Action::Gallery => 3,
    };
    let st = state(app);
    let mut last = st.last_shortcut.lock().unwrap();
    if last[index].is_some_and(|at| at.elapsed() < HOTKEY_DEBOUNCE) {
        return false;
    }
    last[index] = Some(Instant::now());
    drop(last);
    run_action(app, action);
    true
}

/// Néhány játék (pl. League of Legends) fókuszban elnyeli a WM_HOTKEY eseményt.
/// Windowson a fizikai billentyűállapotot is figyeljük; az élváltás és a debounce
/// megakadályozza az ismétlést, ha a natív esemény is megérkezik.
#[cfg(windows)]
fn start_hotkey_fallback(app: AppHandle) {
    std::thread::spawn(move || {
        let mut down = HashSet::new();
        while !state(&app).quitting.load(Ordering::SeqCst) {
            let shortcuts = state(&app).shortcuts.lock().unwrap().clone();
            down.retain(|shortcut| shortcuts.iter().any(|(candidate, _)| candidate == shortcut));
            for (shortcut, action) in shortcuts {
                if platform::shortcut_down(&shortcut) {
                    if down.insert(shortcut) && run_shortcut_action(&app, action) {
                        logfile::write(&format!(
                            "Gyorsbillentyű polling fallback: {}",
                            pretty_hotkey(&shortcut.into_string())
                        ));
                    }
                } else {
                    down.remove(&shortcut);
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    });
}

/// Leállítja a rögzítőmotort (kilépéskor és frissítés telepítése előtt).
fn stop_engine(app: &AppHandle) {
    let st = state(app);
    let _operation = st.operations.lock().unwrap();
    st.quitting.store(true, Ordering::SeqCst);
    let engine = st.engine.lock().unwrap().take();
    if let Some(engine) = engine {
        engine.shutdown();
    }
}

fn quit(app: &AppHandle) {
    stop_engine(app);
    app.exit(0);
}

// ---------- Értesítés ----------

fn show_toast(app: &AppHandle, kind: &str, title: &str, detail: &str) {
    let st = state(app);
    let (show, sound) = {
        let s = st.settings.lock().unwrap();
        (s.show_notification || kind == "error", s.notification_sound)
    };
    // A folyamatban lévő mentés nem sípol, csak a végeredmény
    if sound && kind != "pending" {
        platform::beep(kind == "error");
    }
    if !show {
        return;
    }
    let Some(window) = app.get_webview_window("toast") else { return };
    let generation = st.toast_generation.fetch_add(1, Ordering::SeqCst) + 1;

    platform::show_overlay(&window, TOAST_MARGIN);
    let _ = app.emit(
        "toast",
        json!({ "kind": kind, "title": title, "detail": detail, "lang": i18n::lang() }),
    );

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(TOAST_DURATION);
        if state(&app).toast_generation.load(Ordering::SeqCst) == generation {
            if let Some(window) = app.get_webview_window("toast") {
                platform::hide_overlay(&window);
            }
        }
    });
}

// ---------- Gyorsbillentyűk, automatikus indítás ----------

fn parse_hotkeys(s: &Settings) -> Result<Vec<(Shortcut, Action)>, String> {
    let mut list: Vec<(Shortcut, Action)> = Vec::new();
    for (text, action, label) in [
        (&s.hotkey_save, Action::Save, "action.save"),
        (&s.hotkey_record, Action::Record, "action.record"),
        (&s.hotkey_open_folder, Action::OpenFolder, "action.openFolder"),
        (&s.hotkey_gallery, Action::Gallery, "action.gallery"),
    ] {
        if text.trim().is_empty() {
            continue;
        }
        let shortcut: Shortcut = text
            .parse()
            .map_err(|_| tf("error.hotkeyInvalid", &[("action", &t(label)), ("hotkey", &pretty_hotkey(text))]))?;
        if list.iter().any(|(other, _)| *other == shortcut) {
            return Err(tf("error.hotkeyDuplicate", &[("hotkey", &pretty_hotkey(text))]));
        }
        list.push((shortcut, action));
    }
    Ok(list)
}

/// Regisztrálja a gyorsbillentyűket; amelyiket más program foglalja, azt kihagyja és jelzi.
fn register_hotkeys(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let list = parse_hotkeys(s)?;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut failed = Vec::new();
    for (shortcut, _) in &list {
        if gs.register(*shortcut).is_err() {
            failed.push(shortcut.into_string());
        }
    }
    *state(app).shortcuts.lock().unwrap() = list;
    logfile::write(&format!(
        "Gyorsbillentyűk: mentés={} felvétel={} mappa={} galéria={}{}",
        pretty_hotkey(&s.hotkey_save),
        pretty_hotkey(&s.hotkey_record),
        pretty_hotkey(&s.hotkey_open_folder),
        pretty_hotkey(&s.hotkey_gallery),
        if failed.is_empty() {
            String::new()
        } else {
            format!(" | NEM sikerült: {}", failed.join(", "))
        }
    ));
    if failed.is_empty() {
        Ok(())
    } else {
        let hotkeys = failed.iter().map(|h| pretty_hotkey(h)).collect::<Vec<_>>().join(", ");
        Err(tf("error.hotkeyTaken", &[("hotkeys", &hotkeys)]))
    }
}

// ---------- Tálca ----------

fn update_tray(app: &AppHandle, status: &Status) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let s = current_settings(app);
    for (item, key) in state(app).tray_labels.lock().unwrap().iter() {
        let _ = item.set_text(t(key));
    }
    let (icon, tooltip) = if status.recording {
        (ICON_ACTIVE, tf("tray.recording", &[("hotkey", &pretty_hotkey(&s.hotkey_record))]))
    } else if status.replay_active {
        (ICON_ACTIVE, tf("tray.replayActive", &[("hotkey", &pretty_hotkey(&s.hotkey_save))]))
    } else if !status.obs_installed {
        (ICON_IDLE, t("tray.engineMissing"))
    } else if !status.obs_running {
        (ICON_IDLE, t("tray.captureNotStarted"))
    } else if !status.replay_enabled {
        (ICON_IDLE, t("tray.replayPaused"))
    } else {
        (ICON_IDLE, t("tray.replayStopped"))
    };
    if let Some((record, replay)) = state(app).tray_items.lock().unwrap().as_ref() {
        let _ = record.set_text(t(if status.recording {
            "tray.stopRecording"
        } else {
            "tray.startRecording"
        }));
        let _ = record.set_enabled(status.obs_running);
        let _ = replay.set_text(t(if status.replay_enabled {
            "tray.pauseReplay"
        } else {
            "tray.resumeReplay"
        }));
        let _ = replay.set_enabled(status.obs_running);
    }
    if let Ok(image) = Image::from_bytes(icon) {
        let _ = tray.set_icon(Some(image));
    }
    let _ = tray.set_tooltip(Some(&tooltip));
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let gallery = MenuItem::with_id(app, "gallery", t("tray.openGallery"), true, None::<&str>)?;
    let save = MenuItem::with_id(app, "save", t("tray.saveNow"), true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", t("tray.startRecording"), false, None::<&str>)?;
    let replay = MenuItem::with_id(app, "replay", t("tray.pauseReplay"), false, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", t("tray.lastClipFolder"), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", t("tray.settings"), true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", t("tray.quit"), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &gallery,
            &save,
            &record,
            &folder,
            &PredefinedMenuItem::separator(app)?,
            &replay,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_IDLE)?)
        .tooltip(t("tray.starting"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "gallery" => show_main(app, "gallery"),
            "save" => run_action(app, Action::Save),
            "record" => run_action(app, Action::Record),
            "replay" => {
                let enabled = !state(app).status.lock().unwrap().replay_enabled;
                let app = app.clone();
                std::thread::spawn(move || {
                    let _ = set_replay(&app, enabled);
                });
            }
            "folder" => open_last_folder(app),
            "settings" => show_main(app, "settings"),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle(), "gallery");
            }
        })
        .build(app)?;
    *state(app).tray_items.lock().unwrap() = Some((record, replay));
    *state(app).tray_labels.lock().unwrap() = vec![
        (gallery, "tray.openGallery"),
        (save, "tray.saveNow"),
        (folder, "tray.lastClipFolder"),
        (settings, "tray.settings"),
        (quit_item, "tray.quit"),
    ];
    Ok(())
}

// ---------- Parancsok a felületnek ----------

/// A felület csak a mentési mappán belüli fájlokhoz nyúlhat.
fn clip_in_output_dir(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    clips::checked_path(Path::new(&current_settings(app).output_dir), Path::new(path))
}

/// Az aktív nyelv kódja és szövegei a felületnek
#[tauri::command]
fn get_locale() -> serde_json::Value {
    json!({ "lang": i18n::lang(), "messages": i18n::messages() })
}

#[tauri::command]
fn open_project_link(target: String) -> Result<(), String> {
    let url = links::project_url(&target).ok_or_else(|| "Unknown project link".to_string())?;
    platform::open_path(url);
    Ok(())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    current_settings(&app)
}

#[tauri::command]
fn get_status(app: AppHandle) -> Status {
    state(&app).status.lock().unwrap().clone()
}

#[tauri::command]
async fn list_clips(app: AppHandle) -> Vec<Clip> {
    let root = PathBuf::from(current_settings(&app).output_dir);
    let recording = state(&app)
        .engine
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|e| e.recording_path().map(PathBuf::from));
    tauri::async_runtime::spawn_blocking(move || clips::list(&root, recording.as_deref()))
        .await
        .unwrap_or_default()
}

/// Menti a beállításokat; figyelmeztetéssel tér vissza, ha valami csak részben sikerült.
#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<String, String> {
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let st = state(&handle);
        let _operation = st.operations.lock().unwrap();
        if st.installing.load(Ordering::SeqCst) || st.quitting.load(Ordering::SeqCst) {
            return Err(t("update.busy"));
        }
        settings.validate()?;
        parse_hotkeys(&settings)?;
        let old = current_settings(&handle);
        let mut new = settings;
        for (previous, next) in [(&old.output_dir, &new.output_dir), (&old.buffer_dir, &new.buffer_dir)] {
            if previous != next {
                let canonical = Path::new(next).canonicalize().map_err(|e| e.to_string())?;
                if !st.selected_folders.lock().unwrap().contains(&canonical) {
                    return Err(t("validate.pickFolder"));
                }
            }
        }
        new.last_clip = old.last_clip.clone();
        new.replay_enabled = old.replay_enabled;
        let rebuild = old.pipeline_fingerprint() != new.pipeline_fingerprint();
        if rebuild && new.buffer_storage == "disk" && !engine::disk_buffer_available() {
            return Err(t("validate.diskUnavailable"));
        }
        if rebuild && st.save_in_progress.load(Ordering::SeqCst) {
            return Err(t("engine.settingsBusy"));
        }
        let mut slot = st.engine.lock().unwrap();
        if let Some(engine) = slot.as_mut() {
            if rebuild {
                engine.apply(&engine_config(&new))?;
            } else {
                engine.set_desktop_visible(new.capture_desktop);
                engine.set_mic_device(&new.mic_device);
                if let Err(error) = engine.set_mic_enabled(new.mic_mode != "off") {
                    engine.set_desktop_visible(old.capture_desktop);
                    engine.set_mic_device(&old.mic_device);
                    let _ = engine.set_mic_enabled(old.mic_mode != "off");
                    return Err(error);
                }
            }
        } else if rebuild && engine::engine_available() {
            *slot = Some(Engine::start(&engine_config(&new), new.replay_enabled)?);
        }
        if let Err(error) = settings::save(&new) {
            if let Some(engine) = slot.as_mut() {
                if rebuild {
                    let _ = engine.apply(&engine_config(&old));
                } else {
                    engine.set_desktop_visible(old.capture_desktop);
                    engine.set_mic_device(&old.mic_device);
                    let _ = engine.set_mic_enabled(old.mic_mode != "off");
                }
            }
            return Err(tf("error.settingsSave", &[("error", &error)]));
        }
        drop(slot);
        *st.settings.lock().unwrap() = new.clone();
        *st.engine_error.lock().unwrap() = None;
        i18n::set_language(&new.language);
        apply_mic_settings(&new);
        let mut warnings = Vec::new();
        if let Err(error) = register_hotkeys(&handle, &new) {
            warnings.push(error);
        }
        if old.autostart != new.autostart {
            if let Err(error) = platform::set_autostart(new.autostart) {
                warnings.push(tf("error.autostart", &[("error", &error)]));
            }
        }
        let _ = handle.emit("locale-changed", ());
        let status = st.status.lock().unwrap().clone();
        update_tray(&handle, &status);
        if let Some(toast) = handle.get_webview_window("toast") {
            let _ = toast.set_title(&t("toast.windowTitle"));
        }
        Ok(warnings.join("\n"))
    })
    .await
    .map_err(|e| e.to_string())?;
    refresh_status(&app);
    result
}

#[tauri::command]
fn buffer_budget(seconds: u32, bitrate_mbps: u32) -> resources::BufferBudget {
    resources::memory_budget(seconds.min(1200), bitrate_mbps.clamp(5, 150) * 1000, resources::memory())
}

#[derive(Serialize)]
struct Mic {
    id: String,
    name: String,
}

#[tauri::command]
fn list_mics(app: AppHandle) -> Vec<Mic> {
    state(&app)
        .engine
        .lock()
        .unwrap()
        .as_ref()
        .map(|e| e.list_mics().into_iter().map(|(id, name)| Mic { id, name }).collect())
        .unwrap_or_default()
}

#[tauri::command]
fn disk_buffer_available() -> bool {
    engine::disk_buffer_available()
}

#[tauri::command]
fn save_replay(app: AppHandle) -> Result<(), String> {
    request_save(&app)
}

#[tauri::command]
async fn toggle_record(app: AppHandle) {
    let _ = tauri::async_runtime::spawn_blocking(move || toggle_recording(&app)).await;
}

#[tauri::command]
async fn set_replay_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || set_replay(&app, enabled))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_clip(app: AppHandle, path: String) -> Result<(), String> {
    let clip = clip_in_output_dir(&app, &path)?;
    platform::open_path(&clip.to_string_lossy());
    Ok(())
}

#[tauri::command]
fn reveal_clip(app: AppHandle, path: String) -> Result<(), String> {
    let clip = clip_in_output_dir(&app, &path)?;
    platform::reveal(&clip.to_string_lossy());
    Ok(())
}

#[tauri::command]
fn delete_clip(app: AppHandle, path: String) -> Result<(), String> {
    let clip = clip_in_output_dir(&app, &path)?;
    if !platform::recycle(&clip.to_string_lossy()) {
        return Err(t("error.clipDelete"));
    }
    let st = state(&app);
    let mut s = st.settings.lock().unwrap();
    if s.last_clip.as_deref() == Some(path.as_str()) {
        s.last_clip = None;
        let _ = settings::save(&s);
    }
    Ok(())
}

#[tauri::command]
fn open_output_folder(app: AppHandle) {
    platform::open_path(&current_settings(&app).output_dir);
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app.dialog().file().blocking_pick_folder()?.to_string();
        let canonical = Path::new(&selected).canonicalize().ok()?;
        state(&app).selected_folders.lock().unwrap().insert(canonical);
        Some(selected)
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
fn is_ptt_key_supported(vk: u32) -> bool {
    settings::is_ptt_key_supported(vk)
}

/// Gyorsbillentyű-rögzítés közben a meglévők ne süljenek el.
#[tauri::command]
fn suspend_hotkeys(app: AppHandle) {
    if !app
        .get_webview_window("main")
        .is_some_and(|window| window.is_focused().unwrap_or(false))
    {
        return;
    }
    state(&app).hotkeys_suspended.store(true, Ordering::SeqCst);
    let _ = app.global_shortcut().unregister_all();
    state(&app).shortcuts.lock().unwrap().clear();
    let generation = state(&app).hotkey_generation.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(35));
        if state(&app).hotkey_generation.load(Ordering::SeqCst) == generation {
            resume_hotkeys(app);
        }
    });
}

#[tauri::command]
fn resume_hotkeys(app: AppHandle) {
    state(&app).hotkey_generation.fetch_add(1, Ordering::SeqCst);
    if state(&app).hotkeys_suspended.swap(false, Ordering::SeqCst) {
        let _ = register_hotkeys(&app, &current_settings(&app));
    }
}

#[tauri::command]
fn get_update_state(app: AppHandle) -> updater::UpdateState {
    updater::state(&app)
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<updater::UpdateState, String> {
    updater::check(&app).await
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}

// ---------- Önteszt ----------

/// `ClipCat.exe --selftest <mappa> [másodperc]`: felület nélkül elindítja a motort, a megadott idő
/// (alapból 8 s) után ment, és az eredményt a mappába írja (selftest.txt). A normál példánytól
/// függetlenül fut.
fn run_selftest(dir: &str, seconds: u64) -> i32 {
    logfile::init("selftest.log");
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    engine::set_event_handler(move |event| {
        let result = match event {
            engine::Event::Saved(path) => path.ok_or_else(|| "a mentett fájl útvonala nem elérhető".to_string()),
            engine::Event::SaveFailed(e) => Err(e),
            _ => return,
        };
        let _ = tx.lock().unwrap().send(result);
    });
    let mut s = settings::load();
    s.output_dir = dir.to_string();
    let report = |text: String| {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(Path::new(dir).join("selftest.txt"), &text);
        logfile::write(&format!("Önteszt: {text}"));
    };

    let mut engine = match Engine::start(&engine_config(&s), true) {
        Ok(engine) => engine,
        Err(e) => {
            report(format!("HIBA indítás: {e}"));
            return 1;
        }
    };
    std::thread::sleep(Duration::from_secs(seconds));
    let active = engine.replay_active();
    let result = engine.save().and_then(|_| {
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| "nem jött mentési jelzés".to_string())?
    });
    engine.shutdown();
    match result {
        Ok(path) => {
            report(format!("OK aktív={active} fájl={path}"));
            0
        }
        Err(e) => {
            report(format!("HIBA aktív={active}: {e}"));
            1
        }
    }
}

// ---------- Indítás ----------

/// Összeomláskor a hibaüzenet a crash.log-ba kerül (az exe-nek nincs konzolja).
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = format!("{} {info}\n", platform::local_time("%Y.%m.%d %H:%M:%S"));
        logfile::write_crash(&text);
    }));
}

fn main() {
    install_panic_hook();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        let seconds = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(8);
        std::process::exit(run_selftest(&dir, seconds));
    }
    let autostarted = args.iter().any(|a| a == "--autostart");
    let settings = settings::load();

    i18n::set_language(&settings.language);

    // Az állapotot a Builderen kell regisztrálni: a konfigurációban megadott ablakok a setup előtt
    // jönnek létre, és a felület JavaScriptje gyors betöltésnél már a setup előtt parancsokat hív.
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("clipcat", |context, request, responder| {
            let app = context.app_handle().clone();
            let allowed = context.webview_label() == "main";
            std::thread::spawn(move || {
                let root = PathBuf::from(current_settings(&app).output_dir);
                responder.respond(media::response(&root, request, allowed));
            });
        })
        // A webview alapértelmezett helyi menüje (Vissza, Frissítés, Vizsgálat…) sehol ne jelenjen meg
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.eval("document.addEventListener('contextmenu', e => e.preventDefault());");
            }
        })
        .manage(AppState {
            settings: Mutex::new(settings.clone()),
            operations: Mutex::new(()),
            installing: AtomicBool::new(false),
            save_in_progress: AtomicBool::new(false),
            hotkeys_suspended: AtomicBool::new(false),
            hotkey_generation: AtomicU64::new(0),
            selected_folders: Mutex::new(HashSet::new()),
            status: Mutex::new(Status::default()),
            shortcuts: Mutex::new(Vec::new()),
            last_shortcut: Mutex::new([None; 4]),
            tray_items: Mutex::new(None),
            tray_labels: Mutex::new(Vec::new()),
            engine: Mutex::new(None),
            engine_error: Mutex::new(None),
            pending_folder: Mutex::new(None),
            last_recover: Mutex::new(None),
            quitting: AtomicBool::new(false),
            toast_generation: AtomicU64::new(0),
        })
        .manage(updater::Updater::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app, "gallery")))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    logfile::write(&format!("Gyorsbillentyű lenyomva: {}", pretty_hotkey(&shortcut.into_string())));
                    let action = state(app)
                        .shortcuts
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(s, _)| s == shortcut)
                        .map(|(_, a)| *a);
                    if let Some(action) = action {
                        run_shortcut_action(app, action);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // Csak az első példányban fut le; egy második indítás nem nullázza a naplót
            logfile::init("clipcat.log");
            let handle = app.handle().clone();
            let _ = platform::set_autostart(settings.autostart);
            apply_mic_settings(&settings);
            build_tray(&handle)?;

            if let Some(toast) = app.get_webview_window("toast") {
                let _ = toast.set_ignore_cursor_events(true);
                let _ = toast.set_title(&t("toast.windowTitle"));
                platform::prepare_overlay(&toast);
            }
            if let Some(main) = app.get_webview_window("main") {
                let main_handle = main.clone();
                main.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Focused(false) | WindowEvent::CloseRequested { .. }) {
                        resume_hotkeys(main_handle.app_handle().clone());
                    }
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_handle.hide();
                        // A rejtett ablakban ne szóljon tovább a lejátszó: a felület a
                        // "main-hidden" eseményre bezárja; a szüneteltetés csak védőháló
                        let _ = main_handle.eval("document.querySelectorAll('video').forEach(v => v.pause());");
                        let _ = main_handle.emit("main-hidden", ());
                    }
                });
            }

            logfile::write(&format!(
                "Visszajátszás induláskor: {}",
                if settings.replay_enabled {
                    "bekapcsolva"
                } else {
                    "szünetel (legutóbb leállítva)"
                }
            ));
            let hotkey_error = register_hotkeys(&handle, &settings).err();
            #[cfg(windows)]
            start_hotkey_fallback(handle.clone());
            std::thread::spawn(move || {
                platform::migrate_legacy();
                start_engine(&handle);
                if let Some(e) = hotkey_error {
                    show_toast(&handle, "error", &t("toast.hotkey"), &e);
                }
                start_mic_thread(handle.clone());
                start_status_thread(handle);
            });
            updater::start(app.handle().clone());

            if !autostarted {
                show_main(app.handle(), "gallery");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_locale,
            open_project_link,
            get_settings,
            get_status,
            list_clips,
            save_settings,
            save_replay,
            toggle_record,
            set_replay_enabled,
            list_mics,
            disk_buffer_available,
            buffer_budget,
            open_clip,
            reveal_clip,
            delete_clip,
            open_output_folder,
            pick_folder,
            is_ptt_key_supported,
            suspend_hotkeys,
            resume_hotkeys,
            get_update_state,
            check_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("a ClipCat nem indítható");
}
