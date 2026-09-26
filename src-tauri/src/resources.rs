//! Conservative recording budgets. Never delete finished clips to reclaim space.
use std::path::Path;
pub const MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;

pub fn memory() -> Option<(u64, u64)> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
    }
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let value = |name: &str| {
            text.lines().find_map(|line| {
                line.strip_prefix(name)?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|kb| kb * 1024)
            })
        };
        Some((value("MemTotal:")?, value("MemAvailable:")?))
    }
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferBudget {
    pub max_mb: u64,
    pub seconds: u32,
}

pub fn memory_budget(seconds: u32, kbps: u32, memory: Option<(u64, u64)>) -> BufferBudget {
    // Allow room for the saved packet snapshot AND incoming packets, the game and OS.
    let (total, available) = memory.unwrap_or((2 * 1024 * MIB, 512 * MIB));
    let cap = (total / 8).min(available / 4).min(2 * 1024 * MIB) / MIB;
    let bytes_per_second = (u64::from(kbps) + 192) * 1000 / 8;
    let usable = cap.saturating_sub(32) * MIB * 2 / 3;
    let seconds = seconds.min((usable / bytes_per_second.max(1)).min(u32::MAX as u64) as u32);
    BufferBudget { max_mb: cap, seconds }
}

pub fn free_bytes(path: &Path) -> std::io::Result<u64> {
    // A new recording folder may not exist yet; check its nearest existing parent.
    let mut dir = path;
    while !dir.exists() {
        dir = dir.parent().ok_or_else(|| std::io::Error::other("No existing parent directory"))?;
    }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let path: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0;
        if GetDiskFreeSpaceExW(path.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(available)
    }
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes())?;
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stat) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
}

pub fn require_space(path: &Path, reserve: u64) -> Result<(), String> {
    match free_bytes(path) {
        Ok(free) if free >= reserve => Ok(()),
        Ok(_) => Err(crate::i18n::t("engine.diskFull")),
        Err(error) => Err(crate::i18n::tf("engine.diskSpaceUnknown", &[("error", &error)])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_limit_accounts_for_small_machines_and_save_peak() {
        let budget = memory_budget(1200, 150_000, Some((8 * 1024 * MIB, 2 * 1024 * MIB)));
        assert_eq!(budget.max_mb, 512);
        assert!(budget.seconds < 30);
        let low = memory_budget(150, 30_000, Some((8 * 1024 * MIB, 64 * MIB)));
        assert_eq!(low.seconds, 0);
        assert!(low.max_mb <= 16);
    }
    #[test]
    fn normal_budget_preserves_requested_time() {
        let budget = memory_budget(150, 30_000, Some((32 * 1024 * MIB, 16 * 1024 * MIB)));
        assert_eq!(budget.seconds, 150);
        assert_eq!(budget.max_mb, 2048);
    }
    #[test]
    fn full_volume_is_rejected_without_writing() {
        assert!(require_space(&std::env::temp_dir(), u64::MAX).is_err());
        assert!(free_bytes(&std::env::temp_dir()).is_ok());
    }
}
