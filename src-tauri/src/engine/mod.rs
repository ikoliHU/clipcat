//! Beágyazott libobs: az OBS rögzítőmotorját tölti be, felület nélkül.
//!
//! Csak a szükséges modulok töltődnek be (képernyő-/játékrögzítés, hang, hardveres kódoló, replay
//! buffer), a böngésző, websocket és egyéb pluginok nem. A replay buffer a memóriában tartja a kódolt
//! képkockákat; mentéskor az `obs-ffmpeg-mux` segédprogram írja ki őket fájlba. Lemezes módban a
//! puffer darabokban a lemezre íródik (lásd `disk`).
//!
//! A platformfüggő rész (a libobs helye és betöltése, a források és kódolók azonosítói) a `sys`
//! modulban van: Windowson a ClipCat saját motormappája, Linuxon a rendszerre telepített OBS.

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
mod sys;

mod disk;

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::i18n::{t, tf};
use crate::logfile;

const LOG_INFO: c_int = 300;
const OBS_VIDEO_SUCCESS: c_int = 0;
const MODULE_SUCCESS: c_int = 0;
const VIDEO_FORMAT_NV12: c_int = 2;
const VIDEO_CS_709: c_int = 2;
const VIDEO_RANGE_PARTIAL: c_int = 1;
const OBS_SCALE_BICUBIC: c_int = 2;
const SPEAKERS_STEREO: c_int = 2;
const OBS_BOUNDS_SCALE_INNER: c_int = 2;
const CHANNEL_VIDEO: u32 = 0;
const CHANNEL_DESKTOP_AUDIO: u32 = 1;
const CHANNEL_MIC: u32 = 3;

#[repr(C)]
struct VideoInfo {
    graphics_module: *const c_char,
    fps_num: u32,
    fps_den: u32,
    base_width: u32,
    base_height: u32,
    output_width: u32,
    output_height: u32,
    output_format: c_int,
    adapter: u32,
    gpu_conversion: bool,
    colorspace: c_int,
    range: c_int,
    scale_type: c_int,
}

#[repr(C)]
struct AudioInfo {
    samples_per_sec: u32,
    speakers: c_int,
}

/// A libobs `struct vec2` egy __m128 unióval 16 bájtos és 16-ra igazított.
#[repr(C, align(16))]
struct Vec2 {
    x: f32,
    y: f32,
    _pad: [f32; 2],
}

#[repr(C)]
pub struct Calldata {
    stack: *mut u8,
    size: usize,
    capacity: usize,
    fixed: bool,
}

impl Calldata {
    fn new() -> Self {
        Self {
            stack: null_mut(),
            size: 0,
            capacity: 0,
            fixed: false,
        }
    }
}

type Ptr = *mut c_void;
type SignalCallback = unsafe extern "C" fn(Ptr, *mut Calldata);
type LogHandler = unsafe extern "C" fn(c_int, *const c_char, Ptr, Ptr);

