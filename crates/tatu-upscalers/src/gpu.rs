//! The GPUs in this PC, from their PCI ids: which vendor, whether DLSS can
//! run (NVIDIA on NVIDIA's own driver — not nouveau/NVK, not Windows' basic
//! display driver), and for AMD the RDNA generation FSR 4 depends on. Only
//! devices actually present: a driver left behind by a card that was
//! swapped out doesn't count.
//!
//! The RDNA generation comes, most reliable first, from the graphics core
//! version the Linux driver reports, a table of known device ids, and the
//! marketing name (`RX 9070`, `780M`, `Navi 31`) — so a card newer than the
//! table still reads right as long as its name follows AMD's series.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gpu {
    pub vendor: Vendor,
    /// As Windows names it, or from the system's PCI id database on Linux
    /// (empty when it has none).
    pub name: String,
    pub device_id: u16,
    /// AMD's RDNA generation (2, 3, 4), when the device id is a known one.
    pub rdna: Option<u8>,
    /// Runs DLSS: an NVIDIA card on NVIDIA's own driver.
    pub dlss: bool,
    /// Kernel driver (Linux) or driver service (Windows): `nvidia`,
    /// `nouveau`, `amdgpu`, `nvlddmkm`...
    pub driver: String,
}

const NVIDIA: u16 = 0x10de;
const AMD: u16 = 0x1002;
const INTEL: u16 = 0x8086;

fn gpu(vendor_id: u16, device_id: u16, name: String, driver: String) -> Gpu {
    let vendor = match vendor_id {
        NVIDIA => Vendor::Nvidia,
        AMD => Vendor::Amd,
        INTEL => Vendor::Intel,
        _ => Vendor::Other,
    };
    let driver_lc = driver.to_ascii_lowercase();
    Gpu {
        vendor,
        rdna: (vendor == Vendor::Amd)
            .then(|| rdna(device_id).or_else(|| rdna_from_name(&name)))
            .flatten(),
        // `nvidia` on Linux, `nvlddmkm` on Windows — and an RTX.
        dlss: vendor == Vendor::Nvidia
            && (driver_lc == "nvidia" || driver_lc == "nvlddmkm")
            && rtx(device_id, &name),
        name,
        device_id,
        driver,
    }
}

/// An NVIDIA card with the tensor cores DLSS runs on: RTX 20 and every
/// generation since — not GTX 10/16.
/// ponytail: the name decides when there is one (Windows and pci.ids say
/// GTX/RTX); otherwise the chip id: Turing's RTX chips are `1E00-1F7F`, its
/// GTX 16 ones (TU116/TU117) `1F80-21FF`, Ampere and later from `2200`.
/// Older ids are all pre-RTX.
fn rtx(device: u16, name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    if name.contains("GTX") {
        return false;
    }
    name.contains("RTX") || matches!(device, 0x1e00..=0x1f7f) || device >= 0x2200
}

/// RDNA generation from an AMD PCI device id.
/// ponytail: known ranges and APU ids as of 2026 (Navi 2x `73xx`/`742x-743F`,
/// Navi 3x `744x-74FF`, Navi 4x `75xx`; Steam Deck and Phoenix/Strix APUs by
/// id). A chip missing here falls back to its name ([`rdna_from_name`]); one
/// with neither reads as unknown and the tab offers both FSR 4 switches.
fn rdna(device: u16) -> Option<u8> {
    match device {
        0x7550..=0x75ff => Some(4),
        0x7440..=0x74ff => Some(3),
        // Phoenix (780M), Strix Point (890M), Strix Halo (8060S): RDNA 3 / 3.5.
        0x15bf | 0x15c8 | 0x150e | 0x1586 => Some(3),
        0x73a0..=0x73ff | 0x7420..=0x743f => Some(2),
        // Steam Deck LCD / OLED, Rembrandt (680M).
        0x163f | 0x1435 | 0x1681 => Some(2),
        _ => None,
    }
}

