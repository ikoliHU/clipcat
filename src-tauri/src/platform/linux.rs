//! Linux: XDG directories, xdg-open/gio, .desktop autostart. Display, foreground window, and
//! key state are available through xcb on X11 (partially on Wayland through XWayland). Load xcb
//! at runtime so the app can start without X, with only these features unavailable.

use super::{Monitor, WindowInfo};
use libloading::Library;
use std::ffi::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};
use tauri::{PhysicalPosition, WebviewWindow};

const AUTOSTART_FILE: &str = "clipcat.desktop";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// $XDG_<name> if it is an absolute path; otherwise the default relative to HOME
fn xdg_dir(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(default))
}

pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("clipcat")
}

pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("clipcat")
}

/// The user's Videos directory (xdg-user-dir), falling back to ~/Videos
pub fn videos_dir() -> PathBuf {
    Command::new("xdg-user-dir")
        .arg("VIDEOS")
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
        // xdg-user-dir returns HOME for an unconfigured directory
        .filter(|dir| dir.is_absolute() && *dir != home())
        .unwrap_or_else(|| home().join("Videos"))
}

pub fn local_time(pattern: &str) -> String {
    let t = unsafe {
        let now = libc::time(null_mut());
        let mut t: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut t);
        t
    };
    pattern
        .replace("%Y", &format!("{:04}", t.tm_year + 1900))
        .replace("%m", &format!("{:02}", t.tm_mon + 1))
        .replace("%d", &format!("{:02}", t.tm_mday))
        .replace("%H", &format!("{:02}", t.tm_hour))
        .replace("%M", &format!("{:02}", t.tm_min))
        .replace("%S", &format!("{:02}", t.tm_sec))
}

// ---------- File manager, trash ----------

fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// Select the file in the file manager (FileManager1 D-Bus interface), or open its folder if unavailable.
pub fn reveal(path: &str) {
    let shown = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{}", file_uri(path)))
        .arg("string:")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !shown {
        if let Some(dir) = Path::new(path).parent() {
            open_path(&dir.to_string_lossy());
        }
    }
}

pub fn open_path(path: &str) {
    let _ = Command::new("xdg-open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Move to trash (recoverable deletion) through GLib
pub fn recycle(path: &str) -> bool {
    Command::new("gio")
        .args(["trash", "--"])
        .arg(path)
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Linux has no universal system sound; notifications are silent.
pub fn beep(_error: bool) {}

// ---------- Autostart ----------

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let path = xdg_dir("XDG_CONFIG_HOME", ".config").join("autostart").join(AUTOSTART_FILE);
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    // In an AppImage, current_exe is in the temporary mount point; the actual file is identified by APPIMAGE
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => PathBuf::from(appimage),
        None => std::env::current_exe().map_err(|e| e.to_string())?,
    };
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=ClipCat\nExec=\"{}\" --autostart\nIcon=clipcat\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
        exe.display()
    );
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    std::fs::write(&path, entry).map_err(|e| e.to_string())
}

/// Linux has no legacy version that ran OBS as a separate process.
pub fn migrate_legacy() {}

// ---------- Notification window ----------

/// Window settings (unfocused, hidden from the taskbar, always on top) come from the configuration.
pub fn prepare_overlay(_window: &WebviewWindow) {}

pub fn show_overlay(window: &WebviewWindow, margin: i32) {
    let monitor = window.primary_monitor().ok().flatten().or_else(|| window.current_monitor().ok().flatten());
    if let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) {
        let (pos, area) = (monitor.position(), monitor.size());
        let x = pos.x + area.width as i32 - size.width as i32 - margin;
        let _ = window.set_position(PhysicalPosition::new(x, pos.y + margin));
    }
    let _ = window.show();
}

pub fn hide_overlay(window: &WebviewWindow) {
    let _ = window.hide();
}

// ---------- X11 (xcb) ----------