macro_rules! obs_api {
    ($($name:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        #[cfg_attr(windows, allow(dead_code))]
        struct Api { $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* }

        impl Api {
            #[cfg(test)]
            fn fake() -> Self {
                Self { $($name: {
                    unsafe extern "C" fn stub($(_: $arg),*) $(-> $ret)? { Default::default() }
                    stub
                },)* }
            }
            unsafe fn load(lib: &libloading::Library) -> Result<Self, String> {
                Ok(Self { $($name: *lib
                    .get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(concat!(stringify!($name), "\0").as_bytes())
                    .map_err(|e| tf("engine.missingFunction", &[("name", &stringify!($name)), ("error", &e)]))?,)* })
            }
        }
    };
}

obs_api! {
    obs_startup: fn(*const c_char, *const c_char, Ptr) -> bool;
    obs_shutdown: fn();
    obs_get_version_string: fn() -> *const c_char;
    obs_add_data_path: fn(*const c_char);
    obs_open_module: fn(*mut Ptr, *const c_char, *const c_char) -> c_int;
    obs_init_module: fn(Ptr) -> bool;
    obs_post_load_modules: fn();
    obs_reset_video: fn(*mut VideoInfo) -> c_int;
    obs_reset_audio: fn(*const AudioInfo) -> bool;
    obs_get_video: fn() -> Ptr;
    obs_get_audio: fn() -> Ptr;
    obs_set_output_source: fn(u32, Ptr);
    obs_data_create: fn() -> Ptr;
    obs_data_release: fn(Ptr);
    obs_data_set_string: fn(Ptr, *const c_char, *const c_char);
    obs_data_set_int: fn(Ptr, *const c_char, i64);
    obs_data_set_bool: fn(Ptr, *const c_char, bool);
    obs_data_get_string: fn(Ptr, *const c_char) -> *const c_char;
    obs_source_get_settings: fn(Ptr) -> Ptr;
    obs_source_create: fn(*const c_char, *const c_char, Ptr, Ptr) -> Ptr;
    obs_source_release: fn(Ptr);
    obs_source_update: fn(Ptr, Ptr);
    obs_source_set_muted: fn(Ptr, bool);
    obs_source_set_audio_mixers: fn(Ptr, u32);
    obs_scene_create: fn(*const c_char) -> Ptr;
    obs_scene_release: fn(Ptr);
    obs_scene_get_source: fn(Ptr) -> Ptr;
    obs_scene_add: fn(Ptr, Ptr) -> Ptr;
    obs_sceneitem_set_bounds_type: fn(Ptr, c_int);
    obs_sceneitem_set_bounds: fn(Ptr, *const Vec2);
    obs_sceneitem_set_visible: fn(Ptr, bool) -> bool;
    obs_video_encoder_create: fn(*const c_char, *const c_char, Ptr, Ptr) -> Ptr;
    obs_audio_encoder_create: fn(*const c_char, *const c_char, Ptr, usize, Ptr) -> Ptr;
    obs_enum_encoder_types: fn(usize, *mut *const c_char) -> bool;
    obs_enum_input_types: fn(usize, *mut *const c_char) -> bool;
    obs_encoder_set_video: fn(Ptr, Ptr);
    obs_encoder_set_audio: fn(Ptr, Ptr);
    obs_encoder_release: fn(Ptr);
    obs_output_create: fn(*const c_char, *const c_char, Ptr, Ptr) -> Ptr;
    obs_output_release: fn(Ptr);
    obs_output_update: fn(Ptr, Ptr);
    obs_output_set_video_encoder: fn(Ptr, Ptr);
    obs_output_set_audio_encoder: fn(Ptr, Ptr, usize);
    obs_output_start: fn(Ptr) -> bool;
    obs_output_stop: fn(Ptr);
    obs_output_force_stop: fn(Ptr);
    obs_output_active: fn(Ptr) -> bool;
    obs_output_get_last_error: fn(Ptr) -> *const c_char;
    obs_output_get_proc_handler: fn(Ptr) -> Ptr;
    obs_output_get_signal_handler: fn(Ptr) -> Ptr;
    obs_source_get_signal_handler: fn(Ptr) -> Ptr;
    proc_handler_call: fn(Ptr, *const c_char, *mut Calldata) -> bool;
    signal_handler_connect: fn(Ptr, *const c_char, SignalCallback, Ptr);
    calldata_get_string: fn(*const Calldata, *const c_char, *mut *const c_char) -> bool;
    calldata_get_data: fn(*const Calldata, *const c_char, Ptr, usize) -> bool;
    bfree: fn(Ptr);
    obs_get_source_properties: fn(*const c_char) -> Ptr;
    obs_properties_get: fn(Ptr, *const c_char) -> Ptr;
    obs_properties_destroy: fn(Ptr);
    obs_property_list_item_count: fn(Ptr) -> usize;
    obs_property_list_item_name: fn(Ptr, usize) -> *const c_char;
    obs_property_list_item_string: fn(Ptr, usize) -> *const c_char;
    base_set_log_handler: fn(LogHandler, Ptr);
}

pub enum Event {
    /// Kész a mentés; az OBS által írt fájl útvonala (None, ha nem kérdezhető le)
    Saved(Option<String>),
    /// A rögzítés leállt; nem nulla hibakód esetén hiba miatt
    Stopped(i64),
    /// Lezárult a kézi felvétel; a fájl útvonala
    Recorded(String),
    /// A kézi felvétel hiba miatt leállt (hibakód)
    RecordingFailed(i64),
    /// A lemezes puffer mentése nem sikerült
    SaveFailed(String),
}

static API: OnceLock<Api> = OnceLock::new();
static EVENTS: OnceLock<Box<dyn Fn(Event) + Send + Sync>> = OnceLock::new();
/// A jelzés-visszahívásokból is el kell érni az aktuális kimenetet és a mikrofont.
static CURRENT_OUTPUT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static MIC_SOURCE: Mutex<usize> = Mutex::new(0);
static MIC_MUTED: AtomicBool = AtomicBool::new(true);
static SAVE_PENDING: AtomicBool = AtomicBool::new(false);
static RECORD_STOP_CODE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

// A rejtett forrás nem dolgozik (a monitorrögzítés elengedi a duplikációt, a játékrögzítés
// lecsatlakozik), ezért csak akkor látható, ha a képére szükség van. A jelzések más szálról
// is módosítják, ezért az állapot statikus.
static DISPLAY_ITEM: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static GAME_ITEM: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static DESKTOP_WANTED: AtomicBool = AtomicBool::new(false);
static REPLAY_WANTED: AtomicBool = AtomicBool::new(false);
static RECORDING_WANTED: AtomicBool = AtomicBool::new(false);
/// A játékrögzítés épp egy (teljes képernyős) játékot rögzít, ami eltakarja az asztalt
static GAME_HOOKED: AtomicBool = AtomicBool::new(false);
static VISIBILITY_LOCK: Mutex<()> = Mutex::new(());

/// A jelenetelemek láthatóságát az állapothoz igazítja.
fn refresh_visibility() {
    let Some(api) = API.get() else { return };
    let _guard = VISIBILITY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (display, game) = (DISPLAY_ITEM.load(Ordering::SeqCst), GAME_ITEM.load(Ordering::SeqCst));
    // Játékrögzítés nem minden platformon van
    if display.is_null() {
        return;
    }
    let capturing = REPLAY_WANTED.load(Ordering::SeqCst) || RECORDING_WANTED.load(Ordering::SeqCst);
    if !capturing {
        // A rejtett játékrögzítés lecsatlakozik; újra megjelenítéskor a hooked jelzés jön újra
        GAME_HOOKED.store(false, Ordering::SeqCst);
    }
    let desktop = capturing && DESKTOP_WANTED.load(Ordering::SeqCst) && !GAME_HOOKED.load(Ordering::SeqCst);
    unsafe {
        if !game.is_null() {
            (api.obs_sceneitem_set_visible)(game, capturing);
        }
        (api.obs_sceneitem_set_visible)(display, desktop);
    }
}

/// A jelzések a libobs saját szálain érkeznek; a jelenetet onnan nem módosítjuk.
fn set_game_hooked(hooked: bool) {
    GAME_HOOKED.store(hooked, Ordering::SeqCst);
    std::thread::spawn(refresh_visibility);
}

unsafe extern "C" fn on_game_hooked(_data: Ptr, _cd: *mut Calldata) {
    set_game_hooked(true);
}

unsafe extern "C" fn on_game_unhooked(_data: Ptr, _cd: *mut Calldata) {
    set_game_hooked(false);
}

pub fn set_event_handler(handler: impl Fn(Event) + Send + Sync + 'static) {
    let _ = EVENTS.set(Box::new(handler));
}

fn emit(event: Event) {
    if let Some(handler) = EVENTS.get() {
        handler(event);
    }
}

/// A libobs és a modulok helye a lemezen (a platform `sys::locate` adja).
pub(crate) struct Layout {
    /// A motor gyökere (a naplóhoz)
    root: PathBuf,
    /// Maga a libobs (obs.dll / libobs.so.0)
    lib: PathBuf,
    /// A libobs adatfájljai (shaderek)
    data: PathBuf,
    /// A modulok binárisai és adatmappái
    plugins: PathBuf,
    plugin_data: PathBuf,
}

impl Layout {
    fn module(&self, name: &str) -> (PathBuf, PathBuf) {
        (
            self.plugins.join(format!("{name}{}", std::env::consts::DLL_SUFFIX)),
            self.plugin_data.join(name),
        )
    }
}

pub fn engine_available() -> bool {
    sys::locate().is_some()
}

/// A lemezes pufferhez kell az ffmpeg (Windowson a motor mellé csomagolva, Linuxon a rendszeré).
pub fn disk_buffer_available() -> bool {
    disk::ffmpeg_available()
}

fn cs(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

fn forward(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A libobs printf-stílusú naplóüzeneteit a saját naplófájlunkba írja.
unsafe extern "C" fn log_handler(level: c_int, format: *const c_char, args: Ptr, _param: Ptr) {
    if level > LOG_INFO {
        return;
    }
    let mut buffer = [0 as c_char; 2048];
    sys::vsnprintf(&mut buffer, format, args);
    logfile::write(&format!("[obs] {}", CStr::from_ptr(buffer.as_ptr()).to_string_lossy()));
}

unsafe extern "C" fn on_saved(_data: Ptr, _cd: *mut Calldata) {
    let output = CURRENT_OUTPUT.load(Ordering::SeqCst);
    let path = API.get().and_then(|api| last_replay(api, output));
    SAVE_PENDING.store(false, Ordering::SeqCst);
    emit(Event::Saved(path));
}

/// Lemezes puffer: a muxer új darabba kezdett.
unsafe extern "C" fn on_segment(_data: Ptr, cd: *mut Calldata) {
    let Some(api) = API.get() else { return };
    let mut path: *const c_char = std::ptr::null();
    if (api.calldata_get_string)(cd, c"next_file".as_ptr(), &mut path) && !path.is_null() {
        disk::segment_started(PathBuf::from(CStr::from_ptr(path).to_string_lossy().into_owned()));
    }
}

unsafe fn stop_code(api: &Api, cd: *mut Calldata) -> i64 {
    let mut code: i64 = 0;
    (api.calldata_get_data)(cd, c"code".as_ptr(), &mut code as *mut i64 as Ptr, size_of::<i64>());
    code
}

unsafe extern "C" fn on_stop(_data: Ptr, cd: *mut Calldata) {
    let Some(api) = API.get() else { return };
    emit(Event::Stopped(stop_code(api, cd)));
}

/// A kézi felvétel szabályos leállítását a stop_recording jelzi; itt csak a hiba számít.
unsafe extern "C" fn on_record_stop(_data: Ptr, cd: *mut Calldata) {
    let Some(api) = API.get() else { return };
    let code = stop_code(api, cd);
    RECORD_STOP_CODE.store(code, Ordering::SeqCst);
    if code != 0 {
        RECORDING_WANTED.store(false, Ordering::SeqCst);
        std::thread::spawn(refresh_visibility);
        emit(Event::RecordingFailed(code));
    }
}

unsafe fn last_error(api: &Api, output: Ptr) -> String {
    let err = (api.obs_output_get_last_error)(output);
    if err.is_null() {
        String::new()
    } else {
        CStr::from_ptr(err).to_string_lossy().into_owned()
    }
}

/// Leállítja a kimenetet, és megvárja, amíg a libobs lezárja (legfeljebb `timeout` ideig).
unsafe fn stop_output(api: &Api, output: Ptr, timeout: Duration) -> bool {
    (api.obs_output_stop)(output);
    let deadline = Instant::now() + timeout;
    while (api.obs_output_active)(output) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let finished = !(api.obs_output_active)(output);
    if !finished {
        (api.obs_output_force_stop)(output);
    }
    finished
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

unsafe fn last_replay(api: &Api, output: Ptr) -> Option<String> {
    if output.is_null() {
        return None;
    }
    let mut cd = Calldata::new();
    let mut path: *const c_char = std::ptr::null();
    let ph = (api.obs_output_get_proc_handler)(output);
    let found = (api.proc_handler_call)(ph, c"get_last_replay".as_ptr(), &mut cd)
        && (api.calldata_get_string)(&cd, c"path".as_ptr(), &mut path)
        && !path.is_null();
    let result = found.then(|| CStr::from_ptr(path).to_string_lossy().into_owned());
    if !cd.stack.is_null() {
        (api.bfree)(cd.stack as Ptr);
    }
    result
}

/// Mikrofon némítása (push-to-talk); hamis, ha a motor még nem fut.
pub fn set_mic_muted(muted: bool) -> bool {
    MIC_MUTED.store(muted, Ordering::SeqCst);
    // Keep this lock through the FFI call; shutdown takes it before releasing the source.
    let mic = MIC_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
    match API.get() {
        Some(api) if *mic != 0 => {
            unsafe { (api.obs_source_set_muted)(*mic as Ptr, muted) };
            true
        }
        _ => false,
    }
}

/// A libobs betöltése; a platform előkészíti a keresési útvonalakat.
fn load_api() -> Result<(&'static Api, Layout), String> {
    let layout = sys::locate().ok_or_else(|| t(sys::NOT_INSTALLED))?;
    if let Some(api) = API.get() {
        return Ok((api, layout));
    }
    logfile::write(&format!("Rögzítőmotor: {}", layout.root.display()));
    let lib = sys::open(&layout)?;
    let api = unsafe { Api::load(&lib)? };
    sys::before_startup(&lib)?;
    // A könyvtár a folyamat végéig betöltve marad
    std::mem::forget(lib);
    let api = API.get_or_init(|| api);
    unsafe { (api.base_set_log_handler)(log_handler, null_mut()) };
    Ok((api, layout))
}

/// obs_data_t építő; a libobs lemásolja az értékeket, a Drop felszabadítja.
struct Data<'a> {
    api: &'a Api,
    ptr: Ptr,
}

impl<'a> Data<'a> {
    fn new(api: &'a Api) -> Self {
        Self {
            api,
            ptr: unsafe { (api.obs_data_create)() },
        }
    }
    fn str(self, key: &str, value: &str) -> Self {
        unsafe { (self.api.obs_data_set_string)(self.ptr, cs(key).as_ptr(), cs(value).as_ptr()) };
        self
    }
    fn int(self, key: &str, value: i64) -> Self {
        unsafe { (self.api.obs_data_set_int)(self.ptr, cs(key).as_ptr(), value) };
        self
    }
    fn bool(self, key: &str, value: bool) -> Self {
        unsafe { (self.api.obs_data_set_bool)(self.ptr, cs(key).as_ptr(), value) };
        self
    }
}

impl Drop for Data<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.obs_data_release)(self.ptr) };
    }
}

