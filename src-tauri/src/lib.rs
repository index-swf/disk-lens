mod models;
mod scanner;

#[cfg(test)]
mod api_tests;

use models::{DriveInfo, ScanResult, TreeNode};
use scanner::prune::TopNMode;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Process-wide cache of the most recent *full* scan tree. The on-the-wire
/// `scan_drive` response only carries a pruned copy (to stay inside WebView
/// memory limits); the full tree is kept here so the frontend can lazily expand
/// any node via `get_node` without re-scanning.
#[cfg(not(test))]
struct AppState {
    tree: Mutex<Option<TreeNode>>,
    /// Cancellation flag of the currently running scan (set by `cancel_scan`).
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

#[cfg(not(test))]
impl Default for AppState {
    fn default() -> Self {
        AppState {
            tree: Mutex::new(None),
            cancel: Mutex::new(None),
        }
    }
}

// ---------------------------------------------------------------------------
// 数据导出（export_scan_data）：把最近一次扫描的全量树导出为带绝对路径的
// JSON 文件，供外部工具 / AI agent 直接读取（免去重新扫描）。
// ---------------------------------------------------------------------------

/// 导出用的树节点：在 `TreeNode` 基础上补全**绝对路径** `path` 与 `is_dir`。
/// agent 拿到后可直接按 path 定位要清理的目录/文件。字段刻意精简
/// （无 allocated_size），并用紧凑 JSON 序列化，尽量节省 agent 的 token。
#[derive(Serialize, Clone)]
struct ExportNode {
    path: String,
    name: String,
    size: u64,
    /// 占父目录大小的百分比（0-100，保留两位小数）；根节点为 100。
    pct_of_parent: f64,
    file_count: u32,
    folder_count: u32,
    last_modified: i64,
    is_dir: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<ExportNode>,
}

#[derive(Serialize)]
struct ExportSummary {
    /// 导出树中的节点总数（含目录与文件）。
    node_count: u64,
    dir_count: u64,
    file_count: u64,
    total_size: u64,
}

#[derive(Serialize)]
struct ExportPayload {
    app: &'static str,
    version: &'static str,
    exported_at: String,
    /// "full" 或 "filtered"。
    mode: &'static str,
    /// 过滤阈值（字节）；全量导出为 0。
    min_size_bytes: u64,
    root_path: String,
    summary: ExportSummary,
    root: ExportNode,
}

/// 拼接绝对路径。`parent` 为空表示根节点（直接用 name，如 "C:" / "/"）。
fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        return name.to_string();
    }
    #[cfg(target_os = "windows")]
    let sep = "\\";
    #[cfg(not(target_os = "windows"))]
    let sep = "/";
    // 已以分隔符结尾（如 "/" 或 "C:\"）直接拼接，避免 "//home" 双斜杠
    if parent.ends_with('\\') || parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        // 盘符 "C:" 或普通路径都补一个分隔符
        format!("{parent}{sep}{name}")
    }
}

/// 目录节点判定（与 prune 的 `is_dir_child` 语义一致）：文件叶子当且仅当
/// `file_count==1 && folder_count==0 && children.is_empty()`。
fn is_dir_node(n: &TreeNode) -> bool {
    !(n.file_count == 1 && n.folder_count == 0 && n.children.is_empty())
}

/// 把缓存的全量树构建为导出树。`parent_size` 用于计算占父目录百分比
/// （根节点传 0 → pct_of_parent = 100）。`threshold` 为过滤阈值（字节）：
/// - `threshold == 0`：全量导出，不过滤
/// - `threshold > 0`：过滤导出，只保留 `size >= threshold` 的目录（递归）与文件；
///   根节点始终保留（作为锚点），其子树仍按阈值裁剪
fn build_export_node(
    node: &TreeNode,
    parent_path: &str,
    parent_size: u64,
    threshold: u64,
) -> ExportNode {
    let path = join_path(parent_path, &node.name);
    let is_dir = is_dir_node(node);
    let pct_of_parent = if parent_size > 0 {
        ((node.size as f64 / parent_size as f64) * 100.0 * 100.0).round() / 100.0
    } else {
        100.0
    };
    let children = if is_dir {
        node.children
            .iter()
            .map(|c| build_export_node(c, &path, node.size, threshold))
            .filter(|c| threshold == 0 || c.size >= threshold)
            .collect()
    } else {
        Vec::new()
    };
    ExportNode {
        path,
        name: node.name.clone(),
        size: node.size,
        pct_of_parent,
        file_count: node.file_count,
        folder_count: node.folder_count,
        last_modified: node.last_modified,
        is_dir,
        children,
    }
}

