use crate::models::{ScanErrors, ScannerError, TreeNode};
use crate::scanner::{volinfo, ScanCtx};
use rayon::prelude::*;
#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::Window;
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FileStandardInfo, FILE_STANDARD_INFO, GetFileInformationByHandleEx,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Encode a path as a null-terminated UTF-16 wide string for Win32 APIs.
#[cfg(windows)]
pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

/// Open a handle to a file or directory. Callers MUST close it with `CloseHandle`.
/// `FILE_FLAG_BACKUP_SEMANTICS` lets us open directory handles.
#[cfg(windows)]
fn open_handle(path: &Path) -> windows::core::Result<HANDLE> {
    let wide = to_wide(&path.to_string_lossy());
    unsafe {
        CreateFileW(
            PCWSTR::from_raw(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
}

/// Return `(size, allocated_size)` for a file (Windows).
///
/// Uses `GetFileInformationByHandleEx` for the true on-disk (allocated) size and
/// falls back to `metadata().len()` when the handle cannot be opened (e.g. no
/// permission). The handle is always closed.
#[cfg(windows)]
pub(crate) fn file_sizes(path: &Path) -> (u64, u64) {
    if let Ok(handle) = open_handle(path) {
        let mut info = FILE_STANDARD_INFO::default();
        let res = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                &mut info as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        };
        unsafe {
            let _ = CloseHandle(handle);
        }
        if res.is_ok() {
            let size = info.EndOfFile.max(0) as u64;
            let alloc = info.AllocationSize.max(0) as u64;
            return (size, alloc);
        }
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let len = meta.len();
        (len, len)
    } else {
        (0, 0)
    }
}

/// Return `(size, allocated_size)` for a file (Unix/Linux).
///
/// `st_blocks * 512` is the true on-disk allocation straight from the metadata —
/// no extra syscall, no handle open (simpler than the Windows path).
#[cfg(not(windows))]
pub(crate) fn file_sizes(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let size = meta.len();
        let alloc = meta.blocks().saturating_mul(512).max(size);
        (size, alloc)
    } else {
        (0, 0)
    }
}

/// Return `meta.modified()` as Unix seconds (UTC), or 0 when unavailable.
/// Cheap: the caller already holds the `fs::Metadata`, so no extra syscall.
fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Recursively scan a directory and return its aggregated `TreeNode`.
/// `is_root` keeps the drive root from being treated as a skippable entry.
/// `precise` controls whether each file's *allocated* (on-disk, cluster-rounded)
/// size is fetched via a dedicated handle open. When `precise` is false we use the
/// logical size already available from the directory entry (`entry.metadata()`),
/// which avoids one `CreateFileW` + `GetFileInformationByHandleEx` syscall per
/// file — the single biggest speedup for large volumes.
///
/// Cycle safety: we never follow symlinks or junctions (`is_symlink()` is true for
/// both on Windows), which is sufficient to prevent infinite loops on NTFS (real
/// directory cycles cannot occur without reparse points). Unreadable directories
/// are skipped and scanning continues.
fn scan_dir(path: &Path, ctx: &ScanCtx, is_root: bool, precise: bool) -> Option<TreeNode> {
    let read_dir = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(e) => {
            // 无权限 / 无法访问 -> 记录错误并继续扫描其余目录。
            ctx.record_error(format!("无法读取目录: {} ({e})", path.display()));
            return None;
        }
    };

    let mut file_children: Vec<TreeNode> = Vec::new();
    let mut dir_paths: Vec<PathBuf> = Vec::new();
    let mut dir_size = 0u64;
    let mut dir_alloc = 0u64;
    let mut dir_file_count = 0u32;

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let p = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                ctx.record_error(format!("无法读取文件元数据: {} ({e})", p.display()));
                continue;
            }
        };
        // Never follow symlinks / junctions: prevents loops and double counting.
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            dir_paths.push(p);
        } else {
            let (sz, al) = if precise {
                file_sizes(&p)
            } else {
                // Logical size is already available from the directory entry —
                // reuse it for `size` and round it up to the volume's cluster
                // size for `allocated_size` (no separate handle open). This makes
                // the "占用分配空间" column real (>= logical size, e.g. small
                // files claim a whole 4K cluster) without the per-file syscall
                // cost of `precise` mode. When the cluster size is unknown
                // (e.g. network share) we fall back to reporting the raw size.
                let len = meta.len();
                let al = if ctx.cluster > 0 && len > 0 {
                    let c = ctx.cluster as u64;
                    ((len + c - 1) / c) * c
                } else {
                    len
                };
                (len, al)
            };
            dir_size += sz;
            dir_alloc += al;
            dir_file_count += 1;
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            file_children.push(TreeNode {
                name,
                size: sz,
                allocated_size: al,
                file_count: 1,
                folder_count: 0,
                last_modified: mtime_secs(&meta),
                children: vec![],
                truncated: false,
            });
        }
    }

    ctx.scanned_files.fetch_add(dir_file_count as u64, Ordering::Relaxed);
    ctx.scanned_folders.fetch_add(1, Ordering::Relaxed);
    ctx.report(path);

    // Descend into sub-directories in parallel.
    let sub_nodes: Vec<TreeNode> = dir_paths
        .par_iter()
        .filter_map(|d| scan_dir(d, ctx, false, precise))
        .collect();

    let folder_count = sub_nodes.len() as u32;
    let mut size = dir_size;
    let mut allocated = dir_alloc;
    let mut file_count = dir_file_count;
    let mut folder_count_total = folder_count;
    for sn in &sub_nodes {
        size += sn.size;
        allocated += sn.allocated_size;
        file_count += sn.file_count;
        folder_count_total += sn.folder_count;
    }

    let mut children = sub_nodes;
    children.extend(file_children);
    // Largest children first for nicer treemap / table ordering.
    children.sort_by(|a, b| b.size.cmp(&a.size));

    let name = if is_root {
        path.to_string_lossy().into_owned()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    Some(TreeNode {
        name,
        size,
        allocated_size: allocated,
        file_count,
        folder_count: folder_count_total,
        // `meta` above is a *child* entry's metadata; the directory's own mtime
        // must be read from `path` itself (one extra stat per directory — cheap
        // next to the `read_dir` we already pay).
        last_modified: std::fs::metadata(path)
            .ok()
            .map(|m| mtime_secs(&m))
            .unwrap_or(0),
        children,
        truncated: false,
    })
}