type Conn = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Cookie {
    sequence: u32,
}

#[repr(C)]
struct GenericIterator {
    data: *mut c_void,
    rem: c_int,
    index: c_int,
}

#[repr(C)]
struct Setup {
    status: u8,
    pad0: u8,
    protocol_major_version: u16,
    protocol_minor_version: u16,
    length: u16,
    release_number: u32,
    resource_id_base: u32,
    resource_id_mask: u32,
    motion_buffer_size: u32,
    vendor_len: u16,
    maximum_request_length: u16,
    roots_len: u8,
    pixmap_formats_len: u8,
    image_byte_order: u8,
    bitmap_format_bit_order: u8,
    bitmap_format_scanline_unit: u8,
    bitmap_format_scanline_pad: u8,
    min_keycode: u8,
    max_keycode: u8,
}

#[repr(C)]
struct Screen {
    root: u32,
    default_colormap: u32,
    white_pixel: u32,
    black_pixel: u32,
    current_input_masks: u32,
    width_in_pixels: u16,
    height_in_pixels: u16,
}

#[repr(C)]
struct InternAtomReply {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    length: u32,
    atom: u32,
}

#[repr(C)]
struct GetPropertyReply {
    response_type: u8,
    format: u8,
    sequence: u16,
    length: u32,
    type_: u32,
    bytes_after: u32,
    value_len: u32,
    pad0: [u8; 12],
}

#[repr(C)]
struct QueryKeymapReply {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    length: u32,
    keys: [u8; 32],
}

#[repr(C)]
struct QueryPointerReply {
    response_type: u8,
    same_screen: u8,
    sequence: u16,
    length: u32,
    root: u32,
    child: u32,
    root_x: i16,
    root_y: i16,
    win_x: i16,
    win_y: i16,
    mask: u16,
    pad0: [u8; 2],
}

#[repr(C)]
struct KeyboardMappingReply {
    response_type: u8,
    keysyms_per_keycode: u8,
    sequence: u16,
    length: u32,
    pad0: [u8; 24],
}

#[repr(C)]
struct MonitorInfo {
    name: u32,
    primary: u8,
    automatic: u8,
    n_output: u16,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    width_in_millimeters: u32,
    height_in_millimeters: u32,
}

type ReplyFn<R> = unsafe extern "C" fn(Conn, Cookie, *mut *mut c_void) -> *mut R;

struct Xcb {
    conn: Conn,
    root: u32,
    min_keycode: u8,
    max_keycode: u8,
    /// The first keysym for each keycode (starting at min_keycode), used to detect the PTT key
    keysyms: Vec<u32>,
    intern_atom: unsafe extern "C" fn(Conn, u8, u16, *const c_char) -> Cookie,
    intern_atom_reply: ReplyFn<InternAtomReply>,
    get_property: unsafe extern "C" fn(Conn, u8, u32, u32, u32, u32, u32) -> Cookie,
    get_property_reply: ReplyFn<GetPropertyReply>,
    get_property_value: unsafe extern "C" fn(*const GetPropertyReply) -> *mut c_void,
    get_property_value_length: unsafe extern "C" fn(*const GetPropertyReply) -> c_int,
    query_keymap: unsafe extern "C" fn(Conn) -> Cookie,
    query_keymap_reply: ReplyFn<QueryKeymapReply>,
    query_pointer: unsafe extern "C" fn(Conn, u32) -> Cookie,
    query_pointer_reply: ReplyFn<QueryPointerReply>,
    randr: Option<Randr>,
    _libs: (Library, Option<Library>),
}

struct Randr {
    get_monitors: unsafe extern "C" fn(Conn, u32, u8) -> Cookie,
    get_monitors_reply: ReplyFn<c_void>,
    monitors_iterator: unsafe extern "C" fn(*const c_void) -> GenericIterator,
    monitor_info_next: unsafe extern "C" fn(*mut GenericIterator),
}

