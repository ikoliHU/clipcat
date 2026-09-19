//! Linux: a rendszerre telepített OBS Studio (disztribúciós csomag, a deb/rpm függősége) libobs-a.
//! X11-en xshm képernyőrögzítés, Waylandon a PipeWire-portál (az első indításkor a rendszer
//! megkérdezi, melyik képernyőt; a választást a portál visszaállító tokenje megjegyzi).
//! PulseAudio/PipeWire hang, NVENC vagy VAAPI kódolás. Játékrögzítés (hook) Linuxon nincs.

use super::{cs, Api, Data, Layout, Ptr};
use crate::i18n::tf;
use crate::logfile;
use libloading::Library;
use std::ffi::{c_char, c_int, CStr};
use std::path::PathBuf;

pub const NOT_INSTALLED: &str = "engine.notInstalledSystem";
pub const GRAPHICS_MODULE: &CStr = c"libobs-opengl";
pub const GAME_SOURCE: Option<&str> = None;
pub const DESKTOP_AUDIO_SOURCE: &str = "pulse_output_capture";
pub const MIC_SOURCE: &str = "pulse_input_capture";

const OBS_NIX_PLATFORM_X11_EGL: c_int = 1;
const OBS_NIX_PLATFORM_WAYLAND: c_int = 2;
const PIPEWIRE_SOURCE: &str = "pipewire-screen-capture-source";
/// A PipeWire-portál visszaállító tokenje: ezzel a következő indításkor nem kérdez újra
const RESTORE_TOKEN_FILE: &str = "pipewire-restore-token";

/// A libobs lehetséges helyei: (könyvtármappa, telepítési előtag)
const LIB_DIRS: &[(&str, &str)] = &[
    ("/usr/lib/x86_64-linux-gnu", "/usr"),
    ("/usr/lib/aarch64-linux-gnu", "/usr"),
    ("/usr/lib64", "/usr"),
    ("/usr/lib", "/usr"),
    ("/usr/local/lib", "/usr/local"),
    ("/usr/local/lib64", "/usr/local"),
];

/// A képernyőrögzítő forrás azonosítói, az elsőként létrehozhatóval
pub fn display_sources() -> &'static [&'static str] {
    if wayland() { &[PIPEWIRE_SOURCE] } else { &["xshm_input_v2", "xshm_input"] }
}

/// Wayland-munkamenet: a libobs is Waylandon fut, és csak a portálon keresztül rögzíthet képernyőt
fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

pub fn locate() -> Option<Layout> {
    LIB_DIRS.iter().find_map(|&(dir, prefix)| {
        let lib = PathBuf::from(dir).join("libobs.so.0");
        lib.exists().then(|| Layout {
            root: PathBuf::from(dir),
            lib,
            data: PathBuf::from(prefix).join("share/obs/libobs"),
            plugins: PathBuf::from(dir).join("obs-plugins"),
            plugin_data: PathBuf::from(prefix).join("share/obs/obs-plugins"),
        })
    })
}

/// (modul, kötelező-e)
pub fn modules() -> Vec<(&'static str, bool)> {
    let capture = if wayland() { "linux-pipewire" } else { "linux-capture" };
    vec![(capture, true), ("linux-pulseaudio", true), ("obs-ffmpeg", true), ("obs-nvenc", false), ("obs-x264", false)]
}

pub fn hardware_encoders(hevc: bool) -> &'static [&'static str] {
    if hevc {
        &["obs_nvenc_hevc_tex", "hevc_ffmpeg_vaapi_tex"]
    } else {
        &["obs_nvenc_h264_tex", "ffmpeg_vaapi_tex"]
    }
}

pub fn open(layout: &Layout) -> Result<Library, String> {
    // A mentést végző obs-ffmpeg-mux a futó program mellett keresendő (os_get_executable_path);
    // a disztribúciós OBS és a ClipCat csomag egyaránt a /usr/bin-be telepít.
    unsafe { Library::new(&layout.lib) }.map_err(|e| tf("engine.dllLoad", &[("error", &e)]))
}

