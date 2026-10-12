//! Just enough PE parsing to read a DLL's version: the headers, the section
//! table and the `.rsrc` section. nvngx_dlss.dll and libxess.dll run to tens
//! of MB; reading them whole to learn four numbers would make opening the
//! tab wait on the disk.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// DOS header, PE header and section table all live well inside this.
const HEADER_READ: usize = 64 * 1024;
/// A resource section bigger than this isn't a version block worth reading.
const MAX_RSRC: u32 = 64 * 1024 * 1024;
/// `VS_FIXEDFILEINFO.dwSignature`.
const FIXED_FILE_INFO: [u8; 4] = 0xFEEF04BDu32.to_le_bytes();

/// `a.b.c.d` from the version resource, if `path` is a PE that has one.
pub fn file_version(path: &Path) -> Option<String> {
    let v = version_parts(path)?;
    Some(format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]))
}

/// True if `candidate` is a PE at least as new as `reference` — both need a
/// version resource, otherwise there's nothing to compare and it's `false`.
pub fn version_newer_or_equal(candidate: &Path, reference: &Path) -> bool {
    match (version_parts(candidate), version_parts(reference)) {
        (Some(c), Some(r)) => c >= r,
        _ => false,
    }
}

/// True for an OptiScaler build under any name: its version resource names
/// OptiScaler (the release's setup_windows.bat checks the original file name
/// for the same reason — finding earlier installs).
pub fn is_optiscaler(path: &Path) -> bool {
    rsrc_contains_utf16(path, "OptiScaler")
}

/// True for NVIDIA's DLSS DLLs (nvngx_dlss, nvngx_dlssd, nvngx_dlssg). Their
/// version resource has no useful original file name (a build number), but
/// every one carries NGX's own keys and "DLSS" in its name.
pub fn is_dlss_dll(path: &Path) -> bool {
    let Some(rsrc) = rsrc(path) else {
        return false;
    };
    ["NGXApiVersion", "DLSS"].iter().all(|needle| {
        let needle: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
        rsrc.windows(needle.len()).any(|w| w == needle)
    })
}

/// True if the version resource mentions `needle` (UTF-16, as Windows stores
/// the strings) — e.g. "DLSS" for NVIDIA's nvngx_dlss.dll.
pub fn rsrc_contains_utf16(path: &Path, needle: &str) -> bool {
    let Some(rsrc) = rsrc(path) else {
        return false;
    };
    let needle: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    !needle.is_empty() && rsrc.windows(needle.len()).any(|w| w == needle)
}

fn version_parts(path: &Path) -> Option<[u16; 4]> {
    let rsrc = rsrc(path)?;
    let at = rsrc.windows(4).position(|w| w == FIXED_FILE_INFO)?;
    let ms = u32le(&rsrc, at + 8)?;
    let ls = u32le(&rsrc, at + 12)?;
    Some([(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16])
}

/// The raw bytes of the `.rsrc` section, if `path` is a PE with one.
fn rsrc(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut head = vec![0u8; HEADER_READ];
    let n = read_up_to(&mut file, &mut head)?;
    head.truncate(n);

    if head.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = u32le(&head, 0x3C)? as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = u16le(&head, coff + 2)? as usize;
    let optional_size = u16le(&head, coff + 16)? as usize;
    let table = coff + 20 + optional_size;

    for i in 0..sections.min(96) {
        let section = table + i * 40;
        if !head.get(section..section + 8)?.starts_with(b".rsrc") {
            continue;
        }
        let size = u32le(&head, section + 16)?; // SizeOfRawData
        let offset = u32le(&head, section + 20)?; // PointerToRawData
        let file_len = file.metadata().ok()?.len();
        if size == 0 || size > MAX_RSRC || u64::from(offset) + u64::from(size) > file_len {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        file.seek(SeekFrom::Start(offset.into())).ok()?;
        let n = read_up_to(&mut file, &mut data)?;
        data.truncate(n);
        return Some(data);
    }
    None
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> Option<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]).ok()? {
            0 => break,
            n => total += n,
        }
    }
    Some(total)
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal PE: headers, one `.rsrc` section holding a fixed file info
    /// block for `version` and a UTF-16 `name`. Enough for what this module
    /// reads; nothing could load it.
    pub(crate) fn fake_pe(version: [u16; 4], name: &str) -> Vec<u8> {
        let mut rsrc = vec![0u8; 16];
        rsrc.extend_from_slice(&FIXED_FILE_INFO);
        rsrc.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        rsrc.extend_from_slice(
            &((u32::from(version[0]) << 16) | u32::from(version[1])).to_le_bytes(),
        );
        rsrc.extend_from_slice(
            &((u32::from(version[2]) << 16) | u32::from(version[3])).to_le_bytes(),
        );
        rsrc.extend(name.encode_utf16().flat_map(u16::to_le_bytes));

        let pe_at = 0x40usize;
        let optional_size = 0xF0usize;
        let table = pe_at + 4 + 20 + optional_size;
        let data_at = 0x400usize;
        let mut file = vec![0u8; data_at];
        file[0..2].copy_from_slice(b"MZ");
        file[0x3C..0x40].copy_from_slice(&(pe_at as u32).to_le_bytes());
        file[pe_at..pe_at + 4].copy_from_slice(b"PE\0\0");
        let coff = pe_at + 4;
        file[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        file[coff + 16..coff + 18].copy_from_slice(&(optional_size as u16).to_le_bytes());
        file[table..table + 5].copy_from_slice(b".rsrc");
        file[table + 16..table + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        file[table + 20..table + 24].copy_from_slice(&(data_at as u32).to_le_bytes());
        file.extend_from_slice(&rsrc);
        file
    }

    #[test]
    fn reads_the_version_and_strings_of_a_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nvngx_dlss.dll");
        std::fs::write(&path, fake_pe([310, 4, 0, 0], "NVIDIA DLSS")).unwrap();
        assert_eq!(file_version(&path).as_deref(), Some("310.4.0.0"));
        assert!(rsrc_contains_utf16(&path, "DLSS"));
        assert!(!is_optiscaler(&path));
        assert!(!is_dlss_dll(&path), "DLSS alone isn't enough");
        let real = dir.path().join("real.dll");
        std::fs::write(&real, fake_pe([310, 5, 0, 0], "NVIDIA DLSS NGXApiVersion")).unwrap();
        assert!(is_dlss_dll(&real));

        let older = dir.path().join("older.dll");
        std::fs::write(&older, fake_pe([3, 7, 10, 0], "x")).unwrap();
        assert!(version_newer_or_equal(&path, &older));
        assert!(!version_newer_or_equal(&older, &path));
    }

    #[test]
    fn anything_else_has_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("readme.txt");
        std::fs::write(&text, "MZ but not really").unwrap();
        assert_eq!(file_version(&text), None);
        assert_eq!(file_version(&dir.path().join("missing.dll")), None);
        assert!(!version_newer_or_equal(&text, &text));
    }
}
