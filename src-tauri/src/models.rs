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
    // NOTE: children must ALWAYS be serialized (even when empty) — the frontend
    // accesses `node.children.length` / `.map` directly. Omitting the field on
    // leaf nodes would throw at runtime (a second white-screen cause). The tree
    // sent over IPC is pruned and small, so empty arrays cost nothing.
    pub children: Vec<TreeNode>,
    #[serde(default)]
    pub truncated: bool,
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
