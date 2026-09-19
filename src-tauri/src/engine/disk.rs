//! Lemezes visszajátszási puffer: a memóriás replay buffer helyett a kódolt adatfolyam rövid
//! MKV darabokba íródik (ffmpeg_muxer + split_file), a már nem kellő darabokat a takarító törli.
//! Mentéskor az aktuális darab lezárul, és az ffmpeg újrakódolás nélkül összefűzi, majd a puffer
//! hosszára vágja a darabokat (kulcskockánál, így a klip legfeljebb egy GOP-pal hosszabb).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::{now_ms, sys};
use crate::i18n::{t, tf};
use crate::logfile;

/// Egy darab hossza; a puffer ennyivel több adatot tart meg a beállított hossznál
pub const SEGMENT_SECONDS: i64 = 10;
/// MKV: lezáráskor indexet (cues) kap, így a vágás pontosan kulcskockára ugrik (az MPEG-TS-ben
/// nincs index, ott a keresés a következő kulcskockára ugrik, és a hang a kép előtt indul)
pub const EXTENSION: &str = "mkv";
/// Csak az ezzel kezdődő fájlokat törli: a puffer mappája lehet a felhasználó bármelyik mappája
pub const PREFIX: &str = "clipcat-seg ";
const LIST_FILE: &str = "clipcat-concat.txt";
/// A split_file kérés a következő kulcskockánál teljesül (keyint 2 s)
const SPLIT_TIMEOUT: Duration = Duration::from_secs(8);
/// Az obs-ffmpeg-mux a fájlváltás jelzése után írja ki az előző darab végét (indexét) és zárja le
const CLOSE_DELAY: Duration = Duration::from_millis(500);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

struct Segment {
    path: PathBuf,
    /// A darab első képkockájának ideje, Unix ms
    start_ms: u64,
}

struct Ring {
    dir: PathBuf,
    keep_ms: u64,
    /// Időrendben; az utolsó az éppen íródó darab
    segments: VecDeque<Segment>,
    /// Mentés közben a takarító nem töröl, mert az ffmpeg épp olvassa a darabokat
    saving: bool,
}

static RING: Mutex<Option<Ring>> = Mutex::new(None);
static CHANGED: Condvar = Condvar::new();

fn ring() -> MutexGuard<'static, Option<Ring>> {
    RING.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn ffmpeg_available() -> bool {
    sys::ffmpeg().is_some()
}

/// A mappában maradt korábbi darabok törlése (pl. összeomlás után).
fn clean_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.starts_with(PREFIX) && name.ends_with(&format!(".{EXTENSION}"))) || name == LIST_FILE {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Új puffer a kimenet indítása előtt; az első darab útvonalát adja.
pub fn begin(dir: &Path, buffer_seconds: u32) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| tf("engine.bufferDir", &[("error", &e)]))?;
    clean_dir(dir);
    let now = now_ms();
    let first = dir.join(format!("{PREFIX}{now}.{EXTENSION}"));
    *ring() = Some(Ring {
        dir: dir.to_path_buf(),
        keep_ms: (buffer_seconds as u64 + SEGMENT_SECONDS as u64) * 1000,
        segments: VecDeque::from([Segment { path: first.clone(), start_ms: now }]),
        saving: false,
    });
    Ok(first)
}

/// A kimenet leállítása után: a darabok törlése.
pub fn end() {
    if let Some(ring) = ring().take() {
        clean_dir(&ring.dir);
    }
    CHANGED.notify_all();
}

/// Mentés után üríti a puffert; csak az éppen íródó darab marad.
pub fn clear() {
    if let Some(ring) = ring().as_mut() {
        while ring.segments.len() > 1 {
            if let Some(old) = ring.segments.pop_front() {
                let _ = std::fs::remove_file(old.path);
            }
        }
    }
}

/// A muxer új darabot kezdett (a `file_changed` jelzés a libobs szálán).
pub fn segment_started(path: PathBuf) {
    let mut guard = ring();
    let Some(ring) = guard.as_mut() else { return };
    ring.segments.push_back(Segment { path, start_ms: now_ms() });
    if !ring.saving {
        prune(ring);
    }
    CHANGED.notify_all();
}

/// A legrégebbi darab törölhető, ha nélküle is megvan a puffer hossza.
fn prune(ring: &mut Ring) {
    while ring.segments.len() > 2 {
        let current = ring.segments[ring.segments.len() - 1].start_ms;
        if current.saturating_sub(ring.segments[1].start_ms) < ring.keep_ms {
            break;
        }
        if let Some(old) = ring.segments.pop_front() {
            let _ = std::fs::remove_file(old.path);
        }
    }
}

