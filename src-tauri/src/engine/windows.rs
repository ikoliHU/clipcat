//! Windows: use the engine folder installed alongside ClipCat (obs\, created by bundle-obs.ps1 from
//! the OBS portable zip), or fall back to installed OBS Studio. Game capture, WASAPI audio, NVENC.

use super::{Api, Data, Layout, Ptr};
use crate::logfile;
use libloading::os::windows::{Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
use std::ffi::{c_char, c_int, CStr};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use crate::i18n::tf;

/// Fallback when ClipCat's own engine folder (obs\) is missing.
const INSTALLED_OBS_DIR: &str = r"C:\Program Files\obs-studio";
/// libobs looks for these next to the running exe, so copy them from OBS to the ClipCat folder.
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

/// Screen capture source IDs, using the first one that can be created
pub fn display_sources() -> &'static [&'static str] {
    &["monitor_capture"]
}

/// (module, required)
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

    // Plugin dependencies (avcodec, obs.dll, libobs-d3d11 …) are in the OBS bin directory;
    // child processes (muxer, NVENC test) find them through PATH.
    crate::platform::add_dll_directory(&bin.to_string_lossy());
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{};{path}", bin.display()));
    // libobs also looks for its data files relative to this path (../../data/libobs)
    let _ = std::env::set_current_dir(&bin);

    unsafe { Library::load_with_flags(&layout.lib, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }
        .map(Into::into)
        .map_err(|e| tf("engine.dllLoad", &[("error", &e)]))
}

fn sync_file(src: &Path, dst: &Path) {
    let meta = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
    if src.exists() && meta(src) != meta(dst) {
        if let Err(e) = std::fs::copy(src, dst) {
            logfile::write(&format!("Cannot copy {}: {e}", dst.display()));
        }
    }
}

/// Disk buffer trimmer: the static ffmpeg.exe bundled in the engine folder (bundle-obs.ps1)
pub fn ffmpeg() -> Option<PathBuf> {
    let bundled = exe_dir().join(r"obs\ffmpeg\ffmpeg.exe");
    bundled.exists().then_some(bundled)
}

/// Whether the muxer has closed the file: exclusive access fails while it is still open.
pub fn file_closed(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).share_mode(0).open(path).is_ok()
}

/// Prevent a console window from flashing when running a helper program
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
    data.str("capture_mode", "window")
        // WINDOW_PRIORITY_EXE: never fall back to an unrelated process with a matching title/class.
        .int("priority", 2)
        .str("window", "")
        .bool("capture_cursor", true)
        .bool("allow_transparency", false)
        .bool("anti_cheat_hook", true)
        .bool("capture_overlays", false)
        .bool("limit_framerate", false)
        .int("hook_rate", 1)
        .bool("capture_audio", false)
}

pub fn game_window() -> Option<String> {
    game_window_for(crate::platform::foreground_window().as_ref())
}

fn game_window_for(window: Option<&crate::platform::WindowInfo>) -> Option<String> {
    let window = window.filter(|w| crate::games::is_game(w) && !w.class.is_empty())?;
    // libobs ms_build_window_strings expects title:class:exe with these escapes.
    let encode = |s: &str| s.replace('#', "#22").replace(':', "#3A");
    Some(format!("{}:{}:{}", encode(&window.title), encode(&window.class), encode(&window.exe)))
}

pub fn persist_display(_api: &Api, _display: Ptr) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::WindowInfo;

    fn window(exe: &str, title: &str, fullscreen: bool) -> WindowInfo {
        WindowInfo { title: title.into(), class: "GameWindow".into(), exe: exe.into(), fullscreen }
    }

    #[test]
    fn foreground_game_selection_excludes_fullscreen_players_and_browsers() {
        for exe in ["vlc.exe", "MPC-HC64.exe", "mpv.exe", "chrome.exe", "msedge.exe", "clipcat.exe"] {
            assert_eq!(game_window_for(Some(&window(exe, "League of Legends", true))), None, "{exe}");
        }
        assert_eq!(game_window_for(None), None);
        assert_eq!(game_window_for(Some(&window("", "Unknown", true))), None);
        assert_eq!(game_window_for(Some(&window("unknown.exe", "Unknown", false))), None);
        let mut no_class = window("League of Legends.exe", "League of Legends", false);
        no_class.class.clear();
        assert_eq!(game_window_for(Some(&no_class)), None);
    }

    #[test]
    fn known_windowed_games_and_unknown_fullscreen_games_have_explicit_targets() {
        for (exe, title, fullscreen) in [
            ("League of Legends.exe", "League of Legends", false),
            ("javaw.exe", "Minecraft 1.21", false),
            ("other-game.exe", "Other Game", true),
        ] {
            let w = window(exe, title, fullscreen);
            assert_ne!(crate::games::folder_for(Some(window(exe, title, fullscreen))), crate::games::DESKTOP_FOLDER);
            assert_eq!(game_window_for(Some(&w)), Some(format!("{title}:GameWindow:{exe}")));
        }
    }

    #[test]
    fn obs_window_identifier_escapes_delimiters_and_literal_escape_sequences() {
        let mut w = window("game.exe", "Game: #3A", true);
        w.class = "Class:#22".into();
        assert_eq!(game_window_for(Some(&w)).as_deref(), Some("Game#3A #223A:Class#3A#2222:game.exe"));
    }
}

#[link(name = "ucrt")]
extern "C" {
    fn __stdio_common_vsprintf(options: u64, buffer: *mut c_char, count: usize, format: *const c_char, locale: Ptr, args: Ptr) -> c_int;
}

/// Expand a libobs printf-style log message
pub unsafe fn vsnprintf(buffer: &mut [c_char], format: *const c_char, args: Ptr) {
    const STANDARD_SNPRINTF_BEHAVIOR: u64 = 2;
    __stdio_common_vsprintf(STANDARD_SNPRINTF_BEHAVIOR, buffer.as_mut_ptr(), buffer.len(), format, null_mut(), args);
}
