//! Bounded helper lifetime and bounded diagnostics; no pipe-fill deadlock.
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub fn run(command: &mut Command, timeout: Duration, cancel: &AtomicBool) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stderr = child.stderr.take().expect("piped stderr");
    let reader = std::thread::spawn(move || {
        let mut tail = Vec::new();
        let mut bytes = [0; 4096];
        while let Ok(n) = stderr.read(&mut bytes) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&bytes[..n]);
            if tail.len() > 8192 {
                tail.drain(..tail.len() - 8192);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });
    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break if status.success() { Ok(()) } else { Err(format!("{status}")) },
            Err(error) => break Err(error.to_string()),
            _ => {}
        }
        if cancel.load(Ordering::SeqCst) {
            break Err("canceled".into());
        }
        if Instant::now() >= deadline {
            break Err("timeout".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let detail = reader.join().unwrap_or_default();
    result.map_err(|error| format!("{error}: {}", detail.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hung_helper_is_killed_and_reaped() {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("powershell.exe");
            c.args(["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 30"]);
            c
        };
        #[cfg(target_os = "linux")]
        let mut command = {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        let start = Instant::now();
        let result = run(&mut command, Duration::from_millis(150), &AtomicBool::new(false));
        assert!(result.unwrap_err().starts_with("timeout"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