#[derive(Clone, PartialEq)]
pub struct Config {
    pub output_dir: String,
    pub buffer_seconds: u32,
    /// Lemezes puffer mappája; None: memóriás replay buffer
    pub buffer_dir: Option<String>,
    pub base: (u32, u32),
    pub output: (u32, u32),
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub hevc: bool,
    pub capture_desktop: bool,
    pub monitor_id: String,
    pub mic_device: String,
    pub mic_enabled: bool,
}

pub struct Engine {
    api: &'static Api,
    config: Config,
    scene: Ptr,
    game: Ptr,
    display: Ptr,
    display_item: Ptr,
    desktop_audio: Ptr,
    mic: Ptr,
    video_encoder: Ptr,
    audio_encoder: Ptr,
    output: Ptr,
    /// Hamis, ha a felhasználó leállította a visszajátszást
    replay_enabled: bool,
    /// A puffer utolsó (újra)indítása, Unix ms
    buffer_since: u64,
    /// Kézi felvétel (ffmpeg_muxer), ugyanazokkal a kódolókkal
    record_output: Ptr,
    record_path: String,
    record_since: u64,
    save_worker: Option<std::thread::JoinHandle<()>>,
    encoder_index: usize,
    encoder_name: String,
    buffer_seconds: u32,
    memory_buffer_bytes: u64,
    initialized: bool,
}

