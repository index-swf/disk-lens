pub mod parallel;
pub mod prune;
pub mod volinfo;

use crate::models::{ScanErrors, ScanProgress, ScannerError, TreeNode};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Window;
use tauri::Emitter;

/// Cap on the number of detailed error messages kept in `ScanCtx.errors`.
/// The total *count* is tracked separately (`error_count`) so the UI can report
/// the true total while the IPC payload stays bounded.
pub(crate) const ERROR_LOG_CAP: usize = 1000;

/// Shared scan state threaded through the walkers. Holds the (optional) Tauri
/// window for progress emission, plus error collection and (for the parallel
/// walker's allocation rounding) the volume cluster size.
pub(crate) struct ScanCtx {
    pub window: Option<Window>,
    pub scanned_files: AtomicU64,
    pub scanned_folders: AtomicU64,
    pub last_emit: Mutex<Instant>,
    /// Total number of errors encountered (access denied etc.).
    pub error_count: AtomicU64,
    /// Capped detail log of error messages.
    pub errors: Mutex<Vec<String>>,
    /// Bytes per cluster of the scanned volume; 0 = unknown (no rounding).
    pub cluster: u32,
    /// Cancellation flag set by the UI "stop scan" button. Walkers check it at
    /// every directory and stop descending when set (returning the partial tree
    /// aggregated so far).
    pub cancelled: Arc<AtomicBool>,
}

impl ScanCtx {
    pub fn new(window: Option<Window>, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            window,
            scanned_files: AtomicU64::new(0),
            scanned_folders: AtomicU64::new(0),
            last_emit: Mutex::new(Instant::now()),
            error_count: AtomicU64::new(0),
            errors: Mutex::new(Vec::new()),
            cluster: 0,
            cancelled,
        }
    }

    /// Record one scan error: always bumps the total count, appends the message
    /// to the sample log unless it already holds `ERROR_LOG_CAP` entries.
    pub fn record_error(&self, msg: String) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut list) = self.errors.lock() {
            if list.len() < ERROR_LOG_CAP {
                list.push(msg);
            }
        }
    }

    /// Snapshot `(count, samples)` and clear the log for a fresh scan.
    pub fn take_errors(&self) -> ScanErrors {
        let samples = self.errors.lock().unwrap_or_else(|p| p.into_inner()).clone();
        ScanErrors {
            count: self.error_count.load(Ordering::Relaxed),
            samples,
        }
    }

    /// Throttled progress emit (at most ~once per 150ms) to avoid flooding the
    /// frontend event channel.
    pub fn report(&self, path: &Path) {
        let now = Instant::now();
        let mut last = self.last_emit.lock().unwrap();
        if now.duration_since(*last).as_millis() >= 150 {
            *last = now;
            if let Some(w) = &self.window {
                let _ = w.emit(
                    "scan-progress",
                    ScanProgress {
                        scanned_files: self.scanned_files.load(Ordering::Relaxed),
                        scanned_folders: self.scanned_folders.load(Ordering::Relaxed),
                        current_dir: path.to_string_lossy().into_owned(),
                    },
                );
            }
        }
    }
}

/// Convert a drive-ish path (`C:`, `C:\`, `C:\foo`) into the volume root used for
/// Win32 volume calls, e.g. `C:\`.
#[cfg(target_os = "windows")]
pub(crate) fn volume_root(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(idx) = s.find(':') {
        let drive = &s[..=idx]; // "C:"
        format!("{}:\\", &drive[..drive.len() - 1]) // "C:\"
    } else {
        s.into_owned()
    }
}

/// Non-Windows: paths are already absolute mount points (`/`, `/home`, ...).
#[cfg(not(target_os = "windows"))]
pub(crate) fn volume_root(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Decode the octal escapes `/proc/mounts` uses for special characters in mount
/// points / devices (space = `\040`, tab = `\011`, backslash = `\134`, ...).
/// Without this a USB stick labeled "UBUNTU 24_0" comes back as the bogus path
/// `UBUNTU\04024_0`, which does not exist on disk. Any other byte is kept as-is.
#[cfg(not(target_os = "windows"))]
pub(crate) fn unescape_mount_field(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            let (d1, d2, d3) = (b[i + 1], b[i + 2], b[i + 3]);
            if (b'0'..=b'7').contains(&d1)
                && (b'0'..=b'7').contains(&d2)
                && (b'0'..=b'7').contains(&d3)
            {
                out.push((d1 - b'0') * 64 + (d2 - b'0') * 8 + (d3 - b'0'));
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Top-level scan entry point.
///
/// The USN-journal path was removed (never validated in production); every
/// strategy maps to the portable parallel walker, which works on all
/// filesystems and platforms. `method` is accepted for API compatibility only
/// and always reports `"parallel"` as the strategy. `cancelled` is the
/// cooperative stop flag the UI flips via `cancel_scan`.
///
/// Returns the aggregated tree, the strategy (`"parallel"`), and the collected
/// scan errors (access denied etc.). The tree may be partial if cancelled.
pub fn scan_drive(
    window: Option<Window>,
    drive_path: String,
    _method: &str,
    precise: bool,
    threads: Option<u32>,
    cancelled: Arc<AtomicBool>,
) -> Result<(TreeNode, String, ScanErrors), ScannerError> {
    // Normalize "C:" -> "C:\" so we scan the whole volume rather than the drive's
    // current working directory. Rust's std adds the `\\?\` prefix as needed for
    // paths exceeding MAX_PATH.
    let mut normalized = drive_path.trim().to_string();
    if let Some(idx) = normalized.find(':') {
        if normalized.len() == idx + 1 {
            normalized.push('\\');
        }
    }
    let path = Path::new(&normalized).to_path_buf();

    if !path.exists() {
        return Err(ScannerError::Msg(format!("path does not exist: {}", drive_path)));
    }

    #[cfg(target_os = "linux")]
    {
        if is_linux_pseudo_fs(&path) {
            return Err(ScannerError::Msg(format!(
                "refusing to scan pseudo filesystem: {}",
                drive_path
            )));
        }
    }

    let (tree, errors) = parallel::scan_parallel(&path, window, precise, threads, cancelled)?;
    Ok((tree, "parallel".to_string(), errors))
}

/// Linux pseudo filesystems that must never be walked (virtual files, no real
/// disk usage, e.g. `/proc/kcore` reports a nonsensical size). Component-level
/// prefix match so nested paths (`/proc/123/...`) are covered too.
#[cfg(target_os = "linux")]
fn is_linux_pseudo_fs(path: &Path) -> bool {
    ["/proc", "/sys", "/dev", "/run"]
        .iter()
        .any(|p| path.starts_with(p))
}
