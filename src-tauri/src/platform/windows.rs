//! Thin Win32 layer: monitors, foreground window, processes, notification window, recycle bin,
//! Explorer, autostart (Run key), directories.

use super::{Monitor, WindowInfo};
use crate::logfile;
use std::ffi::OsStr;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use tauri::WebviewWindow;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW, DISPLAY_DEVICEW, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::{AddDllDirectory, SetDllDirectoryW};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsZoomed, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, MB_ICONASTERISK, MB_ICONEXCLAMATION, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

const DISPLAY_DEVICE_PRIMARY_DEVICE: u32 = 0x4;
const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x1;
const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;
const FO_DELETE: u32 = 0x3;
const FOF_SILENT: u16 = 0x4;
const FOF_NOCONFIRMATION: u16 = 0x10;
const FOF_ALLOWUNDO: u16 = 0x40;
const FOF_NOERRORUI: u16 = 0x400;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "ClipCat";
/// Autostart entry from before the rename
const LEGACY_RUN_VALUE: &str = "ReplayTray";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

pub fn primary_monitor() -> Option<Monitor> {
    unsafe {
        let mut index = 0;
        loop {
            let mut adapter: DISPLAY_DEVICEW = zeroed();
            adapter.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(std::ptr::null(), index, &mut adapter, 0) == 0 {
                return None;
            }
            index += 1;
            if adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE == 0 {
                continue;
            }

            let mut mode: DEVMODEW = zeroed();
            mode.dmSize = size_of::<DEVMODEW>() as u16;
            let (width, height) = if EnumDisplaySettingsW(adapter.DeviceName.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) != 0 {
                (mode.dmPelsWidth, mode.dmPelsHeight)
            } else {
                (1920, 1080)
            };

            let mut monitor: DISPLAY_DEVICEW = zeroed();
            monitor.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            let device_id = if EnumDisplayDevicesW(adapter.DeviceName.as_ptr(), 0, &mut monitor, EDD_GET_DEVICE_INTERFACE_NAME) != 0 {
                from_wide(&monitor.DeviceID)
            } else {
                String::new()
            };
            return Some(Monitor { device_id, width, height });
        }
    }
}

unsafe fn monitor_rect(monitor: HMONITOR) -> Option<RECT> {
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcMonitor)
}

/// The rectangle of the monitor fully covered by the window (fullscreen application).
/// A maximized window may cover the monitor due to its invisible frame without being fullscreen.
unsafe fn fullscreen_monitor(hwnd: HWND) -> Option<RECT> {
    if hwnd.is_null() || IsZoomed(hwnd) != 0 {
        return None;
    }
    let mut window: RECT = zeroed();
    if GetWindowRect(hwnd, &mut window) == 0 {
        return None;
    }
    let m = monitor_rect(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))?;
    (window.left <= m.left && window.top <= m.top && window.right >= m.right && window.bottom >= m.bottom).then_some(m)
}

/// Notification location: the foreground fullscreen application's (game's) monitor,
/// or the primary monitor otherwise.
fn notification_monitor_rect() -> Option<RECT> {
    unsafe {
        fullscreen_monitor(GetForegroundWindow())
            .or_else(|| monitor_rect(MonitorFromWindow(std::ptr::null_mut(), MONITOR_DEFAULTTOPRIMARY)))
    }
}

pub fn foreground_window() -> Option<WindowInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || hwnd == GetShellWindow() || hwnd == GetDesktopWindow() {
            return None;
        }
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32).max(0) as usize;

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut exe = String::new();
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let mut name = [0u16; 1024];
            let mut size = name.len() as u32;
            if QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut size) != 0 {
                let full = String::from_utf16_lossy(&name[..size as usize]);
                exe = full.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
            }
            CloseHandle(process);
        }

        Some(WindowInfo {
            title: String::from_utf16_lossy(&title[..len]),
            exe,
            fullscreen: fullscreen_monitor(hwnd).is_some(),
        })
    }
}