/// A mentés első lépése, a darab lezárásának kérése előtt: az eddigi darabok száma.
pub fn begin_save() -> Result<usize, String> {
    let mut guard = ring();
    let ring = guard.as_mut().ok_or_else(|| t("engine.replayNotRunning"))?;
    if ring.saving {
        return Err(t("engine.saveBusy"));
    }
    ring.saving = true;
    Ok(ring.segments.len())
}

/// Megvárja a darab lezárását, majd kivágja a klipet az `output_dir` mappába.
pub fn finish_save(known: usize, buffer_seconds: u32, output_dir: &Path) -> Result<PathBuf, String> {
    let result = cut(known, buffer_seconds, output_dir);
    end_save();
    result
}

/// A mentés vége (vagy elmaradása): a takarító újra törölhet.
pub fn end_save() {
    if let Some(ring) = ring().as_mut() {
        ring.saving = false;
        prune(ring);
    }
}

fn cut(known: usize, buffer_seconds: u32, output_dir: &Path) -> Result<PathBuf, String> {
    let ffmpeg = sys::ffmpeg().ok_or_else(|| t("engine.ffmpegMissing"))?;
    {
        let guard = ring();
        let (guard, timeout) = CHANGED
            .wait_timeout_while(guard, SPLIT_TIMEOUT, |ring| ring.as_ref().is_some_and(|r| r.segments.len() <= known))
            .unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            return Err(t("engine.replayNotRunning"));
        }
        if timeout.timed_out() {
            return Err(t("engine.segmentTimeout"));
        }
    }
    let closing = ring().as_ref().and_then(|r| r.segments.get(known - 1)).map(|s| s.path.clone());
    std::thread::sleep(CLOSE_DELAY);
    if let Some(path) = closing {
        let deadline = std::time::Instant::now() + CLOSE_TIMEOUT;
        while !sys::file_closed(&path) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // A lezárt darabok a legújabbtól visszafelé, amíg ki nem adják a puffer hosszát
    let (dir, parts, offset_ms) = {
        let guard = ring();
        let ring = guard.as_ref().ok_or_else(|| t("engine.replayNotRunning"))?;
        let segments: Vec<_> = ring.segments.iter().collect();
        let want = buffer_seconds as u64 * 1000;
        let mut total = 0;
        let mut first = segments.len() - 1;
        while first > 0 && total < want {
            first -= 1;
            total += segments[first + 1].start_ms - segments[first].start_ms;
        }
        if total == 0 {
            return Err(t("engine.noSegments"));
        }
        let parts: Vec<PathBuf> = segments[first..segments.len() - 1].iter().map(|s| s.path.clone()).collect();
        (ring.dir.clone(), parts, total.saturating_sub(want))
    };

    // A felesleg mindig rövidebb az első darabnál, így elég annak a kezdőpontját eltolni; másolásnál
    // az ffmpeg az előtte lévő kulcskockától kezd (a concat bemenet a -ss-t figyelmen kívül hagyja)
    let list = dir.join(LIST_FILE);
    let mut entries = String::new();
    for (i, part) in parts.iter().enumerate() {
        entries += &format!("file '{}'\n", part.to_string_lossy().replace('\\', "/").replace('\'', r"'\''"));
        if i == 0 && offset_ms > 0 {
            entries += &format!("inpoint {}.{:03}\n", offset_ms / 1000, offset_ms % 1000);
        }
    }
    std::fs::write(&list, entries).map_err(|e| tf("engine.bufferDir", &[("error", &e)]))?;

    let _ = std::fs::create_dir_all(output_dir);
    let target = output_dir.join(format!("Replay {}.mp4", now_ms()));
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0"]);
    cmd.arg("-i")
        .arg(&list)
        .args(["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero", "-movflags", "+faststart"])
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    sys::hide_console(&mut cmd);
    let output = cmd.output();
    let _ = std::fs::remove_file(&list);
    let output = output.map_err(|e| tf("engine.ffmpegFailed", &[("detail", &e)]))?;
    if !output.status.success() || !target.exists() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        let _ = std::fs::remove_file(&target);
        logfile::write(&format!("ffmpeg hiba ({}): {stderr}", output.status));
        return Err(tf("engine.ffmpegFailed", &[("detail", &detail)]));
    }
    logfile::write(&format!("Lemezes puffer: {} darab, {} ms levágva", parts.len(), offset_ms));
    Ok(target)
}