// Access the connection only while holding the XCB mutex
unsafe impl Send for Xcb {}

/// An xcb response freed by libc's free
struct Reply<R>(*mut R);

impl<R> Reply<R> {
    fn get(&self) -> &R {
        unsafe { &*self.0 }
    }
}

impl<R> Drop for Reply<R> {
    fn drop(&mut self) {
        unsafe { libc::free(self.0 as *mut c_void) };
    }
}

unsafe fn reply<R>(conn: Conn, f: ReplyFn<R>, cookie: Cookie) -> Option<Reply<R>> {
    let mut error: *mut c_void = null_mut();
    let r = f(conn, cookie, &mut error);
    if !error.is_null() {
        libc::free(error);
    }
    (!r.is_null()).then(|| Reply(r))
}

macro_rules! sym {
    ($lib:expr, $name:literal) => {
        *$lib.get(concat!($name, "\0").as_bytes()).ok()?
    };
}

impl Xcb {
    unsafe fn connect() -> Option<Xcb> {
        let lib = Library::new("libxcb.so.1").ok()?;
        let connect: unsafe extern "C" fn(*const c_char, *mut c_int) -> Conn = sym!(lib, "xcb_connect");
        let has_error: unsafe extern "C" fn(Conn) -> c_int = sym!(lib, "xcb_connection_has_error");
        let get_setup: unsafe extern "C" fn(Conn) -> *const Setup = sym!(lib, "xcb_get_setup");
        let roots: unsafe extern "C" fn(*const Setup) -> GenericIterator = sym!(lib, "xcb_setup_roots_iterator");
        let get_keyboard_mapping: unsafe extern "C" fn(Conn, u8, u8) -> Cookie = sym!(lib, "xcb_get_keyboard_mapping");
        let get_keyboard_mapping_reply: ReplyFn<KeyboardMappingReply> = sym!(lib, "xcb_get_keyboard_mapping_reply");
        let keyboard_mapping_keysyms: unsafe extern "C" fn(*const KeyboardMappingReply) -> *const u32 =
            sym!(lib, "xcb_get_keyboard_mapping_keysyms");
        let keyboard_mapping_keysyms_length: unsafe extern "C" fn(*const KeyboardMappingReply) -> c_int =
            sym!(lib, "xcb_get_keyboard_mapping_keysyms_length");

        std::env::var_os("DISPLAY")?;
        let mut screen_num = 0;
        let conn = connect(std::ptr::null(), &mut screen_num);
        if conn.is_null() || has_error(conn) != 0 {
            return None;
        }
        let setup = get_setup(conn);
        let mut it = roots(setup);
        let next_screen: unsafe extern "C" fn(*mut GenericIterator) = sym!(lib, "xcb_screen_next");
        for _ in 0..screen_num {
            next_screen(&mut it);
        }
        if it.data.is_null() {
            return None;
        }
        let root = (*(it.data as *const Screen)).root;
        let (min_keycode, max_keycode) = ((*setup).min_keycode, (*setup).max_keycode);

        let count = max_keycode.saturating_sub(min_keycode).saturating_add(1);
        let mut keysyms = Vec::new();
        if let Some(r) = reply(conn, get_keyboard_mapping_reply, get_keyboard_mapping(conn, min_keycode, count)) {
            let per = r.get().keysyms_per_keycode.max(1) as usize;
            let all = std::slice::from_raw_parts(keyboard_mapping_keysyms(r.0), keyboard_mapping_keysyms_length(r.0).max(0) as usize);
            keysyms = all.chunks(per).map(|c| c[0]).collect();
        }

        let randr_lib = Library::new("libxcb-randr.so.0").ok();
        let randr = randr_lib.as_ref().and_then(|l| {
            Some(Randr {
                get_monitors: sym!(l, "xcb_randr_get_monitors"),
                get_monitors_reply: sym!(l, "xcb_randr_get_monitors_reply"),
                monitors_iterator: sym!(l, "xcb_randr_get_monitors_monitors_iterator"),
                monitor_info_next: sym!(l, "xcb_randr_monitor_info_next"),
            })
        });

        Some(Xcb {
            conn,
            root,
            min_keycode,
            max_keycode,
            keysyms,
            intern_atom: sym!(lib, "xcb_intern_atom"),
            intern_atom_reply: sym!(lib, "xcb_intern_atom_reply"),
            get_property: sym!(lib, "xcb_get_property"),
            get_property_reply: sym!(lib, "xcb_get_property_reply"),
            get_property_value: sym!(lib, "xcb_get_property_value"),
            get_property_value_length: sym!(lib, "xcb_get_property_value_length"),
            query_keymap: sym!(lib, "xcb_query_keymap"),
            query_keymap_reply: sym!(lib, "xcb_query_keymap_reply"),
            query_pointer: sym!(lib, "xcb_query_pointer"),
            query_pointer_reply: sym!(lib, "xcb_query_pointer_reply"),
            randr,
            _libs: (lib, randr_lib),
        })
    }