// A libobs objektumai szálbiztosak; az Engine-t Mutex védi.
unsafe impl Send for Engine {}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn registered(enumerate: unsafe extern "C" fn(usize, *mut *const c_char) -> bool, wanted: &str) -> bool {
    let mut index = 0;
    let mut id = std::ptr::null();
    while unsafe { enumerate(index, &mut id) } {
        if !id.is_null() && unsafe { CStr::from_ptr(id) }.to_bytes() == wanted.as_bytes() {
            return true;
        }
        index += 1;
    }
    false
}

impl Engine {
    pub fn saving(&self) -> bool {
        SAVE_PENDING.load(Ordering::SeqCst)
    }
    pub fn encoder_name(&self) -> &str {
        &self.encoder_name
    }
    pub fn effective_buffer_seconds(&self) -> u32 {
        self.buffer_seconds
    }

    fn finish_pending_save(&mut self) {
        if let Some(worker) = self.save_worker.take() {
            let _ = worker.join();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.saving() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        SAVE_PENDING.store(false, Ordering::SeqCst);
    }

    fn try_next_encoder(&mut self) -> bool {
        if self.recording_active() || self.replay_active() {
            return false;
        }
        let next = self.create_video_encoder();
        if next.is_null() {
            return false;
        }
        unsafe {
            (self.api.obs_encoder_set_video)(next, (self.api.obs_get_video)());
            if !self.output.is_null() {
                (self.api.obs_output_set_video_encoder)(self.output, next);
            }
            if !self.record_output.is_null() {
                (self.api.obs_output_set_video_encoder)(self.record_output, next);
            }
            if !self.video_encoder.is_null() {
                (self.api.obs_encoder_release)(self.video_encoder);
            }
        }
        self.video_encoder = next;
        true
    }

    /// WASAPI/Pulse sources open devices when created; muting alone does not release them.
    fn sync_audio(&mut self) -> Result<(), String> {
        let capturing = REPLAY_WANTED.load(Ordering::SeqCst) || RECORDING_WANTED.load(Ordering::SeqCst);
        let api = self.api;
        if capturing && self.desktop_audio.is_null() {
            self.desktop_audio = self.create_source(
                sys::DESKTOP_AUDIO_SOURCE,
                "Desktop audio",
                Data::new(api).str("device_id", "default"),
            );
            if self.desktop_audio.is_null() {
                return Err(tf("engine.sourceCreate", &[("id", &sys::DESKTOP_AUDIO_SOURCE)]));
            }
            unsafe {
                (api.obs_source_set_audio_mixers)(self.desktop_audio, 1);
                (api.obs_set_output_source)(CHANNEL_DESKTOP_AUDIO, self.desktop_audio);
            }
        } else if !capturing && !self.desktop_audio.is_null() {
            unsafe {
                (api.obs_set_output_source)(CHANNEL_DESKTOP_AUDIO, null_mut());
                (api.obs_source_release)(self.desktop_audio);
            }
            self.desktop_audio = null_mut();
        }
        let mut mic = MIC_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
        if capturing && self.config.mic_enabled && self.mic.is_null() {
            self.mic = self.create_source(
                sys::MIC_SOURCE,
                "Microphone",
                Data::new(api).str("device_id", &self.config.mic_device),
            );
            if self.mic.is_null() {
                return Err(tf("engine.sourceCreate", &[("id", &sys::MIC_SOURCE)]));
            }
            unsafe {
                (api.obs_source_set_audio_mixers)(self.mic, 1);
                (api.obs_source_set_muted)(self.mic, MIC_MUTED.load(Ordering::SeqCst));
                (api.obs_set_output_source)(CHANNEL_MIC, self.mic);
            }
            *mic = self.mic as usize;
        } else if (!capturing || !self.config.mic_enabled) && !self.mic.is_null() {
            *mic = 0;
            unsafe {
                (api.obs_set_output_source)(CHANNEL_MIC, null_mut());
                (api.obs_source_release)(self.mic);
            }
            self.mic = null_mut();
        }
        Ok(())
    }

    pub fn set_mic_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.config.mic_enabled = enabled;
        self.sync_audio()
    }

    pub fn check_resources(&mut self) -> Result<(), String> {
        // A spontaneous muxer failure also closes idle audio devices on the next tick.
        self.sync_audio()?;
        let result = if self.recording_active() {
            crate::resources::require_space(Path::new(&self.config.output_dir), crate::resources::MIN_FREE_BYTES)
        } else {
            Ok(())
        }
        .and_then(|_| {
            if self.replay_active() && self.config.buffer_dir.is_some() {
                disk::health()
            } else {
                Ok(())
            }
        });
        if result.is_err() {
            disk::cancel_save();
            self.stop_replay();
            let _ = self.stop_recording();
            set_replay_wanted(false);
            let _ = self.sync_audio();
        }
        result
    }
    /// Elindítja a libobs-t; folyamatonként egyszer hívható.
    /// `replay`: induljon-e rögtön a visszajátszási puffer.
    pub fn start(config: &Config, replay: bool) -> Result<Engine, String> {
        let (api, layout) = load_api()?;
        unsafe {
            let plugin_config = forward(&logfile::state_dir().join("plugin_config"));
            if !(api.obs_startup)(c"en-US".as_ptr(), cs(&plugin_config).as_ptr(), null_mut()) {
                return Err(t("engine.startup"));
            }
        }

        let mut engine = Engine {
            api,
            config: config.clone(),
            scene: null_mut(),
            game: null_mut(),
            display: null_mut(),
            display_item: null_mut(),
            desktop_audio: null_mut(),
            mic: null_mut(),
            video_encoder: null_mut(),
            audio_encoder: null_mut(),
            output: null_mut(),
            replay_enabled: replay,
            buffer_since: 0,
            record_output: null_mut(),
            record_path: String::new(),
            record_since: 0,
            save_worker: None,
            encoder_index: 0,
            encoder_name: String::new(),
            buffer_seconds: config.buffer_seconds,
            memory_buffer_bytes: 0,
            initialized: true,
        };
        unsafe {
            let version = CStr::from_ptr((api.obs_get_version_string)()).to_string_lossy();
            logfile::write(&format!("libobs {version} elindult"));
            (api.obs_add_data_path)(cs(&format!("{}/", forward(&layout.data))).as_ptr());

            let audio = AudioInfo {
                samples_per_sec: 48000,
                speakers: SPEAKERS_STEREO,
            };
            if !(api.obs_reset_audio)(&audio) {
                return Err(t("engine.audioInit"));
            }
            reset_video(api, config)?;
            load_modules(api, &layout)?;
            (api.obs_post_load_modules)();
        }
        engine.create_sources()?;
        engine.build_pipeline()?;
        Ok(engine)
    }

