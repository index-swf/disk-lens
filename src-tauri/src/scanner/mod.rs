pub mod parallel;
pub mod prune;
pub mod tree_builder;
pub mod usn;
pub mod volinfo;

use crate::models::{ScanErrors, ScanProgress, ScannerError, TreeNode};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
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
}

impl ScanCtx {
    pub fn new(window: Option<Window>) -> Self {
        Self {
            window,
            scanned_files: AtomicU64::new(0),
            scanned_folders: AtomicU64::new(0),
            last_emit: Mutex::new(Instant::now()),
            error_count: AtomicU64::new(0),
            errors: Mutex::new(Vec::new()),
            cluster: 0,
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
pub(crate) fn volume_root(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(idx) = s.find(':') {
        let drive = &s[..=idx]; // "C:"
        format!("{}:\\", &drive[..drive.len() - 1]) // "C:\"
    } else {
        s.into_owned()
    }
}

/// Top-level scan entry point.
///
/// Strategy:
/// 1. `method == "auto"` (default): if the drive is NTFS, attempt the fast
///    USN-journal enumeration; on any USN failure fall back to the portable
///    parallel walker.
/// 2. `method == "usn"`: force the USN path and surface any error (never falls
///    back). Useful for detecting that the journal is unavailable.
/// 3. `method == "parallel"`: force the portable walker on every filesystem.
///
/// Returns the aggregated tree, the strategy that produced it (`"usn"` or
/// `"parallel"`), and the collected scan errors (access denied etc.).
pub fn scan_drive(
    window: Option<Window>,
    drive_path: String,
    method: &str,
    precise: bool,
    threads: Option<u32>,
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

    match method {
        "usn" => {
            let (tree, errors) = usn::scan_usn(&path, window)?;
            Ok((tree, "usn".to_string(), errors))
        }
        "parallel" => {
            let (tree, errors) = parallel::scan_parallel(&path, window, precise, threads)?;
            Ok((tree, "parallel".to_string(), errors))
        }
        _ => {
            // "auto"
            if usn::is_ntfs(&path) {
                if let Ok((tree, errors)) = usn::scan_usn(&path, window.clone()) {
                    return Ok((tree, "usn".to_string(), errors));
                }
                // USN unavailable / failed -> fall through to the parallel walker.
            }
            let (tree, errors) = parallel::scan_parallel(&path, window, precise, threads)?;
            Ok((tree, "parallel".to_string(), errors))
        }
    }
}