/// A lemezes puffer vágója: a rendszer ffmpeg-je (a deb/rpm függősége)
pub fn ffmpeg() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/bin"), PathBuf::from("/usr/local/bin")])
        .map(|dir| dir.join("ffmpeg"))
        .find(|p| p.is_file())
}

/// Linuxon nincs kötelező zárolás; a hívó előtte már várt a lezárásra.
pub fn file_closed(_path: &std::path::Path) -> bool {
    true
}

pub fn hide_console(_cmd: &mut std::process::Command) {}

/// A libobs grafikus rétege a startup előtt tudni akarja, milyen kijelzőszerveren fut.
pub fn before_startup(lib: &Library) -> Result<(), String> {
    unsafe {
        let set_platform: libloading::Symbol<unsafe extern "C" fn(c_int)> =
            lib.get(b"obs_set_nix_platform\0").map_err(|e| e.to_string())?;
        let set_display: libloading::Symbol<unsafe extern "C" fn(Ptr)> =
            lib.get(b"obs_set_nix_platform_display\0").map_err(|e| e.to_string())?;
        let (platform, display) = if wayland() {
            (OBS_NIX_PLATFORM_WAYLAND, open_display("libwayland-client.so.0", b"wl_display_connect\0")?)
        } else {
            (OBS_NIX_PLATFORM_X11_EGL, open_display("libX11.so.6", b"XOpenDisplay\0")?)
        };
        set_platform(platform);
        set_display(display);
    }
    Ok(())
}

/// Saját kapcsolat a kijelzőszerverhez (XOpenDisplay / wl_display_connect a NULL alapértelmezéssel);
/// a folyamat végéig nyitva marad.
unsafe fn open_display(lib_name: &str, symbol: &[u8]) -> Result<Ptr, String> {
    let lib = Library::new(lib_name).map_err(|e| e.to_string())?;
    let connect: libloading::Symbol<unsafe extern "C" fn(*const c_char) -> Ptr> = lib.get(symbol).map_err(|e| e.to_string())?;
    let display = connect(std::ptr::null());
    std::mem::forget(lib);
    if display.is_null() {
        Err(format!("{lib_name}: a kijelzőszerver nem érhető el"))
    } else {
        Ok(display)
    }
}

fn token_path() -> PathBuf {
    logfile::state_dir().join(RESTORE_TOKEN_FILE)
}

pub fn display_settings<'a>(data: Data<'a>, monitor_id: &str) -> Data<'a> {
    if wayland() {
        let token = std::fs::read_to_string(token_path()).unwrap_or_default();
        data.bool("ShowCursor", true).str("RestoreToken", token.trim())
    } else {
        data.int("screen", monitor_id.parse().unwrap_or(0)).bool("show_cursor", true)
    }
}

pub fn game_settings(data: Data) -> Data {
    data
}

/// A portál által adott visszaállító token mentése a következő indításhoz.
pub fn persist_display(api: &Api, display: Ptr) {
    if !wayland() || display.is_null() {
        return;
    }
    unsafe {
        let settings = (api.obs_source_get_settings)(display);
        if settings.is_null() {
            return;
        }
        let token = (api.obs_data_get_string)(settings, cs("RestoreToken").as_ptr());
        if !token.is_null() {
            let token = CStr::from_ptr(token).to_string_lossy().into_owned();
            if !token.is_empty() {
                let _ = std::fs::create_dir_all(logfile::state_dir());
                let _ = std::fs::write(token_path(), token);
            }
        }
        (api.obs_data_release)(settings);
    }
}

extern "C" {
    #[link_name = "vsnprintf"]
    fn libc_vsnprintf(buffer: *mut c_char, size: usize, format: *const c_char, args: Ptr) -> c_int;
}

/// A libobs printf-stílusú naplóüzenetének kifejtése (a va_list mutatóként érkezik)
pub unsafe fn vsnprintf(buffer: &mut [c_char], format: *const c_char, args: Ptr) {
    libc_vsnprintf(buffer.as_mut_ptr(), buffer.len(), format, args);
}
