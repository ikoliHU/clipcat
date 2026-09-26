//! Bounded logging: OBS callbacks never wait for disk I/O.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        mpsc::{sync_channel, SyncSender},
        OnceLock,
    },
};
const MAX_BYTES: u64 = 5 * 1024 * 1024;
static QUEUE: OnceLock<SyncSender<String>> = OnceLock::new();
pub fn state_dir() -> PathBuf {
    crate::platform::state_dir()
}

struct RotatingLog {
    path: PathBuf,
    file: File,
    bytes: u64,
    max: u64,
}
impl RotatingLog {
    fn open(path: PathBuf, max: u64) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path)?;
        let bytes = file.metadata()?.len();
        file.seek(SeekFrom::End(0))?;
        Ok(Self { path, file, bytes, max })
    }
    fn write(&mut self, line: &str) -> std::io::Result<()> {
        if self.bytes + line.len() as u64 + 1 > self.max {
            // Keep a single bounded previous log; rotation failure never grows the active file.
            self.file.flush()?;
            let mut previous = File::create(self.path.with_extension("previous.log"))?;
            let mut source = File::open(&self.path)?;
            source.seek(SeekFrom::Start(self.bytes.saturating_sub(self.max)))?;
            std::io::copy(&mut source.take(self.max), &mut previous)?;
            self.file.set_len(0)?;
            self.file.seek(SeekFrom::Start(0))?;
            self.bytes = 0;
        }
        writeln!(self.file, "{line}")?;
        self.bytes += line.len() as u64 + 1;
        Ok(())
    }
}

pub fn init(name: &str) {
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut log) = RotatingLog::open(dir.join(name), MAX_BYTES) else {
        return;
    };
    let (tx, rx) = sync_channel::<String>(256);
    if QUEUE.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        while let Ok(line) = rx.recv() {
            let _ = log.write(&format!("{} {line}", crate::platform::local_time("%H:%M:%S")));
        }
    });
}

pub fn write(line: &str) {
    if let Some(tx) = QUEUE.get() {
        // Bounded message length and queue; drop excess diagnostics rather than stall capture.
        let _ = tx.try_send(line.chars().take(4096).collect());
    }
}

/// Panic diagnostics must be flushed synchronously, but retain the same disk bound.
pub fn write_crash(line: &str) {
    static CRASH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CRASH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut log) = RotatingLog::open(dir.join("crash.log"), MAX_BYTES) {
        let _ = log.write(&line.chars().take(4096).collect::<String>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_log_messages_do_not_grow_without_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");
        // Upgrade from an old, unbounded log must not retain its entire size either.
        std::fs::write(&path, vec![b'x'; 10_000]).unwrap();
        let mut log = RotatingLog::open(path.clone(), 100).unwrap();
        for _ in 0..1000 {
            log.write("0123456789").unwrap();
        }
        assert!(path.metadata().unwrap().len() <= 100);
        assert!(path.with_extension("previous.log").metadata().unwrap().len() <= 100);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