    fn create_source(&self, id: &str, name: &str, settings: Data) -> Ptr {
        if !registered(self.api.obs_enum_input_types, id) {
            return null_mut();
        }
        unsafe { (self.api.obs_source_create)(cs(id).as_ptr(), cs(name).as_ptr(), settings.ptr, null_mut()) }
    }

    fn create_sources(&mut self) -> Result<(), String> {
        let api = self.api;
        let required = |source: Ptr, id: &str| {
            if source.is_null() {
                Err(tf("engine.sourceCreate", &[("id", &id)]))
            } else {
                Ok(source)
            }
        };
        // Az első létrehozható képernyőrögzítő (a régebbi OBS-ekben más azonosítóval)
        let display = sys::display_sources()
            .iter()
            .map(|id| self.create_source(id, "Asztal", sys::display_settings(Data::new(api), &self.config.monitor_id)))
            .find(|source| !source.is_null())
            .unwrap_or(null_mut());
        self.display = required(display, sys::display_sources()[0])?;
        if let Some(id) = sys::GAME_SOURCE {
            self.game = required(self.create_source(id, "Játék", sys::game_settings(Data::new(api))), id)?;
        }
        unsafe {
            self.scene = (api.obs_scene_create)(c"Felvétel".as_ptr());
            if self.scene.is_null() {
                return Err(tf("engine.sourceCreate", &[("id", &"scene")]));
            }
            // Hozzáadási sorrend = rétegek alulról felfelé: a játék takarja az asztalt
            self.display_item = (api.obs_scene_add)(self.scene, self.display);
            let game_item = if self.game.is_null() {
                null_mut()
            } else {
                (api.obs_scene_add)(self.scene, self.game)
            };
            let bounds = Vec2 {
                x: self.config.base.0 as f32,
                y: self.config.base.1 as f32,
                _pad: [0.0; 2],
            };
            for item in [self.display_item, game_item] {
                if !item.is_null() {
                    (api.obs_sceneitem_set_bounds_type)(item, OBS_BOUNDS_SCALE_INNER);
                    (api.obs_sceneitem_set_bounds)(item, &bounds);
                }
            }
            if !self.game.is_null() {
                let game_signals = (api.obs_source_get_signal_handler)(self.game);
                (api.signal_handler_connect)(game_signals, c"hooked".as_ptr(), on_game_hooked, null_mut());
                (api.signal_handler_connect)(game_signals, c"unhooked".as_ptr(), on_game_unhooked, null_mut());
            }
            GAME_HOOKED.store(false, Ordering::SeqCst);
            DISPLAY_ITEM.store(self.display_item, Ordering::SeqCst);
            GAME_ITEM.store(game_item, Ordering::SeqCst);
            DESKTOP_WANTED.store(self.config.capture_desktop, Ordering::SeqCst);
            REPLAY_WANTED.store(self.replay_enabled, Ordering::SeqCst);
            RECORDING_WANTED.store(false, Ordering::SeqCst);
            refresh_visibility();

            (api.obs_set_output_source)(CHANNEL_VIDEO, (api.obs_scene_get_source)(self.scene));
        }
        self.sync_audio()?;
        Ok(())
    }

    /// Az első elérhető hardveres kódoló (NVENC, Linuxon VAAPI is), ennek hiányában x264.
    fn create_video_encoder(&mut self) -> Ptr {
        let api = self.api;
        let c = &self.config;
        let candidates: Vec<_> = sys::hardware_encoders(c.hevc).iter().copied().chain(["obs_x264"]).collect();
        while self.encoder_index < candidates.len() {
            let id = candidates[self.encoder_index];
            self.encoder_index += 1;
            if !registered(api.obs_enum_encoder_types, id) {
                continue;
            }
            let settings = if id.starts_with("obs_nvenc") {
                Data::new(api)
                    .str("rate_control", "CBR")
                    .int("bitrate", c.bitrate_kbps as i64)
                    .int("keyint_sec", 2)
                    // A p4 a minőség és a kódolóterhelés legjobb egyensúlya; ekkora bitrátán a
                    // pszichovizuális AQ és a B-képkockák alig javítanak a képen, a folyamatos
                    // pufferelésnél viszont sokszorosára növelik a kódoló terhelését
                    // Az OBS 31+ obs-nvenc kulcsai: a régi preset2/psycho_aq hatástalan
                    .str("preset", "p4")
                    .str("tune", "hq")
                    .str("multipass", "disabled")
                    .str("profile", if c.hevc { "main" } else { "high" })
                    .int("bf", 0)
                    .bool("adaptive_quantization", false)
                    .bool("lookahead", false)
            } else {
                Data::new(api)
                    .str("preset", "veryfast")
                    .str("rate_control", "CBR")
                    .int("bitrate", c.bitrate_kbps as i64)
                    .int("keyint_sec", 2)
                    .int("bf", 0)
            };
            let encoder = unsafe { (api.obs_video_encoder_create)(cs(id).as_ptr(), c"clipcat_video".as_ptr(), settings.ptr, null_mut()) };
            if !encoder.is_null() {
                self.encoder_name = id.to_string();
                logfile::write(&format!("Videókódoló: {id}"));
                return encoder;
            }
        }
        null_mut()
    }