    fn atom(&self, name: &str) -> u32 {
        unsafe {
            let cookie = (self.intern_atom)(self.conn, 0, name.len() as u16, name.as_ptr() as *const c_char);
            reply(self.conn, self.intern_atom_reply, cookie).map_or(0, |r| r.get().atom)
        }
    }

    /// Raw bytes of a window property (up to 64 KB)
    fn property(&self, window: u32, name: &str) -> Option<(u8, Vec<u8>)> {
        const ANY_PROPERTY_TYPE: u32 = 0;
        let atom = self.atom(name);
        if atom == 0 {
            return None;
        }
        unsafe {
            let cookie = (self.get_property)(self.conn, 0, window, atom, ANY_PROPERTY_TYPE, 0, 16384);
            let r = reply(self.conn, self.get_property_reply, cookie)?;
            let len = (self.get_property_value_length)(r.0).max(0) as usize;
            let value = (self.get_property_value)(r.0) as *const u8;
            (len > 0 && !value.is_null()).then(|| (r.get().format, std::slice::from_raw_parts(value, len).to_vec()))
        }
    }

    fn u32s(&self, window: u32, name: &str) -> Vec<u32> {
        match self.property(window, name) {
            Some((32, bytes)) => bytes.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => Vec::new(),
        }
    }