/// RDNA generation from AMD's naming: the series digit of a Radeon RX / PRO
/// model (`RX 9070` → 4, `RX 7600M XT` → 3, `PRO W6800` → 2, `AI PRO R9700`
/// → 4), the iGPU model (`780M` → 3, `8060S` → 3.5 → 3, `680M` → 2), or the
/// chip codename in the PCI id database (`Navi 31` → 3). Generic names
/// (`AMD Radeon(TM) Graphics`) say nothing.
fn rdna_from_name(name: &str) -> Option<u8> {
    let upper = name.to_ascii_uppercase();
    let tokens: Vec<&str> = upper
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let series = |digit: u8| match digit {
        b'9' => Some(4),
        b'7' => Some(3),
        b'6' => Some(2),
        b'5' => Some(1),
        _ => None,
    };
    for (i, token) in tokens.iter().enumerate() {
        let previous = i.checked_sub(1).map(|p| tokens[p]);
        let prefix = token.len()
            - token
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .len();
        let digits = token[prefix..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let (letters, number, suffix) = (
            &token[..prefix],
            &token[prefix..prefix + digits],
            &token[prefix + digits..],
        );
        let first = number.bytes().next();
        // `Navi 31`, `Navi 48`: the tens digit is the generation.
        if previous == Some("NAVI") && letters.is_empty() && digits == 2 {
            return first.map(|d| d - b'0').filter(|g| (1..=9).contains(g));
        }
        // Discrete: `RX 7900`, `RX 7600M`, `RX 6800S`, `PRO W7900`, `PRO
        // R9700` — not FirePro's older `W9100` (GCN).
        let discrete = (letters.is_empty() && previous == Some("RX"))
            || ((letters == "W" || letters == "R") && previous == Some("PRO"));
        if discrete && digits == 4 && matches!(suffix, "" | "M" | "S") {
            return first.and_then(series);
        }
        // iGPU: `780M`, `890M`, `680M`, `8060S`.
        if letters.is_empty() && previous != Some("RX") {
            match (digits, suffix, first) {
                (3, "M", Some(b'7' | b'8')) | (4, "S", Some(b'8')) => return Some(3),
                (3, "M", Some(b'6')) => return Some(2),
                _ => {}
            }
        }
    }
    None
}

/// The generation from the graphics core version amdgpu reports through IP
/// discovery: gfx10.1 RDNA 1, gfx10.3 RDNA 2, gfx11 RDNA 3 (11.5 is 3.5),
/// gfx12 RDNA 4 — and on, one major version per generation.
#[cfg_attr(windows, allow(dead_code))]
fn rdna_from_gc(major: u32, minor: u32) -> Option<u8> {
    match major {
        10 if minor >= 3 => Some(2),
        10 => Some(1),
        11.. => u8::try_from(major - 8).ok(),
        _ => None,
    }
}

/// `PCI\VEN_10DE&DEV_2786&SUBSYS_...` → (vendor, device). Anything that isn't
/// a PCI device (a virtual display) is `None`.
fn pci_ids(hardware_id: &str) -> Option<(u16, u16)> {
    let id = hardware_id.to_ascii_uppercase();
    let rest = id.strip_prefix("PCI\\")?;
    let hex = |key: &str| {
        let start = rest.find(key)? + key.len();
        u16::from_str_radix(rest.get(start..start + 4)?, 16).ok()
    };
    Some((hex("VEN_")?, hex("DEV_")?))
}

/// Every display adapter present, discrete first when there are several (a
/// laptop's dGPU is the one games run on).
pub fn gpus() -> Vec<Gpu> {
    let mut list = devices();
    list.sort_by_key(|g| match g.vendor {
        Vendor::Nvidia => 0,
        // Discrete Radeons are 0x6xxx-0x7xxx; APUs sit around 0x1xxx.
        Vendor::Amd if g.device_id >= 0x6000 => 0,
        Vendor::Amd => 1,
        Vendor::Intel => 2,
        Vendor::Other => 3,
    });
    list
}

#[cfg(windows)]
fn devices() -> Vec<Gpu> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, SP_DEVINFO_DATA, SPDRP_DEVICEDESC, SPDRP_HARDWAREID,
        SPDRP_SERVICE, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW,
    };

    let mut out = Vec::new();
    // SAFETY: plain SetupAPI enumeration. The set is destroyed before
    // returning, every buffer is sized and handed over with its size, and
    // property strings are cut at their first NUL.
    unsafe {
        let set = SetupDiGetClassDevsW(
            &GUID_DEVCLASS_DISPLAY,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT,
        );
        if set == -1 {
            return out;
        }
        let mut index = 0;
        loop {
            let mut info: SP_DEVINFO_DATA = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut info) == 0 {
                break;
            }
            index += 1;
            let property = |which| {
                let mut buf = [0u16; 512];
                let ok = SetupDiGetDeviceRegistryPropertyW(
                    set,
                    &info,
                    which,
                    std::ptr::null_mut(),
                    buf.as_mut_ptr().cast(),
                    (buf.len() * 2) as u32,
                    std::ptr::null_mut(),
                );
                if ok == 0 {
                    return String::new();
                }
                // REG_SZ, or REG_MULTI_SZ whose first entry is the most specific.
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..end])
            };
            if let Some((vendor, device)) = pci_ids(&property(SPDRP_HARDWAREID)) {
                out.push(gpu(
                    vendor,
                    device,
                    property(SPDRP_DEVICEDESC),
                    property(SPDRP_SERVICE),
                ));
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// The system's PCI id database (hwdata / pciutils), where distros keep it.
#[cfg(not(windows))]
const PCI_IDS: [&str; 3] = [
    "/usr/share/hwdata/pci.ids",
    "/usr/share/misc/pci.ids",
    "/usr/share/pci.ids",
];

/// A device's name in a `pci.ids` database, e.g. `Navi 31 [Radeon RX 7900
/// XT/7900 XTX/7900 GRE/7900M]`.
#[cfg_attr(windows, allow(dead_code))]
fn pci_name(db: &str, vendor: u16, device: u16) -> Option<String> {
    let vendor = format!("{vendor:04x}");
    let device = format!("\t{device:04x}");
    let mut in_vendor = false;
    for line in db.lines() {
        if !line.starts_with('\t') {
            // A vendor line (`1002  Advanced Micro Devices...`); comments and
            // the device-class section never match a vendor id.
            in_vendor = line.starts_with(&vendor);
        } else if in_vendor && line.starts_with(&device) {
            return Some(line[device.len()..].trim().to_string());
        }
    }
    None
}

/// `/sys/class/drm/card*/device`: the PCI ids, the bound kernel driver, the
/// name from `pci.ids`, and for amdgpu its graphics core version.
#[cfg(not(windows))]
fn devices() -> Vec<Gpu> {
    use std::fs;

    let db = PCI_IDS
        .iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `card0`, not the connectors (`card0-DP-1`) or render nodes.
        if !name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let dev = entry.path().join("device");
        let Ok(real) = fs::canonicalize(&dev) else {
            continue;
        };
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        let id = |file: &str| {
            let text = fs::read_to_string(dev.join(file)).ok()?;
            u16::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
        };
        let (Some(vendor), Some(device)) = (id("vendor"), id("device")) else {
            continue;
        };
        let driver = fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let name = pci_name(&db, vendor, device).unwrap_or_default();
        let mut gpu = gpu(vendor, device, name, driver);
        // `GC` is a link to the numbered folder (11 = the GC hardware id).
        let gc = ["GC", "11"]
            .iter()
            .map(|hw| dev.join("ip_discovery/die/0").join(hw).join("0"))
            .find(|dir| dir.is_dir());
        let version = |dir: &std::path::Path, file: &str| {
            fs::read_to_string(dir.join(file))
                .ok()?
                .trim()
                .parse::<u32>()
                .ok()
        };
        if gpu.vendor == Vendor::Amd
            && let Some(dir) = gc
            && let (Some(major), Some(minor)) = (version(&dir, "major"), version(&dir, "minor"))
            && let Some(generation) = rdna_from_gc(major, minor)
        {
            gpu.rdna = Some(generation);
        }
        out.push(gpu);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdna_generation_by_device_id() {
        assert_eq!(rdna(0x7550), Some(4)); // RX 9070 XT
        assert_eq!(rdna(0x744c), Some(3)); // RX 7900 XTX
        assert_eq!(rdna(0x743f), Some(2)); // RX 6400 (Navi 24), right below Navi 3x
        assert_eq!(rdna(0x163f), Some(2)); // Steam Deck
        assert_eq!(rdna(0x6fdf), None); // RX 580
    }

    #[test]
    fn rdna_generation_by_name_for_chips_the_table_misses() {
        let cases = [
            ("AMD Radeon RX 9070 XT", Some(4)),
            ("AMD Radeon RX 7600M XT", Some(3)),
            ("AMD Radeon RX 6800S", Some(2)),
            ("AMD Radeon RX 5700 XT", Some(1)),
            ("AMD Radeon PRO W7900", Some(3)),
            ("AMD Radeon AI PRO R9700", Some(4)),
            ("AMD Radeon 780M Graphics", Some(3)),
            ("AMD Radeon 8060S Graphics", Some(3)),
            ("AMD Radeon 680M", Some(2)),
            ("Navi 48 [Radeon RX 9070/9070 XT/9070 GRE]", Some(4)),
            (
                "Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]",
                Some(3),
            ),
            ("AMD Radeon RX 580 Series", None),
            ("AMD FirePro W9100", None),
            ("Hawaii XT GL [FirePro W9100]", None),
            ("AMD Radeon R9 290", None),
            ("AMD Radeon(TM) Graphics", None),
        ];
        for (name, expected) in cases {
            assert_eq!(rdna_from_name(name), expected, "{name}");
        }
    }

    #[test]
    fn rdna_generation_from_the_graphics_core_version() {
        assert_eq!(rdna_from_gc(10, 1), Some(1));
        assert_eq!(rdna_from_gc(10, 3), Some(2));
        assert_eq!(rdna_from_gc(11, 5), Some(3));
        assert_eq!(rdna_from_gc(12, 0), Some(4));
        assert_eq!(rdna_from_gc(9, 4), None); // Vega
    }

    #[test]
    fn device_names_from_the_pci_id_database() {
        let db = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t744c  Navi 31 [Radeon RX 7900 XT/7900 XTX]\n\t\t1002 0e3b  RX 7900 GRE\n10de  NVIDIA Corporation\n\t744c  not this one\nC 03  Display controller\n";
        assert_eq!(
            pci_name(db, 0x1002, 0x744c).as_deref(),
            Some("Navi 31 [Radeon RX 7900 XT/7900 XTX]")
        );
        assert_eq!(
            pci_name(db, 0x10de, 0x744c).as_deref(),
            Some("not this one")
        );
        assert_eq!(pci_name(db, 0x1002, 0x7550), None);
    }

    #[test]
    fn dlss_needs_an_rtx() {
        let nvidia = |device, name: &str| gpu(NVIDIA, device, name.into(), "nvlddmkm".into()).dlss;
        assert!(!nvidia(0x1b80, "NVIDIA GeForce GTX 1080"));
        assert!(!nvidia(0x2184, "NVIDIA GeForce GTX 1660"));
        assert!(!nvidia(0x2184, ""), "TU116 by id");
        assert!(!nvidia(0x1f82, ""), "TU117 by id");
        // A GTX 1650 on the RTX chip (TU106): only its name tells.
        assert!(!nvidia(0x1f0a, "TU106 [GeForce GTX 1650]"));
        assert!(nvidia(0x1f08, "NVIDIA GeForce RTX 2060"));
        assert!(nvidia(0x1f08, ""));
        assert!(nvidia(0x2786, ""));
        assert!(nvidia(0x2f04, "NVIDIA GeForce RTX 5070"));
        assert!(nvidia(0x1eb1, "Quadro RTX 4000"));
    }

    #[test]
    fn dlss_needs_nvidias_own_driver() {
        assert!(gpu(NVIDIA, 0x2786, String::new(), "nvidia".into()).dlss);
        assert!(gpu(NVIDIA, 0x2786, String::new(), "nvlddmkm".into()).dlss);
        assert!(!gpu(NVIDIA, 0x2786, String::new(), "nouveau".into()).dlss);
        assert!(!gpu(NVIDIA, 0x2786, String::new(), "BasicDisplay".into()).dlss);
        assert!(!gpu(AMD, 0x744c, String::new(), "amdgpu".into()).dlss);
        assert_eq!(gpu(INTEL, 0x56a0, String::new(), "i915".into()).rdna, None);
    }

    #[test]
    fn pci_ids_from_a_hardware_id() {
        assert_eq!(
            pci_ids(r"PCI\VEN_10DE&DEV_2786&SUBSYS_51411458&REV_A1"),
            Some((0x10de, 0x2786))
        );
        assert_eq!(pci_ids(r"pci\ven_1002&dev_744c"), Some((0x1002, 0x744c)));
        assert_eq!(pci_ids(r"Root\Parsec\VDA"), None);
    }
}