    fn build_pipeline(&mut self) -> Result<(), String> {
        let api = self.api;
        let c = self.config.clone();
        self.encoder_index = 0;
        let budget = crate::resources::memory_budget(c.buffer_seconds, c.bitrate_kbps, crate::resources::memory());
        if c.buffer_dir.is_none() && budget.seconds < 10 {
            return Err(t("engine.memoryLow"));
        }
        self.buffer_seconds = if c.buffer_dir.is_none() { budget.seconds } else { c.buffer_seconds };
        self.memory_buffer_bytes = if c.buffer_dir.is_none() { budget.max_mb * 1024 * 1024 } else { 0 };
        let _ = std::fs::create_dir_all(&c.output_dir);
        unsafe {
            self.video_encoder = self.create_video_encoder();
            if self.video_encoder.is_null() {
                return Err(t("engine.noVideoEncoder"));
            }
            (api.obs_encoder_set_video)(self.video_encoder, (api.obs_get_video)());

            let audio_settings = Data::new(api).int("bitrate", 192);
            self.audio_encoder =
                (api.obs_audio_encoder_create)(c"ffmpeg_aac".as_ptr(), c"clipcat_audio".as_ptr(), audio_settings.ptr, 0, null_mut());
            if self.audio_encoder.is_null() {
                return Err(t("engine.audioEncoder"));
            }
            (api.obs_encoder_set_audio)(self.audio_encoder, (api.obs_get_audio)());

            self.output = match &c.buffer_dir {
                Some(dir) => create_disk_output(api, Path::new(dir)),
                None => create_memory_output(api, &c, budget),
            };
            if self.output.is_null() {
                return Err(t("engine.replayCreate"));
            }
            (api.obs_output_set_video_encoder)(self.output, self.video_encoder);
            (api.obs_output_set_audio_encoder)(self.output, self.audio_encoder, 0);
            let signals = (api.obs_output_get_signal_handler)(self.output);
            if c.buffer_dir.is_some() {
                (api.signal_handler_connect)(signals, c"file_changed".as_ptr(), on_segment, null_mut());
            } else {
                (api.signal_handler_connect)(signals, c"saved".as_ptr(), on_saved, null_mut());
            }
            (api.signal_handler_connect)(signals, c"stop".as_ptr(), on_stop, null_mut());
            CURRENT_OUTPUT.store(self.output, Ordering::SeqCst);
        }
        if self.replay_enabled {
            self.start_replay()?;
        }
        logfile::write(&format!(
            "Rögzítés: {}x{} @ {} FPS, {} kbps, {} s puffer ({})",
            c.output.0,
            c.output.1,
            c.fps,
            c.bitrate_kbps,
            c.buffer_seconds,
            c.buffer_dir.as_deref().map_or("memória".to_string(), |d| format!("lemez: {d}"))
        ));
        Ok(())
    }

    fn start_replay(&mut self) -> Result<(), String> {
        let api = self.api;
        if self.output.is_null() {
            return Err(t("engine.replayNotCreated"));
        }
        if let Some(dir) = &self.config.buffer_dir {
            // Minden indítás új, üres darabsorral kezd
            let first = disk::begin(Path::new(dir), self.config.buffer_seconds, self.config.bitrate_kbps)?;
            let settings = Data::new(api)
                .str("path", &forward(&first))
                .str("directory", &forward(first.parent().unwrap()));
            unsafe { (api.obs_output_update)(self.output, settings.ptr) };
        }
        unsafe {
            // Közvetlenül egy leállítás után a libobs még zárhatja az előző adatfolyamot
            loop {
                if (api.obs_output_start)(self.output) {
                    self.buffer_since = now_ms();
                    return Ok(());
                }
                if !self.try_next_encoder() {
                    break;
                }
            }
            let detail = last_error(api, self.output);
            if self.config.buffer_dir.is_some() {
                disk::end();
            }
            Err(tf("engine.replayStart", &[("detail", &detail)]).trim().to_string())
        }
    }

    fn stop_replay(&mut self) {
        self.finish_pending_save();
        if !self.output.is_null() {
            unsafe { stop_output(self.api, self.output, Duration::from_secs(5)) };
        }
        if self.config.buffer_dir.is_some() {
            disk::end();
        }
        self.buffer_since = 0;
    }

