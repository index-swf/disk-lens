//! Volume-level information for the status bar and allocation rounding.
//!
//! All three calls (free/total space, cluster geometry, filesystem name) are
//! cheap Win32 queries against the volume root (e.g. `C:\`) — one-time per scan,
//! no per-file cost.

use crate::scanner::parallel::to_wide;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetVolumeInformationW,
};

/// Snapshot of volume properties.
#[derive(Clone, Debug)]
pub struct VolumeInfo {
    /// Bytes available to the calling process (≈ free space on the volume).
    pub free_bytes: u64,
    /// Total capacity of the volume in bytes.
    pub total_bytes: u64,
    /// Filesystem type name, e.g. "NTFS". Empty when the call failed.
    pub fs_type: String,
    /// Bytes per allocation unit (cluster), e.g. 4096. 0 when unknown.
    pub cluster_size: u32,
}

/// Query volume info for the volume backing `root`. Returns `None` when the
/// volume root cannot be resolved / the API calls fail (e.g. network share).
pub fn volume_info(root: &Path) -> Option<VolumeInfo> {
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

/// Bytes per cluster for the volume backing `root` (0 when unknown).
pub fn cluster_size(root: &Path) -> u32 {
    volume_info(root).map(|v| v.cluster_size).unwrap_or(0)
}