/// Whether a key or mouse button is pressed (virtual-key code), regardless of the active window.
pub fn key_down(vk: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

/// Some games swallow RegisterHotKey events. Detect the same combination from physical key states
/// so hotkeys work while those games have focus as well.
pub fn shortcut_down(shortcut: &Shortcut) -> bool {
    let modifier = |pressed, wanted| pressed == wanted;
    modifier(key_down(0x10), shortcut.mods.contains(Modifiers::SHIFT))
        && modifier(key_down(0x11), shortcut.mods.contains(Modifiers::CONTROL))
        && modifier(key_down(0x12), shortcut.mods.contains(Modifiers::ALT))
        && modifier(key_down(0x5b) || key_down(0x5c), shortcut.mods.contains(Modifiers::SUPER))
        && key_to_vk(shortcut.key).is_some_and(key_down)
}

/// The same Code -> Windows virtual-key mapping used by global-hotkey.
fn key_to_vk(key: Code) -> Option<u32> {
    Some(match key {
        Code::KeyA => 0x41,
        Code::KeyB => 0x42,
        Code::KeyC => 0x43,
        Code::KeyD => 0x44,
        Code::KeyE => 0x45,
        Code::KeyF => 0x46,
        Code::KeyG => 0x47,
        Code::KeyH => 0x48,
        Code::KeyI => 0x49,
        Code::KeyJ => 0x4a,
        Code::KeyK => 0x4b,
        Code::KeyL => 0x4c,
        Code::KeyM => 0x4d,
        Code::KeyN => 0x4e,
        Code::KeyO => 0x4f,
        Code::KeyP => 0x50,
        Code::KeyQ => 0x51,
        Code::KeyR => 0x52,
        Code::KeyS => 0x53,
        Code::KeyT => 0x54,
        Code::KeyU => 0x55,
        Code::KeyV => 0x56,
        Code::KeyW => 0x57,
        Code::KeyX => 0x58,
        Code::KeyY => 0x59,
        Code::KeyZ => 0x5a,
        Code::Digit0 => 0x30,
        Code::Digit1 => 0x31,
        Code::Digit2 => 0x32,
        Code::Digit3 => 0x33,
        Code::Digit4 => 0x34,
        Code::Digit5 => 0x35,
        Code::Digit6 => 0x36,
        Code::Digit7 => 0x37,
        Code::Digit8 => 0x38,
        Code::Digit9 => 0x39,
        Code::Equal => 0xbb,
        Code::Comma => 0xbc,
        Code::Minus => 0xbd,
        Code::Period => 0xbe,
        Code::Semicolon => 0xba,
        Code::Slash => 0xbf,
        Code::Backquote => 0xc0,
        Code::BracketLeft => 0xdb,
        Code::Backslash => 0xdc,
        Code::BracketRight => 0xdd,
        Code::Quote => 0xde,
        Code::Backspace => 0x08,
        Code::Tab => 0x09,
        Code::Enter | Code::NumpadEnter => 0x0d,
        Code::Pause | Code::MediaPause => 0x13,
        Code::CapsLock => 0x14,
        Code::Escape => 0x1b,
        Code::Space => 0x20,
        Code::PageUp => 0x21,
        Code::PageDown => 0x22,
        Code::End => 0x23,
        Code::Home => 0x24,
        Code::ArrowLeft => 0x25,
        Code::ArrowUp => 0x26,
        Code::ArrowRight => 0x27,
        Code::ArrowDown => 0x28,
        Code::PrintScreen => 0x2c,
        Code::Insert => 0x2d,
        Code::Delete => 0x2e,
        Code::F1 => 0x70,
        Code::F2 => 0x71,
        Code::F3 => 0x72,
        Code::F4 => 0x73,
        Code::F5 => 0x74,
        Code::F6 => 0x75,
        Code::F7 => 0x76,
        Code::F8 => 0x77,
        Code::F9 => 0x78,
        Code::F10 => 0x79,
        Code::F11 => 0x7a,
        Code::F12 => 0x7b,
        Code::F13 => 0x7c,
        Code::F14 => 0x7d,
        Code::F15 => 0x7e,
        Code::F16 => 0x7f,
        Code::F17 => 0x80,
        Code::F18 => 0x81,
        Code::F19 => 0x82,
        Code::F20 => 0x83,
        Code::F21 => 0x84,
        Code::F22 => 0x85,
        Code::F23 => 0x86,
        Code::F24 => 0x87,
        Code::NumLock => 0x90,
        Code::ScrollLock => 0x91,
        Code::Numpad0 => 0x60,
        Code::Numpad1 => 0x61,
        Code::Numpad2 => 0x62,
        Code::Numpad3 => 0x63,
        Code::Numpad4 => 0x64,
        Code::Numpad5 => 0x65,
        Code::Numpad6 => 0x66,
        Code::Numpad7 => 0x67,
        Code::Numpad8 => 0x68,
        Code::Numpad9 => 0x69,
        Code::NumpadMultiply => 0x6a,
        Code::NumpadAdd => 0x6b,
        Code::NumpadSubtract => 0x6d,
        Code::NumpadDecimal => 0x6e,
        Code::NumpadDivide => 0x6f,
        Code::NumpadEqual => 0x45,
        Code::AudioVolumeMute => 0xad,
        Code::AudioVolumeDown => 0xae,
        Code::AudioVolumeUp => 0xaf,
        Code::MediaTrackNext => 0xb0,
        Code::MediaTrackPrevious => 0xb1,
        Code::MediaStop => 0xb2,
        Code::MediaPlay | Code::MediaPlayPause => 0xb3,
        _ => return None,
    })
}

/// Local time using the given format: %Y %m %d %H %M %S.
pub fn local_time(pattern: &str) -> String {
    let t = unsafe {
        let mut t = zeroed();
        GetLocalTime(&mut t);
        t
    };
    pattern
        .replace("%Y", &format!("{:04}", t.wYear))
        .replace("%m", &format!("{:02}", t.wMonth))
        .replace("%d", &format!("{:02}", t.wDay))
        .replace("%H", &format!("{:02}", t.wHour))
        .replace("%M", &format!("{:02}", t.wMinute))
        .replace("%S", &format!("{:02}", t.wSecond))
}

/// Allow libobs plugins and their dependencies to be loaded from here as well.
pub fn add_dll_directory(path: &str) {
    let path = wide(path);
    unsafe {
        AddDllDirectory(path.as_ptr());
        SetDllDirectoryW(path.as_ptr());
    }
}

/// The notification window must never take focus from the game or appear in the alt-tab list.
pub fn prepare_overlay(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        make_overlay(hwnd.0 as _);
    }
}

