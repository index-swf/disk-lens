use serde::Serialize;

/// A node in the scanned disk tree. `children` is omitted from serialization when
/// empty so leaf files stay compact.
///
/// `truncated` marks nodes whose children were collapsed by the pruning step
/// (either because they sit deeper than `max_depth` or because they were
/// aggregated into the synthetic "(其他 N 项)" node). The frontend uses this flag
/// to know that it must lazily request the real children via `get_node`.
#[derive(Serialize, Clone, Debug)]
pub struct TreeNode {
    pub name: String,
    pub size: u64,
    pub allocated_size: u64,
    pub file_count: u32,
    pub folder_count: u32,
    /// Last modification time, Unix seconds (UTC). 0 when not available.
    /// Populated by the scanner from `fs::Metadata::modified()`; the frontend
    /// renders it in the "最后修改日期" column. `#[serde(default)]` keeps the
    /// field optional on the wire so older serialized payloads still parse.
    #[serde(default)]
    pub last_modified: i64,
    // NOTE: children must ALWAYS be serialized (even when empty) — the frontend
    // accesses `node.children.length` / `.map` directly. Omitting the field on
    // leaf nodes would throw at runtime (a second white-screen cause). The tree
    // sent over IPC is pruned and small, so empty arrays cost nothing.
    pub children: Vec<TreeNode>,
    #[serde(default)]
    pub truncated: bool,
}

/// Errors encountered while scanning (e.g. access-denied directories). The walker
/// keeps a full *count* and a capped list of message *samples* so the frontend
/// can show "N errors" plus a detailed log without blowing up IPC payload size.
#[derive(Serialize, Clone, Default, Debug)]
pub struct ScanErrors {
    /// Total number of errors encountered during the scan.
    pub count: u64,
    /// Up to `ERROR_LOG_CAP` (see `scanner/mod.rs`) detail messages, oldest first.
    pub samples: Vec<String>,
}

/// One logical drive present on the machine, for the drive-picker dropdown.
/// `letter` is what the scanner accepts (e.g. "C:"); `kind`/`label` are shown to
/// the user Explorer-style ("本地磁盘", "System", ...).
#[derive(Serialize, Clone, Debug)]
pub struct DriveInfo {
    /// Drive letter with colon, e.g. "C:".
    pub letter: String,
    /// Volume label; empty when the volume has none / the call failed.
    pub label: String,
    /// Human-readable drive type: 本地磁盘 / 可移动磁盘 / 光驱 / 网络驱动器 / RAM 磁盘 / 未知类型.
    pub kind: String,
}

/// Result returned by `scan_drive`. The `root` is a *pruned* copy of the full
/// tree (kept small enough to survive IPC serialization without exhausting the
/// WebView), while the authoritative full tree is cached in `AppState` for
/// on-demand expansion via `get_node`.
///
/// Field names are part of the IPC contract with the frontend and must not
/// change (serde emits snake_case as-is; the frontend reads `result.root`,
/// `result.strategy_used`, `result.elapsed_ms`, etc.).
#[derive(Serialize, Clone)]
pub struct ScanResult {
    pub root: TreeNode,
    pub strategy_used: String,
    pub elapsed_ms: u64,
    pub total_files: u64,
    pub total_folders: u64,
    pub total_size: u64,
    /// Volume free space (bytes) reported by `GetDiskFreeSpaceExW`; 0 when unavailable.
    pub free_bytes: u64,
    /// Volume total capacity (bytes); 0 when unavailable.
    pub total_bytes: u64,
    /// Filesystem name, e.g. "NTFS"; empty when unavailable.
    pub fs_type: String,
    /// Bytes per cluster for the volume (e.g. 4096 on typical NTFS); 0 when unknown.
    pub cluster_size: u32,
    /// Errors produced during the scan (access denied etc.).
    pub errors: ScanErrors,
}

/// Payload emitted on the `scan-progress` event. Field names are part of the IPC
/// contract shared with the frontend and must not change.
#[derive(Serialize, Clone)]
pub struct ScanProgress {
    pub scanned_files: u64,
    pub scanned_folders: u64,
    pub current_dir: String,
}

/// Errors produced while scanning. Mapped to `String` at the command boundary.
#[derive(Debug, thiserror::Error)]
pub enum ScannerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("windows api error: {0}")]
    Windows(#[from] windows::core::Error),

    #[error("{0}")]
    Msg(String),
}

impl From<String> for ScannerError {
    fn from(s: String) -> Self {
        ScannerError::Msg(s)
    }
}