/// Public entry: parallel, NTFS-agnostic walker. Always returns a correctly sized tree.
///
/// * `precise` — when false, skips the per-file handle open and reports logical
///   size as the allocated size (much faster; see `scan_dir`).
/// * `threads` — optional override for the Rayon worker count. `None` uses all
///   logical CPUs. **Both branches build a *local* thread pool and run the walk
///   inside `pool.install`** rather than falling back to Rayon's process-global
///   pool. This is deliberate: mixing a scoped local pool with the global pool in
///   one process (e.g. a `Some(1)` scan followed by a `None` scan) poisons the
///   global pool and can slow a subsequent parallel walk by ~60×. Scoped pools
///   sidestep that entirely and keep repeated scans deterministic.
pub fn scan_parallel(
    root: &Path,
    window: Option<Window>,
    precise: bool,
    threads: Option<u32>,
) -> Result<(TreeNode, ScanErrors), ScannerError> {
    let mut ctx = ScanCtx::new(window);
    // One cheap Win32 query for the volume cluster geometry; used by the fast
    // path to round file sizes up to whole clusters for the allocated column.
    ctx.cluster = volinfo::cluster_size(root);
    let walker = || scan_dir(root, &ctx, true, precise);
    let n = match threads {
        Some(n) if n > 0 => n as usize,
        _ => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .map_err(|e| ScannerError::Msg(format!("failed to build thread pool: {e}")))?;
    let tree = pool.install(walker);
    let errors = ctx.take_errors();
    tree.ok_or_else(|| ScannerError::Msg(format!("failed to scan {}", root.display())))
        .map(|t| (t, errors))
}
