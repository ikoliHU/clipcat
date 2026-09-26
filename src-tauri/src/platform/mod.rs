//! Platform layer: everything that depends on the operating system (display, foreground window,
//! key state, file manager, trash, autostart, directories, notification window).
//! Each platform exposes the same functions; other modules access the system only through these.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use self::linux::*;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("ClipCat currently supports only Windows and Linux.");

pub struct Monitor {
    /// Display identifier used by the recording engine (device path on Windows, RandR monitor index on X11)
    pub device_id: String,
    pub width: u32,
    pub height: u32,
}

pub struct WindowInfo {
    pub title: String,
    /// Executable filename (including the extension on Windows)
    pub exe: String,
    pub fullscreen: bool,
}
