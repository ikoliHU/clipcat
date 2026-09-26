//! Disk replay ring: monotonic retention, per-save pins, isolated sessions, bounded FFmpeg.
use super::{now_ms, sys};
use crate::{
    i18n::{t, tf},
    logfile, resources,
};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex, MutexGuard,
};
use std::time::{Duration, Instant};

pub const SEGMENT_SECONDS: i64 = 10;
pub const EXTENSION: &str = "mkv";
pub const PREFIX: &str = "clipcat-seg ";
const SPLIT_TIMEOUT: Duration = Duration::from_secs(8);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct Segment {
    path: PathBuf,
    started: Instant,
}
struct Saving {
    pins: HashSet<PathBuf>,
    boundary: PathBuf,
    closed_at: Option<Instant>,
    cancel: Arc<AtomicBool>,
}
struct Ring {
    id: u64,
    dir: PathBuf,
    keep: Duration,
    max_bytes: u64,
    segments: VecDeque<Segment>,
    saving: Option<Saving>,
}
static RING: Mutex<Option<Ring>> = Mutex::new(None);
static CHANGED: Condvar = Condvar::new();
fn ring() -> MutexGuard<'static, Option<Ring>> {
    RING.lock().unwrap_or_else(|e| e.into_inner())
}
pub fn ffmpeg_available() -> bool {
    sys::ffmpeg().is_some()
}

fn remove(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            logfile::write(&format!("Buffer cleanup {}: {error}", path.display()));
            false
        }
    }
}