    fn text(&self, window: u32, name: &str) -> Option<String> {
        self.property(window, name).map(|(_, bytes)| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn monitors(&self) -> Vec<MonitorInfo> {
        let Some(randr) = &self.randr else { return Vec::new() };
        let mut list = Vec::new();
        unsafe {
            let Some(r) = reply(self.conn, randr.get_monitors_reply, (randr.get_monitors)(self.conn, self.root, 1)) else {
                return list;
            };
            let mut it = (randr.monitors_iterator)(r.0);
            while it.rem > 0 && !it.data.is_null() {
                list.push(std::ptr::read_unaligned(it.data as *const MonitorInfo));
                (randr.monitor_info_next)(&mut it);
            }
        }
        list
    }
}

/// One X server connection for the entire process; None without X (pure Wayland, console).
fn with_xcb<T>(f: impl FnOnce(&Xcb) -> T) -> Option<T> {
    static XCB: OnceLock<Option<Mutex<Xcb>>> = OnceLock::new();
    let xcb = XCB.get_or_init(|| unsafe { Xcb::connect() }.map(Mutex::new)).as_ref()?;
    let guard = xcb.lock().unwrap_or_else(|e| e.into_inner());
    Some(f(&guard))
}

/// The primary monitor; device_id is the RandR monitor index used by OBS's xshm_input_v2 source.
pub fn primary_monitor() -> Option<Monitor> {
    with_xcb(|x| {
        let monitors = x.monitors();
        let index = monitors.iter().position(|m| m.primary != 0).unwrap_or(0);
        monitors.get(index).map(|m| Monitor { device_id: index.to_string(), width: m.width as u32, height: m.height as u32 })
    })
    .flatten()
}

pub fn foreground_window() -> Option<WindowInfo> {
    with_xcb(|x| {
        let window = *x.u32s(x.root, "_NET_ACTIVE_WINDOW").first().filter(|&&w| w != 0)?;
        let title = x
            .text(window, "_NET_WM_NAME")
            .or_else(|| x.text(window, "WM_NAME"))
            .unwrap_or_default()
            .trim_end_matches('\0')
            .to_string();
        let exe = x
            .u32s(window, "_NET_WM_PID")
            .first()
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let fullscreen_atom = x.atom("_NET_WM_STATE_FULLSCREEN");
        let fullscreen = fullscreen_atom != 0 && x.u32s(window, "_NET_WM_STATE").contains(&fullscreen_atom);
        Some(WindowInfo { title, exe, fullscreen })
    })
    .flatten()
}

/// X keysym -> Windows virtual-key code (recorded as KeyboardEvent.keyCode by the UI)
fn keysym_to_vk(sym: u32) -> Option<u32> {
    Some(match sym {
        0x61..=0x7a => sym - 0x20,             // a-z
        0x41..=0x5a | 0x30..=0x39 => sym,      // A-Z, 0-9
        0xffbe..=0xffd5 => 0x70 + sym - 0xffbe, // F1-F24
        0xffb0..=0xffb9 => 0x60 + sym - 0xffb0, // numerikus 0-9
        0x20 => 0x20,
        0xff08 => 0x08,
        0xff09 => 0x09,
        0xff0d => 0x0d,
        0xff1b => 0x1b,
        0xffe5 => 0x14,
        0xffe1 | 0xffe2 => 0x10,
        0xffe3 | 0xffe4 => 0x11,
        0xffe9 | 0xffea | 0xfe03 => 0x12,
        0xff50 => 0x24,
        0xff51..=0xff54 => 0x25 + sym - 0xff51, // nyilak
        0xff55 => 0x21,
        0xff56 => 0x22,
        0xff57 => 0x23,
        0xff63 => 0x2d,
        0xffff => 0x2e,
        0x60 | 0xf6 => 0xc0, // ` and the Hungarian layout's ö key (Windows VK_OEM_3)
        0x2d => 0xbd,
        0x3d => 0xbb,
        0x5b => 0xdb,
        0x5d => 0xdd,
        0x5c => 0xdc,
        0x3b => 0xba,
        0x27 => 0xde,
        0x2c => 0xbc,
        0x2e => 0xbe,
        0x2f => 0xbf,
        _ => return None,
    })
}

/// Whether a key or middle mouse button is pressed (Windows virtual-key code).
/// Works only on X11 (and with focused XWayland windows); Wayland has no global key state.
pub fn key_down(vk: u32) -> bool {
    const VK_MBUTTON: u32 = 0x04;
    const BUTTON2_MASK: u16 = 1 << 9;
    with_xcb(|x| unsafe {
        if vk == VK_MBUTTON {
            let cookie = (x.query_pointer)(x.conn, x.root);
            return reply(x.conn, x.query_pointer_reply, cookie).is_some_and(|r| r.get().mask & BUTTON2_MASK != 0);
        }
        let Some(r) = reply(x.conn, x.query_keymap_reply, (x.query_keymap)(x.conn)) else { return false };
        let keys = r.get().keys;
        (x.min_keycode..=x.max_keycode).any(|code| {
            keys[code as usize / 8] & (1 << (code % 8)) != 0
                && x.keysyms.get((code - x.min_keycode) as usize).and_then(|&s| keysym_to_vk(s)) == Some(vk)
        })
    })
    .unwrap_or(false)
}
