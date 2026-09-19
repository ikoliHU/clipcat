//! Vékony Win32 réteg: monitorok, előtérben lévő ablak, folyamatok, értesítés-ablak, lomtár,
//! Explorer, automatikus indítás (Run kulcs), mappák.

use super::{Monitor, WindowInfo};
use crate::logfile;
use std::ffi::OsStr;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW,
    DISPLAY_DEVICEW, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::{AddDllDirectory, SetDllDirectoryW};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsZoomed, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, HWND_TOPMOST, MB_ICONASTERISK, MB_ICONEXCLAMATION, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
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
/// Az átnevezés előtti autostart-bejegyzés
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
            let (width, height) =
                if EnumDisplaySettingsW(adapter.DeviceName.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) != 0 {
                    (mode.dmPelsWidth, mode.dmPelsHeight)
                } else {
                    (1920, 1080)
                };

            let mut monitor: DISPLAY_DEVICEW = zeroed();
            monitor.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            let device_id = if EnumDisplayDevicesW(
                adapter.DeviceName.as_ptr(),
                0,
                &mut monitor,
                EDD_GET_DEVICE_INTERFACE_NAME,
            ) != 0
            {
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

/// Annak a monitornak a téglalapja, amelyet az ablak teljesen lefed (teljes képernyős alkalmazás).
/// A maximalizált ablak a láthatatlan kerete miatt is lefedheti a monitort, de az nem teljes képernyő.
unsafe fn fullscreen_monitor(hwnd: HWND) -> Option<RECT> {
    if hwnd.is_null() || IsZoomed(hwnd) != 0 {
        return None;
    }
    let mut window: RECT = zeroed();
    if GetWindowRect(hwnd, &mut window) == 0 {
        return None;
    }
    let m = monitor_rect(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))?;
    (window.left <= m.left && window.top <= m.top && window.right >= m.right && window.bottom >= m.bottom)
        .then_some(m)
}

/// Az értesítés helye: ha teljes képernyős alkalmazás (játék) van előtérben, annak a monitora,
/// egyébként a fő monitor.
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

/// Le van-e nyomva a billentyű vagy egérgomb (virtuális kód), bármelyik ablak is aktív.
pub fn key_down(vk: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

/// Helyi idő a megadott mintával: %Y %m %d %H %M %S.
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

/// A libobs pluginjai és azok függőségei innen is betölthetők legyenek.
pub fn add_dll_directory(path: &str) {
    let path = wide(path);
    unsafe {
        AddDllDirectory(path.as_ptr());
        SetDllDirectoryW(path.as_ptr());
    }
}

/// Az értesítés-ablak soha ne vegye el a fókuszt a játéktól, és ne jelenjen meg az alt-tab listában.
pub fn prepare_overlay(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        make_overlay(hwnd.0 as _);
    }
}

/// Az értesítés-ablakot fókuszváltás nélkül jeleníti meg az értesítési monitor jobb felső sarkában.
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

/// A Tauri nem tud a Win32-vel megjelenített ablakról, ezért az elrejtés is Win32-vel történik.
/// Egy látható, bár átlátszó, mindig felül lévő ablak a játék fölött rontaná a megjelenítés késleltetését.
fn hide(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

fn find_processes(exe_name: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if from_wide(&entry.szExeFile).eq_ignore_ascii_case(exe_name) {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    pids
}

fn terminate(pid: u32) {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

/// Lomtárba helyezés (visszaállítható törlés).
pub fn recycle(path: &str) -> bool {
    let mut from: Vec<u16> = OsStr::new(path).encode_wide().collect();
    from.extend([0, 0]);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = zeroed();
        op.wFunc = FO_DELETE as _;
        op.pFrom = from.as_ptr();
        op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as _;
        SHFileOperationW(&mut op) == 0 && op.fAnyOperationsAborted == 0
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

/// A felhasználó Videók mappája (akkor is, ha áthelyezte, pl. OneDrive-ra)
pub fn videos_dir() -> PathBuf {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
        .and_then(|key| key.get_value::<String, _>("My Video"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into())).join("Videos"))
}

/// A beállítások helye: az exe mappája (felhasználói telepítés, írható; az eltávolító törli)
pub fn config_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Napló és egyéb futási állapot: %LOCALAPPDATA%\ClipCat
pub fn state_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("ClipCat")
}

/// Az előző verzió külön OBS-folyamatot futtatott; azt leállítjuk és a maradványait töröljük.
pub fn migrate_legacy() {
    let state_dir = state_dir();
    let marker = state_dir.join("obs-config.txt");
    if !marker.exists() {
        return;
    }
    for pid in find_processes("obs64.exe") {
        terminate(pid);
    }
    for _ in 0..100 {
        if find_processes("obs64.exe").is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for file in ["obs-config.txt", "state.json", "saved.json", "state.json.tmp", "saved.json.tmp"] {
        let _ = std::fs::remove_file(state_dir.join(file));
    }
    let _ = std::fs::remove_file(config_dir().join("shadowplay.lua"));
    if let Ok(appdata) = std::env::var("APPDATA") {
        // A lelőtt OBS jelzője miatt egy kézi OBS-indításkor csökkentett módot kínálna
        if let Ok(entries) = std::fs::read_dir(PathBuf::from(appdata).join(r"obs-studio\.sentinel")) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    logfile::write("A korábbi, külön OBS-folyamatos működés maradványai eltávolítva");
}