    /// A visszajátszási puffer ki-/bekapcsolása; a kézi felvételt nem érinti.
    pub fn set_replay_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if self.saving() {
            return Err(t("engine.saveBusy"));
        }
        if !enabled {
            self.replay_enabled = false;
            self.stop_replay();
            set_replay_wanted(false);
            self.sync_audio()
        } else if self.replay_active() {
            self.replay_enabled = true;
            Ok(())
        } else {
            set_replay_wanted(true);
            let result = self.sync_audio().and_then(|_| self.start_replay());
            if result.is_err() {
                set_replay_wanted(false);
                let _ = self.sync_audio();
            } else {
                self.replay_enabled = true;
            }
            result
        }
    }

    /// Mentés után üríti a puffert: a következő klip csak az innentől rögzítetteket tartalmazza.
    /// A kódolók futva maradnak, így egy közben zajló kézi felvétel nem szakad meg.
    pub fn clear_replay(&mut self) -> Result<(), String> {
        if !self.replay_active() {
            return Ok(());
        }
        // Lemezen elég a lezárt darabokat törölni, a kimenet futhat tovább
        if self.config.buffer_dir.is_some() {
            disk::clear();
            self.buffer_since = now_ms();
            return Ok(());
        }
        self.stop_replay();
        self.start_replay()
    }

    pub fn start_recording(&mut self, path: &Path) -> Result<(), String> {
        if self.recording_active() {
            return Err(t("engine.recordingRunning"));
        }
        self.release_record_output();
        if self.video_encoder.is_null() || self.audio_encoder.is_null() {
            return Err(t("error.engineNotRunning"));
        }
        let api = self.api;
        crate::resources::require_space(path.parent().unwrap_or(path), crate::resources::MIN_FREE_BYTES)?;
        // Fragmented MP4 survives an interrupted process without a final moov rewrite.
        let settings = Data::new(api).str("path", &forward(path)).str(
            "muxer_settings",
            "movflags=frag_keyframe+empty_moov+default_base_moof flush_packets=1",
        );
        set_recording_wanted(true);
        if let Err(error) = self.sync_audio() {
            set_recording_wanted(false);
            let _ = self.sync_audio();
            return Err(error);
        }
        RECORD_STOP_CODE.store(0, Ordering::SeqCst);
        unsafe {
            let output = (api.obs_output_create)(c"ffmpeg_muxer".as_ptr(), c"clipcat_record".as_ptr(), settings.ptr, null_mut());
            if output.is_null() {
                set_recording_wanted(false);
                let _ = self.sync_audio();
                return Err(t("engine.recordingCreate"));
            }
            (api.obs_output_set_video_encoder)(output, self.video_encoder);
            (api.obs_output_set_audio_encoder)(output, self.audio_encoder, 0);
            let signals = (api.obs_output_get_signal_handler)(output);
            (api.signal_handler_connect)(signals, c"stop".as_ptr(), on_record_stop, null_mut());
            self.record_output = output;
            while !(api.obs_output_start)(output) {
                if self.try_next_encoder() {
                    (api.obs_output_set_video_encoder)(output, self.video_encoder);
                    continue;
                }
                let detail = last_error(api, output);
                self.release_record_output();
                set_recording_wanted(false);
                let _ = self.sync_audio();
                return Err(tf("engine.recordingStart", &[("detail", &detail)]).trim().to_string());
            }
        }
        self.record_path = path.to_string_lossy().into_owned();
        self.record_since = now_ms();
        logfile::write(&format!("Felvétel indult: {}", self.record_path));
        Ok(())
    }

    /// Leállítja a kézi felvételt; a lezárt fájlról `Recorded` eseményt küld.
    pub fn stop_recording(&mut self) -> Result<(), String> {
        if !self.recording_active() {
            return Ok(());
        }
        // A muxer a leállításkor írja ki a fájl végét, ez hosszú felvételnél eltarthat egy ideig
        let finished = unsafe { stop_output(self.api, self.record_output, Duration::from_secs(30)) };
        self.release_record_output();
        set_recording_wanted(false);
        let path = std::mem::take(&mut self.record_path);
        self.sync_audio()?;
        if !finished || RECORD_STOP_CODE.load(Ordering::SeqCst) != 0 || !std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
            return Err(tf("engine.recordingFinalize", &[("path", &path)]));
        }
        logfile::write(&format!("Felvétel leállt: {path}"));
        emit(Event::Recorded(path));
        Ok(())
    }

    fn release_record_output(&mut self) {
        if !self.record_output.is_null() {
            unsafe { (self.api.obs_output_release)(self.record_output) };
            self.record_output = null_mut();
        }
        self.record_since = 0;
    }

    pub fn recording_active(&self) -> bool {
        !self.record_output.is_null() && unsafe { (self.api.obs_output_active)(self.record_output) }
    }

    /// A futó felvétel fájlja (a galéria ezt még nem mutatja)
    pub fn recording_path(&self) -> Option<&str> {
        self.recording_active().then_some(self.record_path.as_str())
    }

    pub fn recording_since(&self) -> u64 {
        if self.recording_active() {
            self.record_since
        } else {
            0
        }
    }

    pub fn replay_enabled(&self) -> bool {
        self.replay_enabled
    }

    pub fn buffer_since(&self) -> u64 {
        if self.replay_active() {
            self.buffer_since
        } else {
            0
        }
    }

    fn teardown_pipeline(&mut self) {
        self.finish_pending_save();
        let _ = self.stop_recording();
        self.release_record_output();
        let api = self.api;
        unsafe {
            if !self.output.is_null() {
                stop_output(api, self.output, Duration::from_secs(5));
                if self.config.buffer_dir.is_some() {
                    disk::end();
                }
                self.buffer_since = 0;
                CURRENT_OUTPUT.store(null_mut(), Ordering::SeqCst);
                (api.obs_output_release)(self.output);
                self.output = null_mut();
            }
            for encoder in [&mut self.video_encoder, &mut self.audio_encoder] {
                if !encoder.is_null() {
                    (api.obs_encoder_release)(*encoder);
                    *encoder = null_mut();
                }
            }
        }
    }

    /// Új beállítások alkalmazása: a puffert újraépíti, szükség esetén a videót is újrainicializálja.
    pub fn apply(&mut self, config: &Config) -> Result<(), String> {
        if self.recording_active() || self.saving() {
            return Err(t("engine.settingsBusy"));
        }
        let old = self.config.clone();
        if let Err(error) = self.apply_inner(config) {
            // Only persist a new configuration after it works. Restore the previous pipeline.
            if let Err(rollback) = self.apply_inner(&old) {
                return Err(format!("{error}\nRollback: {rollback}"));
            }
            return Err(error);
        }
        Ok(())
    }

    fn apply_inner(&mut self, config: &Config) -> Result<(), String> {
        self.teardown_pipeline();
        let old = std::mem::replace(&mut self.config, config.clone());
        let api = self.api;
        if (old.base, old.output, old.fps) != (config.base, config.output, config.fps) {
            reset_video(api, config)?;
        }
        if old.monitor_id != config.monitor_id {
            let settings = sys::display_settings(Data::new(api), &config.monitor_id);
            unsafe { (api.obs_source_update)(self.display, settings.ptr) };
        }
        self.set_desktop_visible(config.capture_desktop);
        if old.mic_device != config.mic_device {
            self.update_mic(&config.mic_device);
        }
        self.sync_audio()?;
        self.build_pipeline()
    }

    /// Mikrofonváltás; a puffert nem kell hozzá újraépíteni.
    pub fn set_mic_device(&mut self, device: &str) {
        if self.config.mic_device != device {
            self.config.mic_device = device.to_string();
            self.update_mic(device);
        }
    }

    fn update_mic(&self, device: &str) {
        if self.mic.is_null() {
            return;
        }
        let settings = Data::new(self.api).str("device_id", device);
        unsafe { (self.api.obs_source_update)(self.mic, settings.ptr) };
        logfile::write(&format!("Mikrofon: {device}"));
    }

    /// A választható mikrofonok (azonosító, név); az alapértelmezett eszköz nélkül.
    pub fn list_mics(&self) -> Vec<(String, String)> {
        let api = self.api;
        let mut list = Vec::new();
        unsafe {
            let props = (api.obs_get_source_properties)(cs(sys::MIC_SOURCE).as_ptr());
            if props.is_null() {
                return list;
            }
            let prop = (api.obs_properties_get)(props, c"device_id".as_ptr());
            if !prop.is_null() {
                for i in 0..(api.obs_property_list_item_count)(prop) {
                    let id = (api.obs_property_list_item_string)(prop, i);
                    let name = (api.obs_property_list_item_name)(prop, i);
                    if id.is_null() || name.is_null() {
                        continue;
                    }
                    let id = CStr::from_ptr(id).to_string_lossy().into_owned();
                    if id != "default" {
                        list.push((id, CStr::from_ptr(name).to_string_lossy().into_owned()));
                    }
                }
            }
            (api.obs_properties_destroy)(props);
        }
        list
    }

    /// Újraindítja a puffert változatlan beállításokkal (pl. hiba miatti leállás után).
    pub fn restart(&mut self) -> Result<(), String> {
        // Outputs share encoders: recovering replay must never stop manual recording.
        if self.saving() {
            return Err(t("engine.saveBusy"));
        }
        self.stop_replay();
        set_replay_wanted(true);
        let result = self.sync_audio().and_then(|_| self.start_replay());
        if result.is_err() {
            set_replay_wanted(false);
            let _ = self.sync_audio();
        }
        result
    }

    pub fn set_desktop_visible(&mut self, visible: bool) {
        self.config.capture_desktop = visible;
        DESKTOP_WANTED.store(visible, Ordering::SeqCst);
        refresh_visibility();
    }

    pub fn replay_active(&self) -> bool {
        !self.output.is_null() && unsafe { (self.api.obs_output_active)(self.output) }
    }

    /// A mentés háttérben készül el; az eredmény `Saved` (vagy `SaveFailed`) eseményként jön.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.replay_active() {
            return Err(t("engine.replayNotRunning"));
        }
        if SAVE_PENDING.swap(true, Ordering::SeqCst) {
            return Err(t("engine.saveBusy"));
        }
        crate::resources::require_space(
            Path::new(&self.config.output_dir),
            crate::resources::MIN_FREE_BYTES.saturating_add(self.memory_buffer_bytes),
        )
        .inspect_err(|_| {
            SAVE_PENDING.store(false, Ordering::SeqCst);
        })?;
        let known = if self.config.buffer_dir.is_some() {
            Some(disk::begin_save().inspect_err(|_| {
                SAVE_PENDING.store(false, Ordering::SeqCst);
            })?)
        } else {
            None
        };
        let proc_name = if known.is_some() { c"split_file" } else { c"save" };
        let mut cd = Calldata::new();
        let ok = unsafe {
            let ph = (self.api.obs_output_get_proc_handler)(self.output);
            let ok = (self.api.proc_handler_call)(ph, proc_name.as_ptr(), &mut cd);
            if !cd.stack.is_null() {
                (self.api.bfree)(cd.stack as Ptr);
            }
            ok
        };
        let Some(known) = known else {
            return if ok {
                Ok(())
            } else {
                SAVE_PENDING.store(false, Ordering::SeqCst);
                Err(t("engine.saveStart"))
            };
        };
        if !ok {
            drop(known);
            SAVE_PENDING.store(false, Ordering::SeqCst);
            return Err(t("engine.saveStart"));
        }
        // A lezárásra várás és az összefűzés másodpercekig tart: nem tartja a motor zárát
        let (seconds, output_dir) = (self.config.buffer_seconds, PathBuf::from(&self.config.output_dir));
        self.save_worker = Some(std::thread::spawn(move || {
            let result = disk::finish_save(known, seconds, &output_dir);
            SAVE_PENDING.store(false, Ordering::SeqCst);
            match result {
                Ok(path) => emit(Event::Saved(Some(path.to_string_lossy().into_owned()))),
                Err(e) => emit(Event::SaveFailed(e)),
            }
        }));
        Ok(())
    }

    pub fn shutdown(self) {
        drop(self);
    }

    fn cleanup(&mut self) {
        if !self.initialized {
            return;
        }
        self.initialized = false;
        let api = self.api;
        self.teardown_pipeline();
        REPLAY_WANTED.store(false, Ordering::SeqCst);
        RECORDING_WANTED.store(false, Ordering::SeqCst);
        if !self.display.is_null() {
            sys::persist_display(api, self.display);
        }
        let mut mic_guard = MIC_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
        *mic_guard = 0;
        {
            let _guard = VISIBILITY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            DISPLAY_ITEM.store(null_mut(), Ordering::SeqCst);
            GAME_ITEM.store(null_mut(), Ordering::SeqCst);
        }
        unsafe {
            for channel in [CHANNEL_VIDEO, CHANNEL_DESKTOP_AUDIO, CHANNEL_MIC] {
                (api.obs_set_output_source)(channel, null_mut());
            }
            if !self.scene.is_null() {
                (api.obs_scene_release)(self.scene);
            }
            for source in [self.game, self.display, self.desktop_audio, self.mic] {
                if !source.is_null() {
                    (api.obs_source_release)(source);
                }
            }
            (api.obs_shutdown)();
        }
        logfile::write("libobs leállítva");
    }
}

