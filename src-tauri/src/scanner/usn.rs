use crate::models::ScannerError;
use crate::scanner::parallel::{file_sizes, to_wide};
use crate::scanner::tree_builder::{build_tree, FlatEntry};
use crate::scanner::{volume_root, ScanCtx};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::Window;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetVolumeInformationW, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;

const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00B3;
const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00F4;

#[repr(C)]
#[derive(Default)]
struct UsnJournalDataV0 {
    usn_journal_id: u64,
    first_usn: i64,
    next_usn: i64,
    lowest_valid_usn: i64,
    max_usn: i64,
    maximum_size: u64,
    allocation_delta: u64,
}

#[repr(C)]
#[derive(Default)]
struct MftEnumDataV0 {
    start_file_reference_number: u64,
    lowest_usn: i64,
    highest_usn: i64,
}

/// Detect whether the drive backing `path` is formatted as NTFS.
pub fn is_ntfs(path: &Path) -> bool {
    let root = volume_root(path);
    let wide = to_wide(&root);
    let mut fs_buf = [0u16; 32];
    let res = unsafe {
        GetVolumeInformationW(
            PCWSTR::from_raw(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_buf),
        )
    };
    if res.is_err() {
        return false;
    }
    let name = String::from_utf16_lossy(&fs_buf)
        .trim_end_matches('\0')
        .to_string();
    name.eq_ignore_ascii_case("NTFS")
}

/// Full USN-journal scan. Returns `Err` on any failure so the caller can fall back
/// to the parallel walker. On success the returned tree carries correct sizes
/// (gathered by stat-ing each file via its reconstructed path) together with the
/// scan errors collected along the way. MFT reference numbers are used to rebuild
/// the parent/child hierarchy.
pub fn scan_usn(
    root: &Path,
    window: Option<Window>,
) -> Result<(crate::models::TreeNode, crate::models::ScanErrors), ScannerError> {
    let drive = {
        let s = root.to_string_lossy();
        match s.find(':') {
            Some(idx) => s[..=idx].to_string(), // "C:"
            None => "C:".to_string(),
        }
    };
    let vol_path = format!("\\\\.\\{}", drive); // "\\\\.\\C:"
    let wide = to_wide(&vol_path);
    let handle = unsafe {
        CreateFileW(
            PCWSTR::from_raw(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }?;

    // Confirm the USN journal exists.
    let mut journal = UsnJournalDataV0::default();
    let mut ret = 0u32;
    let q = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            0,
            Some(&mut journal as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<UsnJournalDataV0>() as u32,
            Some(&mut ret),
            None,
        )
    };
    if q.is_err() {
        return Err(ScannerError::Msg("USN journal not available".into()));
    }

    let mut entries: Vec<FlatEntry> = Vec::new();
    let mut frn_to_name: HashMap<u64, String> = HashMap::new();
    let mut frn_to_parent: HashMap<u64, u64> = HashMap::new();
    let ctx = ScanCtx::new(window.clone());
    let vol_root = volume_root(root);

    let mut start: u64 = 0;
    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let input = MftEnumDataV0 {
            start_file_reference_number: start,
            lowest_usn: 0,
            highest_usn: i64::MAX,
        };
        let mut bytes = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                Some(&input as *const _ as *const std::ffi::c_void),
                std::mem::size_of::<MftEnumDataV0>() as u32,
                Some(buffer.as_mut_ptr() as *mut std::ffi::c_void),
                buffer.len() as u32,
                Some(&mut bytes),
                None,
            )
        };
        if ok.is_err() || bytes <= 8 {
            break;
        }

        let next_start = u64::from_ne_bytes(buffer[0..8].try_into().unwrap());
        let mut offset = 8usize;
        let bytes_us = bytes as usize;
        while offset + 8 <= bytes_us {
            let rec_len =
                u32::from_ne_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize;
            if rec_len < 8 || offset + rec_len > bytes_us {
                break;
            }
            let major = u16::from_ne_bytes(buffer[offset + 4..offset + 6].try_into().unwrap());
            // Only parse USN_RECORD_V2; V3 uses a 128-bit FRN layout we skip for safety.
            if major == 2 {
                let frn = u64::from_ne_bytes(buffer[offset + 8..offset + 16].try_into().unwrap());
                let parent =
                    u64::from_ne_bytes(buffer[offset + 16..offset + 24].try_into().unwrap());
                let attrs =
                    u32::from_ne_bytes(buffer[offset + 52..offset + 56].try_into().unwrap());
                let name_len =
                    u16::from_ne_bytes(buffer[offset + 56..offset + 58].try_into().unwrap())
                        as usize;
                let name_off =
                    u16::from_ne_bytes(buffer[offset + 58..offset + 60].try_into().unwrap())
                        as usize;
                if name_off + name_len <= rec_len {
                    let name_bytes = &buffer[offset + name_off..offset + name_off + name_len];
                    let name = String::from_utf16_lossy(&from_utf16(name_bytes));
                    let is_dir = (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
                    frn_to_name.insert(frn, name.clone());
                    frn_to_parent.insert(frn, parent);
                    entries.push(FlatEntry {
                        frn,
                        parent_frn: parent,
                        name,
                        is_dir,
                        size: 0,
                        allocated_size: 0,
                    });
                    ctx.scanned_files
                        .fetch_add(if is_dir { 0 } else { 1 }, Ordering::Relaxed);
                    ctx.scanned_folders
                        .fetch_add(if is_dir { 1 } else { 0 }, Ordering::Relaxed);
                }
            }
            offset += rec_len;
        }

        ctx.report(Path::new(&vol_path));

        if next_start == 0 {
            break;
        }
        start = next_start;
    }

    // Stat each file (USN records carry no size) via its reconstructed full path.
    for e in entries.iter_mut() {
        if !e.is_dir {
            if let Some(full) = reconstruct_path(e.frn, &frn_to_name, &frn_to_parent, &vol_root) {
                let p = PathBuf::from(&full);
                let (sz, al) = file_sizes(&p);
                e.size = sz;
                e.allocated_size = al;
            }
        }
    }

    let mut tree = build_tree(entries)
        .ok_or_else(|| ScannerError::Msg("failed to build tree from USN data".into()))?;
    tree.name = root.to_string_lossy().into_owned();
    let errors = ctx.take_errors();
    Ok((tree, errors))
}

/// Reconstruct a file's full path by walking parent FRNs up to the volume root.
fn reconstruct_path(
    frn: u64,
    names: &HashMap<u64, String>,
    parents: &HashMap<u64, u64>,
    volume_root: &str,
) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = frn;
    let mut guard = 0;
    while let Some(name) = names.get(&cur) {
        parts.push(name.as_str());
        match parents.get(&cur) {
            Some(&p) if p != cur && p != 0 => cur = p,
            _ => break,
        }
        guard += 1;
        if guard > 1000 {
            break;
        }
    }
    parts.reverse();
    if parts.is_empty() {
        return None;
    }
    Some(format!("{}{}", volume_root, parts.join("\\")))
}

fn from_utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect()
}
