//! Windows: a ClipCat mellé telepített motormappa (obs\, a bundle-obs.ps1 készíti az OBS portable
//! zipjéből), ennek hiányában a telepített OBS Studio. Játékrögzítés, WASAPI hang, NVENC.

use super::{Api, Data, Layout, Ptr};
use crate::logfile;
use libloading::os::windows::{Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
use std::ffi::{c_char, c_int, CStr};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use crate::i18n::tf;

/// Tartalék, ha a ClipCat saját motormappája (obs\) hiányzik.
const INSTALLED_OBS_DIR: &str = r"C:\Program Files\obs-studio";
/// Ezeket a libobs a futó exe mellett keresi, ezért az OBS-ből a ClipCat mellé másoljuk őket.
const HELPER_EXES: &[&str] = &["obs-ffmpeg-mux.exe", "obs-nvenc-test.exe"];

pub const NOT_INSTALLED: &str = "engine.notInstalled";
pub const GRAPHICS_MODULE: &CStr = c"libobs-d3d11";
pub const GAME_SOURCE: Option<&str> = Some("game_capture");
pub const DESKTOP_AUDIO_SOURCE: &str = "wasapi_output_capture";
pub const MIC_SOURCE: &str = "wasapi_input_capture";

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn locate() -> Option<Layout> {
    let bundled = exe_dir().join("obs");
    [bundled, PathBuf::from(INSTALLED_OBS_DIR)]
        .into_iter()
        .find(|root| root.join(r"bin\64bit\obs.dll").exists())
        .map(|root| Layout {
            lib: root.join(r"bin\64bit\obs.dll"),
            data: root.join(r"data\libobs"),
            plugins: root.join(r"obs-plugins\64bit"),
            plugin_data: root.join(r"data\obs-plugins"),
            root,
        })
}

/// A képernyőrögzítő forrás azonosítói, az elsőként létrehozhatóval
pub fn display_sources() -> &'static [&'static str] {
    &["monitor_capture"]
}

/// (modul, kötelező-e)
pub fn modules() -> Vec<(&'static str, bool)> {
    vec![("win-capture", true), ("win-wasapi", true), ("obs-ffmpeg", true), ("obs-nvenc", false), ("obs-x264", false)]
}

pub fn hardware_encoders(hevc: bool) -> &'static [&'static str] {
    if hevc { &["obs_nvenc_hevc_tex"] } else { &["obs_nvenc_h264_tex"] }
}

pub fn open(layout: &Layout) -> Result<libloading::Library, String> {
    let bin = layout.lib.parent().unwrap_or(&layout.root).to_path_buf();
    let app_dir = exe_dir();
    for exe in HELPER_EXES {
        sync_file(&bin.join(exe), &app_dir.join(exe));
    }

    // A pluginok függőségei (avcodec, obs.dll, libobs-d3d11 …) az OBS bin mappájában vannak;
    // a gyerekfolyamatok (muxer, NVENC-teszt) a PATH-ból találják meg őket.
    crate::platform::add_dll_directory(&bin.to_string_lossy());
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{};{path}", bin.display()));
    // A libobs az adatfájljait ehhez képest is keresi (../../data/libobs)
    let _ = std::env::set_current_dir(&bin);

    unsafe { Library::load_with_flags(&layout.lib, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }
        .map(Into::into)
        .map_err(|e| tf("engine.dllLoad", &[("error", &e)]))
}

fn sync_file(src: &Path, dst: &Path) {
    let meta = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
    if src.exists() && meta(src) != meta(dst) {
        if let Err(e) = std::fs::copy(src, dst) {
            logfile::write(&format!("{} nem másolható: {e}", dst.display()));
        }
    }
}

/// A lemezes puffer vágója: a motormappába csomagolt statikus ffmpeg.exe (bundle-obs.ps1)
pub fn ffmpeg() -> Option<PathBuf> {
    let bundled = exe_dir().join(r"obs\ffmpeg\ffmpeg.exe");
    bundled.exists().then_some(bundled)
}

/// Lezárta-e már a muxer a fájlt: amíg nyitva tartja, kizárólagosan nem nyitható meg.
pub fn file_closed(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).share_mode(0).open(path).is_ok()
}

/// Ne villanjon fel konzolablak a segédprogram futtatásakor
pub fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

pub fn before_startup(_lib: &libloading::Library) -> Result<(), String> {
    Ok(())
}

pub fn display_settings<'a>(data: Data<'a>, monitor_id: &str) -> Data<'a> {
    data.int("method", 0).bool("capture_cursor", true).str("monitor_id", monitor_id)
}

pub fn game_settings(data: Data) -> Data {
    data.str("capture_mode", "any_fullscreen")
        .bool("capture_cursor", true)
        .bool("allow_transparency", false)
        .bool("anti_cheat_hook", true)
        .bool("capture_overlays", false)
        .bool("limit_framerate", false)
        .int("hook_rate", 1)
        .bool("capture_audio", false)
}

pub fn persist_display(_api: &Api, _display: Ptr) {}

#[link(name = "ucrt")]
extern "C" {
    fn __stdio_common_vsprintf(options: u64, buffer: *mut c_char, count: usize, format: *const c_char, locale: Ptr, args: Ptr) -> c_int;
}

/// A libobs printf-stílusú naplóüzenetének kifejtése
pub unsafe fn vsnprintf(buffer: &mut [c_char], format: *const c_char, args: Ptr) {
    const STANDARD_SNPRINTF_BEHAVIOR: u64 = 2;
    __stdio_common_vsprintf(STANDARD_SNPRINTF_BEHAVIOR, buffer.as_mut_ptr(), buffer.len(), format, null_mut(), args);
}
