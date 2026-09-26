"""Run unchanged f0887bc functions in an isolated harness. Expected failures prove audit findings.

No OBS/device capture: process enumeration/termination are fakes, APPDATA is a temp fixture.
Only the old disk module and two exact functions are compiled; no application startup runs.
Requires rustc and its normal linker in PATH. All test files live in a TemporaryDirectory.
"""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "f0887bc4c0fffb19e3368c8a7a2661c8eb2f9034"

def original(path):
    return subprocess.check_output(["git", "show", f"{BASELINE}:{path}"], cwd=ROOT).decode("utf-8")

def function(source, name):
    start = source.index(f"pub fn {name}(")
    body = source.index("{", start)
    depth = 1
    end = body + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]

DISK_TESTS = r'''
#[cfg(test)] mod proof {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        let p = PathBuf::from(std::env::var("TEST_ROOT").unwrap()).join(name);
        std::fs::create_dir_all(&p).unwrap(); p
    }
    #[test] fn saving_must_not_disable_retention() {
        let dir = fixture("retention"); begin(&dir, 10).unwrap(); begin_save().unwrap();
        for i in 0..12 { crate::CLOCK.store(100_000 + i * 10_000, std::sync::atomic::Ordering::SeqCst); segment_started(dir.join(format!("{i}.mkv"))); }
        let count = ring().as_ref().unwrap().segments.len(); end();
        assert!(count <= 5, "retained {count} segments during a save");
    }
    #[test] fn stopping_must_preserve_files_pinned_by_save() {
        let dir = fixture("pinned"); let path = begin(&dir, 10).unwrap();
        std::fs::write(&path, b"saved segment").unwrap(); begin_save().unwrap(); end();
        assert!(path.exists(), "stop deleted a file still owned by the save");
    }
    #[test] fn failed_deletion_must_remain_tracked() {
        let dir = fixture("retry"); let blocked = dir.join("locked.mkv"); std::fs::create_dir(&blocked).unwrap();
        let mut r = Ring { dir: dir.clone(), keep_ms: 0, saving: false,
            segments: VecDeque::from([Segment { path: blocked.clone(), start_ms: 0 }, Segment { path: dir.join("b"), start_ms: 10 }, Segment { path: dir.join("c"), start_ms: 20 }]) };
        prune(&mut r); assert!(r.segments.iter().any(|s| s.path == blocked));
    }
    #[test] fn backward_clock_must_not_panic() {
        let dir = fixture("clock");
        *ring() = Some(Ring { dir: dir.clone(), keep_ms: 10_000, saving: true,
            segments: VecDeque::from([Segment { path: dir.join("a"), start_ms: 20_000 }, Segment { path: dir.join("b"), start_ms: 10_000 }]) });
        let _ = cut(1, 10, &dir);
    }
}
'''

HARNESS = r'''
#![allow(dead_code)]
use std::{path::PathBuf, time::Duration, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
static CLOCK: AtomicU64 = AtomicU64::new(0);
fn now_ms() -> u64 { CLOCK.load(Ordering::SeqCst) }
mod sys {
    pub fn ffmpeg() -> Option<std::path::PathBuf> { Some("unused-ffmpeg".into()) }
    pub fn file_closed(_: &std::path::Path) -> bool { true }
    pub fn hide_console(_: &mut std::process::Command) {}
}
mod i18n { pub fn t(s: &str) -> String { s.into() } pub fn tf(s: &str, _: &[(&str, &dyn std::fmt::Display)]) -> String { s.into() } }
mod logfile { pub fn write(_: &str) {} }
mod disk;
static TERMINATED: AtomicUsize = AtomicUsize::new(0);
fn state_dir() -> PathBuf { PathBuf::from(std::env::var("TEST_ROOT").unwrap()).join("owned") }
fn config_dir() -> PathBuf { state_dir() }
fn find_processes(_: &str) -> Vec<u32> { if TERMINATED.load(Ordering::SeqCst) == 0 { vec![777] } else { vec![] } }
fn terminate(_: u32) { TERMINATED.fetch_add(1, Ordering::SeqCst); }
__MIGRATE__
__RESOLUTION__
#[test] fn resolution_must_reject_unbounded_dimensions() { assert_eq!(parse_resolution("4294967294x4294967294"), None); }
#[test] fn migration_must_preserve_foreign_obs_process_and_sentinel() {
    std::fs::create_dir_all(state_dir()).unwrap(); std::fs::write(state_dir().join("obs-config.txt"), "old").unwrap();
    let dir = PathBuf::from(std::env::var("APPDATA").unwrap()).join(r"obs-studio\.sentinel");
    std::fs::create_dir_all(&dir).unwrap(); let sentinel = dir.join("foreign"); std::fs::write(&sentinel, "obs").unwrap();
    migrate_legacy();
    assert!(TERMINATED.load(Ordering::SeqCst) == 0 && sentinel.exists(), "foreign process termination count={}, sentinel preserved={}", TERMINATED.load(Ordering::SeqCst), sentinel.exists());
}
'''

with tempfile.TemporaryDirectory(prefix="clipcat-baseline-") as temp:
    folder = Path(temp)
    (folder / "disk.rs").write_text(original("src-tauri/src/engine/disk.rs") + DISK_TESTS, encoding="utf-8")
    harness = HARNESS.replace("__MIGRATE__", function(original("src-tauri/src/platform/windows.rs"), "migrate_legacy"))
    harness = harness.replace("__RESOLUTION__", function(original("src-tauri/src/settings.rs"), "parse_resolution"))
    (folder / "proof.rs").write_text(harness, encoding="utf-8")
    executable = folder / ("proof.exe" if os.name == "nt" else "proof")
    args = ["rustc", "--edition=2021", "--test", str(folder / "proof.rs"), "-o", str(executable)]
    if os.name == "nt": args += ["-C", "linker=gcc"]
    subprocess.run(args, check=True)
    env = dict(os.environ, TEST_ROOT=str(folder), APPDATA=str(folder / "appdata"))
    result = subprocess.run([str(executable), "--test-threads=1"], env=env, capture_output=True, text=True, timeout=20)
    print(result.stdout)
    if result.returncode == 0 or "0 passed; 6 failed" not in result.stdout:
        raise SystemExit("Baseline proof did not reproduce all six expected failures")
    print("REPRODUCED: all 6 original-source defects; no user files/processes touched.")
