mod models;
mod scanner;

#[cfg(test)]
mod api_tests;

use models::{DriveInfo, ScanResult, TreeNode};
use scanner::prune::TopNMode;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// Process-wide cache of the most recent *full* scan tree. The on-the-wire
/// `scan_drive` response only carries a pruned copy (to stay inside WebView
/// memory limits); the full tree is kept here so the frontend can lazily expand
/// any node via `get_node` without re-scanning.
#[cfg(not(test))]
struct AppState(Mutex<Option<TreeNode>>);

#[cfg(not(test))]
impl Default for AppState {
    fn default() -> Self {
        AppState(Mutex::new(None))
    }
}

#[cfg(not(test))]
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

    // Volume-level info (free/total space, filesystem, cluster size) for the
    // bottom status bar. Cheap Win32 calls against the volume root; computed
    // before `drive_path` is moved into the scan closure.
    let vol = scanner::volinfo::volume_info(Path::new(&scanner::volume_root(Path::new(
        &drive_path,
    ))));

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

    let (full_tree, strategy_used, errors) = scan_result;
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
        free_bytes: vol.as_ref().map(|v| v.free_bytes).unwrap_or(0),
        total_bytes: vol.as_ref().map(|v| v.total_bytes).unwrap_or(0),
        fs_type: vol.as_ref().map(|v| v.fs_type.clone()).unwrap_or_default(),
        cluster_size: vol.as_ref().map(|v| v.cluster_size).unwrap_or(0),
        errors,
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
#[cfg(not(test))]
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

    // 容错：前端在 Treemap 里点到"当前层的大矩形"会触发"自我下钻"，
    // 把当前节点名当成路径首段发过来（例如根层会发 ["C:"]）。这里把等于根名
    // 的首段去掉，避免无意义的 "node not found"。
    let walk: &[String] = if path.first().map_or(false, |f| f == &root.name) {
        &path[1..]
    } else {
        &path
    };

    if cfg!(debug_assertions) {
        eprintln!(
            "[get_node] path={:?} (walk={:?}) max_depth={} top_n={} mode={:?}",
            path, walk, max_depth, top_n, mode
        );
    }

    let mut current = root;
    let mut depth = 0usize;
    for name in walk {
        depth += 1;
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
                if cfg!(debug_assertions) {
                    eprintln!(
                        "[get_node] NOT FOUND segment={:?} at depth {} (full path={:?}); parent node={:?}; {}",
                        name, depth, path, current.name, hint
                    );
                }
                return Err(format!(
                    "node not found: {name} (完整路径={path:?}; 父节点={:?}; {hint})",
                    current.name
                ));
            }
        }
    }

    Ok(scanner::prune::prune(current, max_depth, top_n, mode, merge_files))
}

/// Enumerate the drives / mount points that actually exist on this machine.
///
/// Windows: drive letters (`GetLogicalDriveStringsW`), each enriched with its
/// Explorer-style drive type (`GetDriveTypeW`) and volume label
/// (`GetVolumeInformationW`). Only present drives are returned — no hardcoded
/// C/D/E/F/G.
///
/// Linux: device-backed mount points parsed from `/proc/mounts` (pseudo
/// filesystems like proc/sysfs/tmpfs are skipped), each tagged with its
/// filesystem type. `letter` carries the mount point path (e.g. "/", "/home").
///
/// Extracted as a plain function so the unit tests can verify against the real
/// machine.
pub fn enumerate_drives() -> Vec<DriveInfo> {
    #[cfg(target_os = "windows")]
    {
        enumerate_drives_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        enumerate_drives_unix()
    }
}

#[cfg(target_os = "windows")]
fn enumerate_drives_windows() -> Vec<DriveInfo> {
    use windows::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };
    use windows::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };

    fn kind_label(t: u32) -> String {
        match t {
            x if x == DRIVE_FIXED => "本地磁盘".into(),
            x if x == DRIVE_REMOVABLE => "可移动磁盘".into(),
            x if x == DRIVE_CDROM => "光驱".into(),
            x if x == DRIVE_REMOTE => "网络驱动器".into(),
            x if x == DRIVE_RAMDISK => "RAM 磁盘".into(),
            _ => "未知类型".into(),
        }
    }

    let mut buf = [0u16; 512];
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    let mut drives = Vec::new();
    if len > 0 {
        for part in String::from_utf16_lossy(&buf[..len]).split('\0') {
            let root = part.trim(); // "C:\"
            if root.is_empty() {
                continue;
            }
            let letter = root.trim_end_matches('\\').to_string(); // "C:"
            let wide = scanner::parallel::to_wide(root);
            let root_ptr = windows::core::PCWSTR::from_raw(wide.as_ptr());
            let kind = unsafe { kind_label(GetDriveTypeW(root_ptr)) };
            let mut label_buf = [0u16; 64];
            let label = unsafe {
                if GetVolumeInformationW(root_ptr, Some(&mut label_buf), None, None, None, None)
                    .is_ok()
                {
                    String::from_utf16_lossy(&label_buf)
                        .trim_end_matches('\0')
                        .to_string()
                } else {
                    String::new()
                }
            };
            drives.push(DriveInfo { letter, label, kind });
        }
    }
    drives
}

#[cfg(not(target_os = "windows"))]
fn enumerate_drives_unix() -> Vec<DriveInfo> {
    use std::collections::HashSet;

    let mut seen: HashSet<String> = HashSet::new();
    let mut drives = Vec::new();
    if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let mut it = line.split_whitespace();
            let dev = it.next().unwrap_or("");
            let mp = it.next().unwrap_or("");
            let fstype = it.next().unwrap_or("");
            // Keep only device-backed mounts (dev is an absolute path like
            // /dev/sda2). Pseudo filesystems (proc, sysfs, tmpfs, overlay,
            // squashfs, ...) report a name instead and are skipped.
            if dev.is_empty() || !dev.starts_with('/') || !mp.starts_with('/') {
                continue;
            }
            if !seen.insert(mp.to_string()) {
                continue;
            }
            drives.push(DriveInfo {
                letter: mp.to_string(),
                label: String::new(),
                kind: if fstype.is_empty() {
                    "本地磁盘".to_string()
                } else {
                    fstype.to_string()
                },
            });
        }
    }
    // Fallback so the dropdown is never empty even if /proc/mounts is odd.
    if drives.is_empty() {
        drives.push(DriveInfo {
            letter: "/".to_string(),
            label: String::new(),
            kind: "本地磁盘".to_string(),
        });
    }
    drives
}

#[cfg(not(test))]
#[tauri::command]
fn list_drives() -> Vec<DriveInfo> {
    enumerate_drives()
}

/// Open the native Windows folder-picker (IFileDialog via `rfd`) and return the
/// chosen folder path, or `None` if the user cancelled. The Tauri window is
/// passed as the dialog's parent so the dialog always appears on top of the app
/// (an unparented dialog can open *behind* the window and look like "nothing
/// happened").
#[cfg(not(test))]
#[tauri::command]
#[allow(unused_variables)] // `window` 仅 Windows 分支使用（Linux 无父窗口语义）
fn pick_folder(window: tauri::Window) -> Option<String> {
    // `mut` 仅在 Windows 分支需要（Linux 分支无 set_parent 赋值）。
    #[allow(unused_mut)]
    let mut dialog = rfd::FileDialog::new().set_title("选择要扫描的文件夹");
    #[cfg(target_os = "windows")]
    {
        dialog = dialog.set_parent(&window);
    }
    dialog
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            scan_drive,
            get_node,
            list_drives,
            pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
