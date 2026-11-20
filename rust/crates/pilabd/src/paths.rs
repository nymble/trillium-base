// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
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
    pub fn scenes(&self) -> PathBuf {
        self.config.join("scenes")
    }
    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.toml")
    }
    pub fn state(&self) -> PathBuf {
        self.config.join("state.toml")
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