/// Show the notification window in the notification monitor's top-right corner without changing focus.
pub fn show_overlay(window: &WebviewWindow, margin: i32) {
    if let (Some(area), Ok(size), Ok(hwnd)) = (notification_monitor_rect(), window.outer_size(), window.hwnd()) {
        let x = area.right - size.width as i32 - margin;
        let y = area.top + margin;
        show_no_activate(hwnd.0 as _, x, y);
    }
}

pub fn hide_overlay(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        hide(hwnd.0 as _);
    }
}

fn make_overlay(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST) as isize,
        );
    }
}

fn show_no_activate(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

/// Tauri is unaware of windows shown through Win32, so hide them through Win32 as well.
/// A visible, even transparent, always-on-top window over the game would increase presentation latency.
fn hide(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

fn recycle_source(path: &str) -> Vec<u16> {
    // canonicalize() adds a verbatim prefix that SHFileOperationW rejects.
    // Keep UNC paths absolute when converting them back to Shell syntax.
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    };
    let mut from: Vec<u16> = OsStr::new(&path).encode_wide().collect();
    from.extend([0, 0]);
    from
}

/// Move to the recycle bin (recoverable deletion).
pub fn recycle(path: &str) -> bool {
    let from = recycle_source(path);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = zeroed();
        op.wFunc = FO_DELETE as _;
        op.pFrom = from.as_ptr();
        op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as _;
        let result = SHFileOperationW(&mut op);
        if result != 0 || op.fAnyOperationsAborted != 0 {
            logfile::write(&format!("Recycle failed: code={result:#x}, aborted={}", op.fAnyOperationsAborted));
            return false;
        }
        true
    }
}

