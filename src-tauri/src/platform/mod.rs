//! Platformréteg: minden, ami az operációs rendszertől függ (kijelző, előtérben lévő ablak,
//! billentyűállapot, fájlkezelő, lomtár, automatikus indítás, mappák, értesítés-ablak).
//! Minden platform ugyanazokat a függvényeket adja; a többi modul csak ezeken keresztül éri el a rendszert.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use self::linux::*;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("A ClipCat egyelőre csak Windowson és Linuxon fut.");

pub struct Monitor {
    /// A rögzítőmotor ezzel azonosítja a kijelzőt (Windowson eszközútvonal, X11-en a RandR-monitor sorszáma)
    pub device_id: String,
    pub width: u32,
    pub height: u32,
}

pub struct WindowInfo {
    pub title: String,
    /// A futtatható fájl neve (Windowson kiterjesztéssel)
    pub exe: String,
    pub fullscreen: bool,
}
