//! Egyszerű naplófájl a platform állapotmappájában; indításkor újrakezdődik.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn state_dir() -> PathBuf {
    crate::platform::state_dir()
}

/// `name`: a naplófájl neve; az önteszt külön fájlba ír, hogy ne írja felül a futó példányét.
pub fn init(name: &str) {
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    *FILE.lock().unwrap() = File::create(dir.join(name)).ok();
}

pub fn write(line: &str) {
    if let Some(file) = FILE.lock().unwrap().as_mut() {
        let _ = writeln!(file, "{} {line}", crate::platform::local_time("%H:%M:%S"));
    }
}
