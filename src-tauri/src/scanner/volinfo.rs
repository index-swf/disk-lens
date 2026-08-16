//! Volume-level information for the status bar and allocation rounding.
//!
//! Windows: three cheap Win32 queries against the volume root (`C:\`) — one-time
//! per scan, no per-file cost. Linux: a single `statvfs` call plus a
//! `/proc/mounts` lookup for the filesystem name.

use std::path::Path;

/// Snapshot of volume properties.
#[derive(Clone, Debug)]
pub struct VolumeInfo {
    /// Bytes available to the calling process (≈ free space on the volume).
    pub free_bytes: u64,
    /// Total capacity of the volume in bytes.
    pub total_bytes: u64,
    /// Filesystem type name, e.g. "NTFS" / "ext4". Empty when unavailable.
    pub fs_type: String,
    /// Bytes per allocation unit (cluster), e.g. 4096. 0 when unknown.
    pub cluster_size: u32,
}

/// Query volume info for the volume backing `root`. Returns `None` when the
/// volume root cannot be resolved / the API calls fail (e.g. network share).
pub fn volume_info(root: &Path) -> Option<VolumeInfo> {
    #[cfg(target_os = "windows")]
    {
        volume_info_windows(root)
    }
    #[cfg(not(target_os = "windows"))]
    {
        volume_info_unix(root)
    }
}

/// Bytes per cluster for the volume backing `root` (0 when unknown).
pub fn cluster_size(root: &Path) -> u32 {
    volume_info(root).map(|v| v.cluster_size).unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn volume_info_windows(root: &Path) -> Option<VolumeInfo> {
    use crate::scanner::parallel::to_wide;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetVolumeInformationW,
    };

    let root_str = root.to_string_lossy();
    // GetDiskFreeSpace*W want a root like "C:\" (trailing backslash).
    let volume = if root_str.ends_with('\\') {
        root_str.into_owned()
    } else {
        format!("{}\\", root_str)
    };
    let wide = to_wide(&volume);
    let root_ptr = PCWSTR::from_raw(wide.as_ptr());

    unsafe {
        let mut free = 0u64;
        let mut total = 0u64;
        let mut free_total = 0u64;
        if GetDiskFreeSpaceExW(root_ptr, Some(&mut free), Some(&mut total), Some(&mut free_total))
            .is_err()
        {
            return None;
        }

        let mut spc = 0u32;
        let mut bps = 0u32;
        let mut free_clusters = 0u32;
        let mut total_clusters = 0u32;
        let cluster_size = if GetDiskFreeSpaceW(
            root_ptr,
            Some(&mut spc),
            Some(&mut bps),
            Some(&mut free_clusters),
            Some(&mut total_clusters),
        )
        .is_ok()
        {
            spc.checked_mul(bps).unwrap_or(0)
        } else {
            0
        };

        let mut fs_buf = [0u16; 64];
        let mut fs_type = String::new();
        if GetVolumeInformationW(root_ptr, None, None, None, None, Some(&mut fs_buf)).is_ok() {
            fs_type = String::from_utf16_lossy(&fs_buf)
                .trim_end_matches('\0')
                .to_string();
        }

        Some(VolumeInfo {
            free_bytes: free,
            total_bytes: total,
            fs_type,
            cluster_size,
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn volume_info_unix(root: &Path) -> Option<VolumeInfo> {
    use std::os::unix::ffi::OsStrExt;

    let cpath = std::ffi::CString::new(root.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(cpath.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize.max(1) as u64;
    Some(VolumeInfo {
        free_bytes: (st.f_bavail as u64).saturating_mul(frsize),
        total_bytes: (st.f_blocks as u64).saturating_mul(frsize),
        fs_type: read_fs_type(root),
        cluster_size: frsize.min(u32::MAX as u64) as u32,
    })
}

/// Best-effort filesystem name from `/proc/mounts`: match the *deepest* mount
/// point that prefixes `path` (e.g. "/home" wins over "/" for "/home/user/..."),
/// then read the fs field ("ext4", "ntfs3", "vfat", ...). The "/" entry is the
/// natural fallback (length 1).
#[cfg(not(target_os = "windows"))]
fn read_fs_type(path: &Path) -> String {
    let p = path.to_string_lossy();
    let mut best: Option<(usize, String)> = None;
    if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let mut it = line.split_whitespace();
            let _dev = it.next();
            // /proc/mounts escapes spaces etc. as octal (\040); decode so the
            // prefix match works for mount points with spaces (e.g. USB labels).
            let mp = crate::scanner::unescape_mount_field(it.next().unwrap_or(""));
            let fstype = it.next().unwrap_or("");
            if p.starts_with(&mp) && best.as_ref().map_or(true, |b| mp.len() > b.0) {
                best = Some((mp.len(), fstype.to_string()));
            }
        }
    }
    best.map(|(_, t)| t).unwrap_or_default()
}
