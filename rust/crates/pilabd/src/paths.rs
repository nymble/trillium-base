// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! File locations and the pairing token.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

/// Write a file so a crash or power cut leaves either the old or the new content, never a
/// torn one: write a sibling temp file, flush it to the card, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Keep a copy of a config file that failed to parse before it is replaced by defaults.
pub fn keep_bad_copy(path: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let name = format!("{}.bad-{stamp}", path.file_name()?.to_string_lossy());
    let dest = path.with_file_name(name);
    fs::copy(path, &dest).ok().map(|_| dest)
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn new(config: Option<PathBuf>) -> Paths {
        let config = config.unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("pilab")
        });
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("pilab");
        Paths { config, data }
    }
    pub fn token(&self) -> PathBuf {
        self.config.join("console-token")
    }
    /// One-time pairing code written by `pilabctl pair --qr`: "CODE EXPIRY_UNIX_SECONDS".
    pub fn pair_code(&self) -> PathBuf {
        self.config.join("pair-code")
    }
    pub fn scenes(&self) -> PathBuf {
        self.config.join("scenes")
    }
    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.toml")
    }
    pub fn state(&self) -> PathBuf {
        self.config.join("state.toml")
    }
    /// Sample library root (holds samples/index.json): user, drive, then base tier.
    pub fn samples_root(&self) -> PathBuf {
        let cands = [
            self.data.join("library"),
            PathBuf::from("/srv/library/pilab-library"),
            PathBuf::from("/srv/library"),
            PathBuf::from("/usr/share/pilab/library"),
        ];
        for c in &cands {
            if c.join("samples/index.json").is_file() {
                return c.clone();
            }
        }
        cands[0].clone()
    }
    /// AKWF single-cycle library: first existing of the user, drive and base tiers.
    pub fn akwf_root(&self) -> PathBuf {
        let cands = [
            self.data.join("library/akwf"),
            PathBuf::from("/srv/library/pilab-library/akwf"),
            PathBuf::from("/srv/library/akwf"),
            PathBuf::from("/usr/share/pilab/library/akwf"),
        ];
        for c in &cands {
            if c.is_dir() {
                return c.clone();
            }
        }
        cands[0].clone()
    }
}

fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    let mut f = fs::File::open("/dev/urandom")?;
    std::io::Read::read_exact(&mut f, &mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads the pairing token, creating a new random one (0600) if missing.
pub fn ensure_token(path: &Path) -> std::io::Result<String> {
    if let Ok(s) = fs::read_to_string(path) {
        let t = s.trim().to_string();
        if t.len() >= 32 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            return Ok(t);
        }
    }
    write_new_token(path)
}

pub fn write_new_token(path: &Path) -> std::io::Result<String> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
        let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o700));
    }
    let t = random_hex(24)?;
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        writeln!(f, "{t}")?;
    }
    fs::rename(&tmp, path)?;
    Ok(t)
}

/// Stable, non-secret device id: FNV-1a of /etc/machine-id (never the raw id).
pub fn device_id() -> String {
    let mid = fs::read_to_string("/etc/machine-id").unwrap_or_else(|_| "unknown".into());
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("pilab:{}", mid.trim()).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")[..12].to_string()
}

pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "pilab".into())
}

/// Constant-time string comparison for tokens.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod atomic_tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let d = std::env::temp_dir().join(format!("pilab-atomic-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.toml");
        write_atomic(&f, b"a = 1\n").unwrap();
        write_atomic(&f, b"a = 2\n").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "a = 2\n");
        assert!(!f.with_extension("tmp").exists());
        let kept = keep_bad_copy(&f).unwrap();
        assert_eq!(fs::read_to_string(kept).unwrap(), "a = 2\n");
        let _ = fs::remove_dir_all(&d);
    }
}