pub fn beep(error: bool) {
    unsafe {
        MessageBeep(if error { MB_ICONEXCLAMATION } else { MB_ICONASTERISK });
    }
}

pub fn reveal(path: &str) {
    let _ = Command::new("explorer").raw_arg(format!("/select,\"{path}\"")).spawn();
}

pub fn open_path(path: &str) {
    let _ = Command::new("explorer").arg(path).spawn();
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    let _ = run.delete_value(LEGACY_RUN_VALUE);
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        run.set_value(RUN_VALUE, &format!("\"{}\" --autostart", exe.display()))
            .map_err(|e| e.to_string())
    } else {
        let _ = run.delete_value(RUN_VALUE);
        Ok(())
    }
}

/// The user's Videos folder (even if moved, e.g. to OneDrive)
pub fn videos_dir() -> PathBuf {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
        .and_then(|key| key.get_value::<String, _>("My Video"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into())).join("Videos"))
}

/// Settings location: the exe folder (per-user installation, writable; removed by the uninstaller)
pub fn config_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Logs and other runtime state: %LOCALAPPDATA%\ClipCat
pub fn state_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("ClipCat")
}

/// Only remove ClipCat-owned legacy metadata. A process name never proves ownership.
pub fn migrate_legacy() {
    cleanup_legacy(&state_dir(), &config_dir());
}

fn cleanup_legacy(state_dir: &std::path::Path, config_dir: &std::path::Path) {
    let marker = state_dir.join("obs-config.txt");
    if !marker.exists() {
        return;
    }
    for file in ["obs-config.txt", "state.json", "saved.json", "state.json.tmp", "saved.json.tmp"] {
        let _ = std::fs::remove_file(state_dir.join(file));
    }
    let _ = std::fs::remove_file(config_dir.join("shadowplay.lua"));
    logfile::write("Removed remnants of the legacy separate-OBS-process setup");
}

#[cfg(test)]
mod recycle_tests {
    #[test]
    fn shell_sources_preserve_drive_and_unc_paths_and_have_two_terminators() {
        for (input, expected) in [
            (r"\\?\C:\Clips\clip with spaces.mp4", r"C:\Clips\clip with spaces.mp4"),
            (r"\\?\UNC\server\share\clip.mp4", r"\\server\share\clip.mp4"),
            (r"C:\Clips\clip.mp4", r"C:\Clips\clip.mp4"),
            (r"\\server\share\clip.mp4", r"\\server\share\clip.mp4"),
        ] {
            let source = super::recycle_source(input);
            assert_eq!(String::from_utf16(&source[..source.len() - 2]).unwrap(), expected);
            assert_eq!(&source[source.len() - 2..], &[0, 0]);
        }
    }

    #[test]
    fn recycles_a_validated_canonical_clip_path() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip with spaces.mp4");
        std::fs::write(&clip, b"recycle regression fixture").unwrap();
        let checked = crate::clips::checked_path(dir.path(), &clip).unwrap();
        assert!(super::recycle(&checked.to_string_lossy()));
        assert!(!clip.exists());
    }
}

#[cfg(test)]
mod migration_tests {
    #[test]
    fn migration_only_removes_owned_metadata_and_preserves_foreign_obs_files() {
        let dir = tempfile::tempdir().unwrap();
        let owned = dir.path().join("ClipCat");
        let foreign = dir.path().join("obs-studio/.sentinel");
        std::fs::create_dir_all(&owned).unwrap();
        std::fs::create_dir_all(&foreign).unwrap();
        for path in [
            owned.join("obs-config.txt"),
            owned.join("shadowplay.lua"),
            owned.join("keep.mp4"),
            foreign.join("foreign-obs"),
        ] {
            std::fs::write(path, b"fixture").unwrap();
        }
        super::cleanup_legacy(&owned, &owned);
        assert!(!owned.join("obs-config.txt").exists());
        assert!(!owned.join("shadowplay.lua").exists());
        assert!(owned.join("keep.mp4").exists());
        assert!(foreign.join("foreign-obs").exists());
    }
}