#[cfg(test)]
mod tests;

/// Memóriás replay buffer; mentéskor a libobs írja ki a fájlt a mentési mappába.
unsafe fn create_memory_output(api: &Api, c: &Config, budget: crate::resources::BufferBudget) -> Ptr {
    let settings = Data::new(api)
        .str("directory", &forward(Path::new(&c.output_dir)))
        .str("format", "Replay %CCYY-%MM-%DD %hh-%mm-%ss")
        .str("extension", "mp4")
        .bool("allow_spaces", true)
        .int("max_time_sec", budget.seconds as i64)
        .int("max_size_mb", budget.max_mb as i64);
    (api.obs_output_create)(c"replay_buffer".as_ptr(), c"clipcat_replay".as_ptr(), settings.ptr, null_mut())
}

/// Lemezes puffer: folyamatos felvétel rövid darabokra bontva; az első darab útvonalát indításkor kapja.
unsafe fn create_disk_output(api: &Api, dir: &Path) -> Ptr {
    let settings = Data::new(api)
        .str("directory", &forward(dir))
        .str("format", &format!("{}%CCYY-%MM-%DD %hh-%mm-%ss", disk::PREFIX))
        .str("extension", disk::EXTENSION)
        .bool("allow_spaces", true)
        .bool("split_file", true)
        .int("max_time_sec", disk::SEGMENT_SECONDS)
        .int("max_size_mb", 0)
        .str("muxer_settings", "");
    (api.obs_output_create)(c"ffmpeg_muxer".as_ptr(), c"clipcat_replay".as_ptr(), settings.ptr, null_mut())
}

fn set_replay_wanted(wanted: bool) {
    REPLAY_WANTED.store(wanted, Ordering::SeqCst);
    refresh_visibility();
}

fn set_recording_wanted(wanted: bool) {
    RECORDING_WANTED.store(wanted, Ordering::SeqCst);
    refresh_visibility();
}

fn reset_video(api: &Api, config: &Config) -> Result<(), String> {
    let mut info = VideoInfo {
        graphics_module: sys::GRAPHICS_MODULE.as_ptr(),
        fps_num: config.fps,
        fps_den: 1,
        base_width: config.base.0,
        base_height: config.base.1,
        output_width: config.output.0,
        output_height: config.output.1,
        output_format: VIDEO_FORMAT_NV12,
        adapter: 0,
        gpu_conversion: true,
        colorspace: VIDEO_CS_709,
        range: VIDEO_RANGE_PARTIAL,
        scale_type: OBS_SCALE_BICUBIC,
    };
    let result = unsafe { (api.obs_reset_video)(&mut info) };
    if result == OBS_VIDEO_SUCCESS {
        Ok(())
    } else {
        Err(tf("engine.videoInit", &[("code", &result)]))
    }
}

unsafe fn load_modules(api: &Api, layout: &Layout) -> Result<(), String> {
    for (name, required) in sys::modules() {
        let (bin, data) = layout.module(name);
        let loaded = bin.exists() && {
            let mut module: Ptr = null_mut();
            (api.obs_open_module)(&mut module, cs(&forward(&bin)).as_ptr(), cs(&forward(&data)).as_ptr()) == MODULE_SUCCESS
                && (api.obs_init_module)(module)
        };
        logfile::write(&format!("Modul {name}: {}", if loaded { "betöltve" } else { "NEM töltődött be" }));
        if !loaded && required {
            return Err(tf("engine.moduleLoad", &[("name", &name)]));
        }
    }
    Ok(())
}
