// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Read-only check of a Trillium Zero firmware image (UF2, XIAO nRF52840 Sense UF2
//! bootloader). An image is accepted only if every block is a well-formed UF2 block for
//! the nRF52840 family and writes inside the application window 0x27000..0xEC000: after
//! the MBR and SoftDevice S140 v7, before the settings page and the bootloader. A
//! bootloader-family image is always refused. This module never writes anything.

use serde::Serialize;

pub const MAGIC0: u32 = 0x0A32_4655;
pub const MAGIC1: u32 = 0x9E5D_5157;
pub const MAGIC_END: u32 = 0x0AB1_6F30;
pub const FLAG_NOT_MAIN_FLASH: u32 = 0x0000_0001;
pub const FLAG_FAMILY: u32 = 0x0000_2000;
pub const FAMILY_NRF52840: u32 = 0xADA5_2840;
pub const FAMILY_BOOTLOADER: u32 = 0xD663_823C;
pub const APP_START: u32 = 0x0002_7000;
pub const APP_END: u32 = 0x000E_C000;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub ok: bool,
    pub blocks: u32,
    pub bytes: usize,
    pub lo: u32,
    pub hi: u32,
    pub family: String,
    pub sha256: String,
    /// Version from the file name (xiao-sense-fw-X.Y.Z.uf2) when present.
    pub version: Option<String>,
    pub problems: Vec<String>,
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn version_from_name(name: &str) -> Option<String> {
    let base = name.rsplit('/').next()?;
    let stem = base.strip_suffix(".uf2")?;
    let v = stem.rsplit('-').next()?;
    let ok = v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    ok.then(|| v.to_string())
}

/// Compares two "X.Y.Z" versions.
pub fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let p = |s: &str| s.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    p(a).cmp(&p(b))
}

pub fn check(data: &[u8], name: &str, sha256: &str) -> Report {
    let mut problems = vec![];
    let mut lo = u32::MAX;
    let mut hi = 0u32;
    let mut family = String::new();
    let n = data.len() / 512;
    if data.is_empty() || data.len() % 512 != 0 {
        problems.push(format!("size {} is not a whole number of 512-byte UF2 blocks", data.len()));
    }
    let mut total = None;
    let mut seen = vec![];
    for i in 0..n {
        let b = &data[i * 512..(i + 1) * 512];
        if u32le(b, 0) != MAGIC0 || u32le(b, 4) != MAGIC1 || u32le(b, 508) != MAGIC_END {
            problems.push(format!("block {i}: bad UF2 magic"));
            break;
        }
        let flags = u32le(b, 8);
        let addr = u32le(b, 12);
        let size = u32le(b, 16);
        let no = u32le(b, 20);
        let count = u32le(b, 24);
        let fam = u32le(b, 28);
        if flags & FLAG_NOT_MAIN_FLASH != 0 {
            continue;
        }
        if flags & FLAG_FAMILY == 0 {
            problems.push(format!("block {i}: no family id"));
            break;
        }
        if fam == FAMILY_BOOTLOADER {
            problems.push("bootloader image: refused (only application images are accepted)".into());
            break;
        }
        if fam != FAMILY_NRF52840 {
            problems.push(format!("block {i}: family 0x{fam:08X} is not nRF52840"));
            break;
        }
        family = "nRF52840".into();
        if size == 0 || size > 476 {
            problems.push(format!("block {i}: payload size {size}"));
            break;
        }
        if addr < APP_START || addr + size > APP_END {
            problems.push(format!("block {i}: writes 0x{addr:05X}..0x{:05X}, outside the application window 0x27000..0xEC000", addr + size));
            break;
        }
        match total {
            None => total = Some(count),
            Some(t) if t != count => {
                problems.push(format!("block {i}: block count {count} differs from {t}"));
                break;
            }
            _ => {}
        }
        if no >= count {
            problems.push(format!("block {i}: number {no} ≥ count {count}"));
            break;
        }
        seen.push(no);
        lo = lo.min(addr);
        hi = hi.max(addr + size);
    }
    if problems.is_empty() {
        seen.sort_unstable();
        seen.dedup();
        if Some(seen.len() as u32) != total {
            problems.push(format!("{} distinct blocks for a declared count of {}", seen.len(), total.unwrap_or(0)));
        }
    }
    Report {
        ok: problems.is_empty() && n > 0,
        blocks: n as u32,
        bytes: data.len(),
        lo: if lo == u32::MAX { 0 } else { lo },
        hi,
        family,
        sha256: sha256.into(),
        version: version_from_name(name),
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(addr: u32, no: u32, count: u32, fam: u32) -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[0..4].copy_from_slice(&MAGIC0.to_le_bytes());
        b[4..8].copy_from_slice(&MAGIC1.to_le_bytes());
        b[8..12].copy_from_slice(&FLAG_FAMILY.to_le_bytes());
        b[12..16].copy_from_slice(&addr.to_le_bytes());
        b[16..20].copy_from_slice(&256u32.to_le_bytes());
        b[20..24].copy_from_slice(&no.to_le_bytes());
        b[24..28].copy_from_slice(&count.to_le_bytes());
        b[28..32].copy_from_slice(&fam.to_le_bytes());
        b[508..512].copy_from_slice(&MAGIC_END.to_le_bytes());
        b
    }

    #[test]
    fn accepts_an_application_image() {
        let img = [block(0x27000, 0, 2, FAMILY_NRF52840), block(0x27100, 1, 2, FAMILY_NRF52840)].concat();
        let r = check(&img, "xiao-sense-fw-0.8.0.uf2", "");
        assert!(r.ok, "{:?}", r.problems);
        assert_eq!(r.version.as_deref(), Some("0.8.0"));
        assert_eq!((r.lo, r.hi), (0x27000, 0x27200));
    }

    #[test]
    fn refuses_softdevice_bootloader_and_garbage() {
        assert!(!check(&block(0x1000, 0, 1, FAMILY_NRF52840), "a.uf2", "").ok);
        assert!(!check(&block(0xF4000, 0, 1, FAMILY_NRF52840), "a.uf2", "").ok);
        assert!(!check(&block(0x27000, 0, 1, FAMILY_BOOTLOADER), "a.uf2", "").ok);
        assert!(!check(&block(0x27000, 0, 2, FAMILY_NRF52840), "a.uf2", "").ok); // missing block
        assert!(!check(b"hello", "a.uf2", "").ok);
        assert_eq!(cmp_version("0.10.0", "0.8.0"), std::cmp::Ordering::Greater);
    }
}