pub fn begin(dir: &Path, seconds: u32, kbps: u32) -> Result<PathBuf, String> {
    let mut guard = ring();
    if guard.is_some() {
        return Err(t("engine.saveBusy"));
    }
    let id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    // Never sweep a user-selected folder by filename prefix. Every session owns a subfolder.
    let dir = dir.join(format!("clipcat-buffer-{}-{}-{id}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&dir).map_err(|e| tf("engine.bufferDir", &[("error", &e)]))?;
    if let Err(error) = resources::require_space(&dir, resources::MIN_FREE_BYTES) {
        let _ = std::fs::remove_dir(&dir);
        return Err(error);
    }
    let first = dir.join(format!("{PREFIX}first.{EXTENSION}"));
    *guard = Some(Ring {
        id,
        dir,
        keep: Duration::from_secs(u64::from(seconds) + SEGMENT_SECONDS as u64),
        // At most one saved snapshot plus the rolling window, with container/VBR headroom.
        max_bytes: (u64::from(seconds) + 30) * (u64::from(kbps) + 192) * 1000 / 8 * 3 + 64 * 1024 * 1024,
        segments: VecDeque::from([Segment {
            path: first.clone(),
            started: Instant::now(),
        }]),
        saving: None,
    });
    Ok(first)
}

pub fn end() {
    if let Some(r) = ring().take() {
        for segment in &r.segments {
            if !r.saving.as_ref().is_some_and(|s| s.pins.contains(&segment.path)) {
                remove(&segment.path);
            }
        }
        // A save owns its pinned files until its guard drops, even if end is called early.
        if let Some(save) = &r.saving {
            save.cancel.store(true, Ordering::SeqCst);
        }
        let _ = std::fs::remove_dir(&r.dir);
    }
    CHANGED.notify_all();
}

pub fn clear() {
    if let Some(r) = ring().as_mut() {
        let last = r.segments.back().map(|s| s.path.clone());
        let pins = r.saving.as_ref().map(|s| &s.pins);
        r.segments
            .retain(|s| Some(&s.path) == last.as_ref() || pins.is_some_and(|p| p.contains(&s.path)) || !remove(&s.path));
    }
}

pub fn segment_started(path: PathBuf) {
    let mut guard = ring();
    let Some(r) = guard.as_mut() else { return };
    // Ignore callbacks from an old output/session.
    if path.parent() != Some(r.dir.as_path()) || r.segments.iter().any(|s| s.path == path) {
        return;
    }
    let now = Instant::now();
    if let Some(save) = r.saving.as_mut() {
        if save.closed_at.is_none() && r.segments.back().is_some_and(|s| s.path == save.boundary) {
            save.closed_at = Some(now);
        }
    }
    r.segments.push_back(Segment { path, started: now });
    prune(r);
    CHANGED.notify_all();
}

fn prune(r: &mut Ring) {
    let Some(current) = r.segments.back().map(|s| s.started) else {
        return;
    };
    let mut i = 0;
    while i + 2 < r.segments.len() {
        let expired = current.saturating_duration_since(r.segments[i + 1].started) >= r.keep;
        let pinned = r.saving.as_ref().is_some_and(|s| s.pins.contains(&r.segments[i].path));
        if expired && !pinned && remove(&r.segments[i].path) {
            r.segments.remove(i);
        } else {
            i += 1;
        }
    }
}

pub fn health() -> Result<(), String> {
    let mut guard = ring();
    let Some(r) = guard.as_mut() else { return Ok(()) };
    prune(r);
    let bytes: u64 = r.segments.iter().filter_map(|s| s.path.metadata().ok()).map(|m| m.len()).sum();
    if bytes > r.max_bytes {
        return Err(t("engine.bufferQuota"));
    }
    resources::require_space(&r.dir, resources::MIN_FREE_BYTES)
}

pub fn cancel_save() {
    if let Some(save) = ring().as_ref().and_then(|r| r.saving.as_ref()) {
        save.cancel.store(true, Ordering::SeqCst);
    }
    CHANGED.notify_all();
}

pub struct SaveJob {
    session: u64,
    dir: PathBuf,
    parts: Vec<Segment>,
    cancel: Arc<AtomicBool>,
}
impl Drop for SaveJob {
    fn drop(&mut self) {
        let mut guard = ring();
        if let Some(r) = guard.as_mut().filter(|r| r.id == self.session) {
            r.saving = None;
            prune(r);
        } else {
            for part in &self.parts {
                remove(&part.path);
            }
            let _ = std::fs::remove_dir(&self.dir);
        }
    }
}

pub fn begin_save() -> Result<SaveJob, String> {
    let mut guard = ring();
    let r = guard.as_mut().ok_or_else(|| t("engine.replayNotRunning"))?;
    if r.saving.is_some() {
        return Err(t("engine.saveBusy"));
    }
    let parts: Vec<_> = r.segments.iter().cloned().collect();
    let boundary = parts.last().ok_or_else(|| t("engine.noSegments"))?.path.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    r.saving = Some(Saving {
        pins: parts.iter().map(|s| s.path.clone()).collect(),
        boundary,
        closed_at: None,
        cancel: cancel.clone(),
    });
    Ok(SaveJob {
        session: r.id,
        dir: r.dir.clone(),
        parts,
        cancel,
    })
}

pub fn finish_save(job: SaveJob, seconds: u32, output_dir: &Path) -> Result<PathBuf, String> {
    let ffmpeg = sys::ffmpeg().ok_or_else(|| t("engine.ffmpegMissing"))?;
    let guard = ring();
    let (guard, _) = CHANGED
        .wait_timeout_while(guard, SPLIT_TIMEOUT, |state| {
            !job.cancel.load(Ordering::SeqCst)
                && state
                    .as_ref()
                    .is_some_and(|r| r.id == job.session && r.saving.as_ref().is_some_and(|s| s.closed_at.is_none()))
        })
        .unwrap_or_else(|e| e.into_inner());
    if job.cancel.load(Ordering::SeqCst) {
        return Err(t("engine.saveCancelled"));
    }
    let closed = guard
        .as_ref()
        .filter(|r| r.id == job.session)
        .and_then(|r| r.saving.as_ref())
        .and_then(|s| s.closed_at)
        .ok_or_else(|| t("engine.segmentTimeout"))?;
    drop(guard);
    let closing = &job.parts.last().ok_or_else(|| t("engine.noSegments"))?.path;
    std::thread::sleep(Duration::from_millis(500));
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    while !sys::file_closed(closing) {
        if job.cancel.load(Ordering::SeqCst) {
            return Err(t("engine.saveCancelled"));
        }
        if Instant::now() >= deadline {
            return Err(t("engine.segmentTimeout"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let want = Duration::from_secs(u64::from(seconds));
    let mut first = job.parts.len() - 1;
    while first > 0 && closed.saturating_duration_since(job.parts[first].started) < want {
        first -= 1;
    }
    let offset = closed
        .saturating_duration_since(job.parts[first].started)
        .saturating_sub(want)
        .as_millis();
    let parts = &job.parts[first..];
    let list = job.dir.join("concat.txt");
    let mut entries = String::new();
    for (i, part) in parts.iter().enumerate() {
        entries += &format!("file '{}'\n", part.path.to_string_lossy().replace('\\', "/").replace('\'', r"'\''"));
        if i == 0 && offset > 0 {
            entries += &format!("inpoint {}.{:03}\n", offset / 1000, offset % 1000);
        }
    }
    std::fs::write(&list, entries).map_err(|e| tf("engine.bufferDir", &[("error", &e)]))?;
    let result = (|| {
        std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
        let size: u64 = parts.iter().filter_map(|s| s.path.metadata().ok()).map(|m| m.len()).sum();
        resources::require_space(output_dir, resources::MIN_FREE_BYTES.saturating_add(size))?;
        let target = output_dir.join(format!("Replay {}-{}.mp4", now_ms(), job.session));
        let mut command = Command::new(ffmpeg);
        command
            .args(["-hide_banner", "-loglevel", "error", "-n", "-f", "concat", "-safe", "0"])
            .arg("-i")
            .arg(&list)
            .args([
                "-map",
                "0",
                "-c",
                "copy",
                "-avoid_negative_ts",
                "make_zero",
                "-movflags",
                "+faststart",
            ])
            .arg(&target);
        sys::hide_console(&mut command);
        if let Err(detail) = crate::process::run(&mut command, SAVE_TIMEOUT, &job.cancel) {
            remove(&target);
            return Err(tf("engine.ffmpegFailed", &[("detail", &detail)]));
        }
        if !target.metadata().is_ok_and(|m| m.len() > 0) {
            return Err(t("engine.noSegments"));
        }
        Ok(target)
    })();
    remove(&list);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ending_old_session_preserves_save_files_and_cannot_mutate_new_session() {
        let _serial = super::super::tests::TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = begin(dir.path(), 10, 5_000).unwrap();
        std::fs::write(&old, b"saved segment").unwrap();
        let job = begin_save().unwrap();
        end();
        assert!(old.exists(), "save owns its files until it finishes");
        assert!(job.cancel.load(Ordering::SeqCst));
        let current = begin(dir.path(), 10, 5_000).unwrap();
        std::fs::write(&current, b"new session").unwrap();
        let new_job = begin_save().unwrap();
        segment_started(old.parent().unwrap().join("stale.mkv"));
        drop(job);
        assert!(!old.exists());
        assert!(current.exists());
        assert_eq!(ring().as_ref().unwrap().segments.len(), 1);
        assert!(ring().as_ref().unwrap().saving.is_some());
        drop(new_job);
        end();
    }

    #[test]
    fn save_pins_only_its_snapshot_and_retention_continues() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut r = Ring {
            id: 1,
            dir: dir.path().into(),
            keep: Duration::from_secs(20),
            max_bytes: 1_000_000,
            segments: VecDeque::new(),
            saving: None,
        };
        for i in 0..12 {
            let path = dir.path().join(format!("{i}.mkv"));
            std::fs::write(&path, b"segment").unwrap();
            r.segments.push_back(Segment {
                path,
                started: now + Duration::from_secs(i * 10),
            });
        }
        let pinned = r.segments[0].path.clone();
        r.saving = Some(Saving {
            pins: HashSet::from([pinned.clone()]),
            boundary: pinned.clone(),
            closed_at: None,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        prune(&mut r);
        assert!(pinned.exists());
        assert!(r.segments.len() <= 4, "saving must not disable pruning");
        assert!(!dir.path().join("1.mkv").exists());
    }
    #[test]
    fn failed_deletion_stays_tracked_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let blocked = dir.path().join("locked.mkv");
        std::fs::create_dir(&blocked).unwrap();
        let mut r = Ring {
            id: 2,
            dir: dir.path().into(),
            keep: Duration::ZERO,
            max_bytes: 100,
            saving: None,
            segments: VecDeque::from([
                Segment {
                    path: blocked.clone(),
                    started: now,
                },
                Segment {
                    path: dir.path().join("b"),
                    started: now,
                },
                Segment {
                    path: dir.path().join("c"),
                    started: now,
                },
            ]),
        };
        prune(&mut r);
        assert_eq!(r.segments[0].path, blocked);
    }
}