fn count_export_nodes(n: &ExportNode) -> (u64, u64, u64) {
    let (mut nodes, mut dirs, mut files) = (1u64, 0u64, 0u64);
    if n.is_dir {
        dirs += 1;
    } else {
        files += 1;
    }
    for c in &n.children {
        let (cn, cd, cf) = count_export_nodes(c);
        nodes += cn;
        dirs += cd;
        files += cf;
    }
    (nodes, dirs, files)
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

    // Cooperative cancellation: expose the flag so `cancel_scan` can flip it.
    let cancel = Arc::new(AtomicBool::new(false));
    *state
        .cancel
        .lock()
        .map_err(|_| "cancel mutex poisoned".to_string())? = Some(cancel.clone());

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
            cancel,
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
        .tree
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
        .tree
        .lock()
        .map_err(|_| "scan state mutex poisoned".to_string())?;
    let root = guard
        .as_ref()
        .ok_or_else(|| "no scan result cached; run scan_drive first".to_string())?;

    // 容错：前端在 Treemap 里点到"当前层的大矩形"会触发"自我下钻"，
    // 把当前节点名当成路径首段发过来（例如根层会发 ["C:"]）。这里把等于根名
    // 的首段去掉；同时过滤空段（Linux 绝对路径 "/home/x".split('/') 的首段
    // 是空串），避免无意义的 "node not found"。
    let cleaned: Vec<String> = path.iter().filter(|s| !s.is_empty()).cloned().collect();
    let walk: &[String] = if cleaned.first().map_or(false, |f| f == &root.name) {
        &cleaned[1..]
    } else {
        &cleaned[..]
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
            let mp_raw = it.next().unwrap_or("");
            let fstype = it.next().unwrap_or("");
            // /proc/mounts escapes spaces/tabs/backslashes as octal (`\040` etc.),
            // so a USB labeled "UBUNTU 24_0" reads as "UBUNTU\04024_0". Decode so
            // the returned path is the real on-disk path.
            let dev = crate::scanner::unescape_mount_field(dev);
            let mp = crate::scanner::unescape_mount_field(mp_raw);
            // Keep only real disks: block devices (/dev/sda*, /dev/nvme*,
            // /dev/mmcblk*) plus network shares (cifs/nfs). Everything else
            // (proc, sysfs, tmpfs, overlay, fuse clipboards, loop snap images,
            // ...) is not a user disk and is skipped.
            let real_dev = dev.starts_with("/dev/")
                || matches!(fstype, "cifs" | "nfs" | "nfs4");
            if !real_dev || !mp.starts_with('/') {
                continue;
            }
            // Skip things that are not user disks:
            // - loop-mounted snap images (squashfs) and optical/image media
            // - fuse mounts (FreeRDP clipboard, sshfs, ...)
            // - the EFI system partition (/boot/efi) — no user data
            if matches!(fstype, "squashfs" | "iso9660" | "udf")
                || fstype.starts_with("fuse")
                || mp == "/boot/efi"
                || mp.starts_with("/snap")
                || mp.starts_with("/var/lib/snapd")
            {
                continue;
            }
            if !seen.insert(mp.clone()) {
                continue;
            }
            drives.push(DriveInfo {
                letter: mp,
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

/// Request cancellation of the currently running scan. Cooperative: the walker
/// checks the flag at every directory and stops descending (returns a partial
/// tree). No-op when no scan is running.
#[cfg(not(test))]
#[tauri::command]
fn cancel_scan(state: tauri::State<'_, AppState>) {
    if let Ok(guard) = state.cancel.lock() {
        if let Some(flag) = guard.as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Export the cached full scan tree as a JSON file with absolute paths.
///
/// `filter=false` exports the whole tree; `filter=true` keeps only nodes whose
/// size >= `min_size_mb` (directories recursively, files individually; the root
/// is always kept as an anchor). Pops a native "save as" dialog for the target
/// path (default filename includes a timestamp), writes the file, and returns
/// the saved path. Returns an error if no scan has been cached yet or the user
/// cancels the dialog.
#[cfg(not(test))]
#[tauri::command]
#[allow(unused_variables)] // `window` 仅 Windows 分支使用（对话框父窗口）
fn export_scan_data(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    filter: bool,
    min_size_mb: u64,
) -> Result<String, String> {
    let threshold: u64 = if filter {
        min_size_mb.saturating_mul(1024 * 1024)
    } else {
        0
    };

    // 锁内只做"构建导出树"（遍历+拼路径，毫秒级），随后立即释放锁，
    // 避免在用户慢慢挑保存位置时阻塞 get_node / scan_drive。
    let (root_path, export_root) = {
        let guard = state
            .tree
            .lock()
            .map_err(|_| "scan state mutex poisoned".to_string())?;
        let root = guard
            .as_ref()
            .ok_or_else(|| "no scan result cached; run scan_drive first".to_string())?;
        (root.name.clone(), build_export_node(root, "", 0, threshold))
    };

    let (node_count, dir_count, file_count) = count_export_nodes(&export_root);
    let now = chrono::Local::now();
    let mode: &'static str = if filter { "filtered" } else { "full" };
    let payload = ExportPayload {
        app: "DiskLens",
        version: env!("CARGO_PKG_VERSION"),
        exported_at: now.format("%Y-%m-%d %H:%M:%S %z").to_string(),
        mode,
        min_size_bytes: threshold,
        root_path,
        summary: ExportSummary {
            node_count,
            dir_count,
            file_count,
            total_size: export_root.size,
        },
        root: export_root,
    };
    // 紧凑序列化（无缩进）：减少导出体积，节省 agent 读取时的 token
    let json = serde_json::to_string(&payload).map_err(|e| format!("序列化失败: {e}"))?;

    // 默认文件名带时间戳：disklens-filtered-500mb-20260817-140630.json
    let default_name = if filter {
        format!(
            "disklens-filtered-{}mb-{}.json",
            min_size_mb,
            now.format("%Y%m%d-%H%M%S")
        )
    } else {
        format!("disklens-full-{}.json", now.format("%Y%m%d-%H%M%S"))
    };

    // `mut` 仅在 Windows 分支需要（Linux 分支无 set_parent 赋值）。
    #[allow(unused_mut)]
    let mut dialog = rfd::FileDialog::new()
        .set_title("导出扫描数据")
        .set_file_name(&default_name)
        .add_filter("JSON", &["json"]);
    #[cfg(target_os = "windows")]
    {
        dialog = dialog.set_parent(&window);
    }
    let path = dialog
        .save_file()
        .ok_or_else(|| "导出已取消".to_string())?;

    std::fs::write(&path, json).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
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
            pick_folder,
            cancel_scan,
            export_scan_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
