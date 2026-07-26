mod models;
mod scanner;

use models::{ScanResult, TreeNode};
use scanner::prune::TopNMode;
use std::sync::Mutex;
use std::time::Instant;

/// Process-wide cache of the most recent *full* scan tree. The on-the-wire
/// `scan_drive` response only carries a pruned copy (to stay inside WebView
/// memory limits); the full tree is kept here so the frontend can lazily expand
/// any node via `get_node` without re-scanning.
struct AppState(Mutex<Option<TreeNode>>);

impl Default for AppState {
    fn default() -> Self {
        AppState(Mutex::new(None))
    }
}

#[tauri::command]
async fn scan_drive(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    drive_path: String,
    method: Option<String>,
    max_depth: Option<u32>,
    top_n: Option<u32>,
    top_n_mode: Option<String>,
    merge_files: Option<bool>,
    precise: Option<bool>,
    threads: Option<u32>,
) -> Result<ScanResult, String> {
    let method = method.unwrap_or_else(|| "auto".to_string());
    let max_depth = max_depth.unwrap_or(4);
    let top_n = top_n.unwrap_or(100);
    let mode = match top_n_mode.as_deref() {
        Some("percent") => TopNMode::Percent,
        _ => TopNMode::Count,
    };
    let merge_files = merge_files.unwrap_or(false);
    let precise = precise.unwrap_or(false);

    // Run the (blocking) file walk on a dedicated thread so we don't stall the
    // async runtime. Progress is streamed back via window events.
    let window_clone = window.clone();
    let start = Instant::now();
    let scan_result = tauri::async_runtime::spawn_blocking(move || {
        scanner::scan_drive(
            Some(window_clone),
            drive_path,
            &method,
            precise,
            threads,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("scan task failed: {}", e))??;

    let (full_tree, strategy_used) = scan_result;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    // Cache the authoritative full tree for later `get_node` expansion, then
    // prune from the cached reference — avoids deep-cloning a multi-million
    // node tree (which would momentarily double memory usage).
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "scan state mutex poisoned".to_string())?;
    *guard = Some(full_tree);
    let full = guard.as_ref().expect("just stored");

    Ok(ScanResult {
        root: scanner::prune::prune(full, max_depth, top_n as usize, mode, merge_files),
        strategy_used,
        elapsed_ms,
        total_files: full.file_count as u64,
        total_folders: full.folder_count as u64,
        total_size: full.size,
    })
}

/// Find a direct child by name, tolerating case differences (Windows file names
/// are case-insensitive). Returns `None` if no match exists.
fn find_child<'a>(children: &'a [TreeNode], name: &str) -> Option<&'a TreeNode> {
    children
        .iter()
        .find(|c| c.name == name)
        .or_else(|| children.iter().find(|c| c.name.eq_ignore_ascii_case(name)))
}

/// Lazily expand a single node from the cached full tree.
///
/// `path` is the sequence of child *names* from the drive root down to the
/// wanted node; an empty vector returns the root itself. The result is pruned to
/// `max_depth` (default 2) levels below the requested node and `top_n` (default
/// 100) children per level, mirroring `scan_drive`.
///
/// Returns an error if no scan has been cached yet, or if a name in `path` does
/// not match a child (the synthetic "(其他 N 项)" aggregator is intentionally not
/// reachable this way).
#[tauri::command]
async fn get_node(
    state: tauri::State<'_, AppState>,
    path: Vec<String>,
    max_depth: Option<u32>,
    top_n: Option<u32>,
    top_n_mode: Option<String>,
    merge_files: Option<bool>,
) -> Result<TreeNode, String> {
    let max_depth = max_depth.unwrap_or(2);
    let top_n = top_n.unwrap_or(100) as usize;
    let mode = match top_n_mode.as_deref() {
        Some("percent") => TopNMode::Percent,
        _ => TopNMode::Count,
    };
    let merge_files = merge_files.unwrap_or(false);

    let guard = state
        .0
        .lock()
        .map_err(|_| "scan state mutex poisoned".to_string())?;
    let root = guard
        .as_ref()
        .ok_or_else(|| "no scan result cached; run scan_drive first".to_string())?;

    let mut current = root;
    for name in &path {
        match find_child(current.children.as_slice(), name) {
            Some(next) => current = next,
            None => {
                // Surface the available child names so the failure is debuggable
                // (e.g. when the cached tree came from an incomplete USN pass).
                let sample: Vec<&str> = current
                    .children
                    .iter()
                    .take(8)
                    .map(|c| c.name.as_str())
                    .collect();
                let hint = if sample.is_empty() {
                    "（该节点在缓存树中没有子节点）".to_string()
                } else {
                    format!("可用子节点示例: {:?}", sample)
                };
                return Err(format!("node not found: {name} {hint}"));
            }
        }
    }

    Ok(scanner::prune::prune(current, max_depth, top_n, mode, merge_files))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![scan_drive, get_node])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
