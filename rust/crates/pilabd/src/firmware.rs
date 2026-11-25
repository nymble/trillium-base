// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Trillium Zero firmware: what is running, images kept on the Base, a read-only check of
//! any image, and the update path (bootloader, copy to the UF2 drive, verify).
//!
//! The update is the only step that writes the Zero's flash. It needs `confirm` and the
//! image must pass [`pilab_core::uf2::check`] (application window only, never a
//! bootloader). Images and backups live under `~/.local/share/pilab/firmware/`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use pilab_core::uf2;
use serde_json::{json, Value};

use crate::devices::Manager;

pub const RELEASES_API: &str = "https://api.github.com/repos/nymble/trillium/releases/latest";

pub fn dir(data: &Path) -> PathBuf {
    data.join("firmware")
}

fn sha256_file(p: &Path) -> String {
    Command::new("sha256sum").arg(p).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.split_whitespace().next().map(str::to_string)).unwrap_or_default()
}

fn safe_name(name: &str) -> Result<String, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let ok = base.ends_with(".uf2") && base.len() <= 96 && base.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c)) && !base.starts_with('.');
    ok.then(|| base.to_string()).ok_or_else(|| "file name: letters, digits, - _ . + and ending in .uf2".into())
}

/// Mounted UF2 bootloader drives (a directory with INFO_UF2.TXT).
pub fn bootloader_drives() -> Vec<Value> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut v = vec![];
    for line in mounts.lines() {
        let mut it = line.split_whitespace();
        let (Some(dev), Some(mp)) = (it.next(), it.next()) else { continue };
        let mp = mp.replace("\\040", " ");
        let info = Path::new(&mp).join("INFO_UF2.TXT");
        if let Ok(text) = std::fs::read_to_string(&info) {
            let board = text.lines().find_map(|l| l.strip_prefix("Board-ID:")).unwrap_or("").trim().to_string();
            let sd = text.lines().find_map(|l| l.strip_prefix("SoftDevice:")).unwrap_or("").trim().to_string();
            v.push(json!({ "device": dev, "mount": mp, "board_id": board, "softdevice": sd, "info": text.lines().take(6).collect::<Vec<_>>().join(" · ") }));
        }
    }
    let unmounted = std::fs::read_dir("/dev/disk/by-label").map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.starts_with("XIAO")).collect::<Vec<_>>()).unwrap_or_default();
    for n in unmounted {
        if !v.iter().any(|d| d["mount"].as_str().is_some_and(|m| m.ends_with(&n))) {
            v.push(json!({ "device": format!("/dev/disk/by-label/{n}"), "mount": null, "board_id": "", "info": "bootloader drive present but not mounted" }));
        }
    }
    v
}

pub fn images(data: &Path) -> Vec<Value> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(dir(data)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("uf2") {
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let bytes = std::fs::read(&p).unwrap_or_default();
            let r = uf2::check(&bytes, &name, &sha256_file(&p));
            v.push(json!({ "name": name, "report": r }));
        }
    }
    v.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    v
}

/// Latest published firmware, from a cached copy of the GitHub release (refreshed on request).
pub fn latest(data: &Path, refresh: bool) -> Value {
    let cache = dir(data).join("latest.json");
    if refresh {
        let _ = std::fs::create_dir_all(dir(data));
        let out = Command::new("curl").args(["-fsSL", "--max-time", "10", "-H", "Accept: application/vnd.github+json", RELEASES_API]).output();
        if let Ok(o) = out {
            if o.status.success() {
                if let Ok(j) = serde_json::from_slice::<Value>(&o.stdout) {
                    let asset = j["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.starts_with("xiao-sense-fw-") && n.ends_with(".uf2"))).cloned());
                    let slim = json!({
                        "tag": j["tag_name"], "published": j["published_at"], "url": j["html_url"],
                        "asset": asset.as_ref().map(|a| json!({ "name": a["name"], "size": a["size"], "download": a["browser_download_url"] })),
                        "checked": crate::timefmt::now_secs(),
                    });
                    let _ = std::fs::write(&cache, serde_json::to_vec_pretty(&slim).unwrap());
                }
            }
        }
    }
    std::fs::read_to_string(&cache).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

pub fn status(dm: &Manager, data: &Path) -> Value {
    let zeros: Vec<Value> = dm
        .connected_keys_live()
        .into_iter()
        .map(|k| {
            let h = dm.hello_of(&k);
            json!({
                "key": k,
                "firmware": h.as_ref().map(|h| h.firmware.clone()),
                "imu": h.as_ref().map(|h| h.imu()),
                "mic": h.as_ref().map(|h| h.mic()),
                "ble": h.as_ref().map(|h| h.ble()),
                "bootloader_on_request": h.as_ref().map(|h| h.caps & pilab_core::zero::CAP_BOARD != 0),
                "params": h.as_ref().map(|h| h.params),
            })
        })
        .collect();
    json!({ "zeros": zeros, "drives": bootloader_drives(), "images": images(data), "latest": latest(data, false) })
}

/// Stores an uploaded image on the Base and checks it. Nothing is sent to the device.
pub fn check_upload(dm: &Manager, data: &Path, name: &str, bytes: &[u8]) -> Result<Value, String> {
    let name = safe_name(name)?;
    if bytes.len() > 1024 * 1024 {
        return Err("image larger than 1 MiB: not a Zero application image".into());
    }
    let d = dir(data);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let path = d.join(&name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let r = uf2::check(bytes, &name, &sha256_file(&path));
    if !r.ok {
        let _ = std::fs::remove_file(&path);
    }
    Ok(json!({ "report": r, "kept": r.ok, "compare": compare(dm, &r) }))
}

/// Checks a file already on the Base (CLI: any path the lab user can read).
pub fn check_path(dm: &Manager, path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r = uf2::check(&bytes, &path.to_string_lossy(), &sha256_file(path));
    Ok(json!({ "report": r, "compare": compare(dm, &r) }))
}

fn compare(dm: &Manager, r: &uf2::Report) -> Value {
    let Some(key) = dm.first_connected() else { return json!({ "note": "no Zero connected" }) };
    let cur = dm.hello_of(&key).map(|h| h.firmware).unwrap_or_default();
    let Some(v) = r.version.as_deref() else { return json!({ "current": cur, "note": "image version unknown (name it xiao-sense-fw-X.Y.Z.uf2)" }) };
    let note = match uf2::cmp_version(v, &cur) {
        std::cmp::Ordering::Greater => "newer than the Zero's firmware",
        std::cmp::Ordering::Equal => "same version as the Zero",
        std::cmp::Ordering::Less => "OLDER than the Zero's firmware (a downgrade)",
    };
    json!({ "current": cur, "image": v, "note": note })
}

/// Installs a checked image: bootloader, back up CURRENT.UF2, copy, wait for the Zero to
/// come back, read its version. Writes the Zero's flash; `confirm` is required.
pub fn update(dm: &Manager, data: &Path, key: &str, name: &str, confirm: bool, allow_downgrade: bool) -> Result<Value, String> {
    if !confirm {
        return Err("a firmware update writes the Zero's flash: send confirm: true".into());
    }
    let name = safe_name(name)?;
    let path = dir(data).join(&name);
    let bytes = std::fs::read(&path).map_err(|_| format!("{name} is not stored on the Base; check it first"))?;
    let r = uf2::check(&bytes, &name, &sha256_file(&path));
    if !r.ok {
        return Err(format!("image refused: {}", r.problems.join("; ")));
    }
    let cmp = compare(dm, &r);
    if cmp["note"].as_str().is_some_and(|n| n.starts_with("OLDER")) && !allow_downgrade {
        return Err(format!("{name} is older than the Zero's firmware {}; pass allow_downgrade to install it anyway", cmp["current"]));
    }
    let before = dm.hello_of(key).map(|h| h.firmware).unwrap_or_default();
    let mut log = vec![format!("image {name} ok ({} blocks, 0x{:05X}..0x{:05X})", r.blocks, r.lo, r.hi)];
    let drive = match bootloader_drives().into_iter().find(|d| d["mount"].is_string()) {
        Some(d) => d,
        None => {
            dm.enter_bootloader(key)?;
            log.push("Zero restarted into its bootloader".into());
            let t = Instant::now();
            loop {
                if let Some(d) = bootloader_drives().into_iter().find(|d| d["mount"].is_string()) {
                    break d;
                }
                if let Some(d) = bootloader_drives().into_iter().find(|d| d["mount"].is_null()) {
                    let dev = d["device"].as_str().unwrap_or("").to_string();
                    let _ = Command::new("udisksctl").args(["mount", "--no-user-interaction", "-b", &dev]).output();
                }
                if t.elapsed() > Duration::from_secs(20) {
                    return Err(format!("the bootloader drive did not appear within 20 s ({}); a plain reset returns the Zero to its current firmware", log.join("; ")));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    };
    let mount = PathBuf::from(drive["mount"].as_str().unwrap());
    let board = drive["board_id"].as_str().unwrap_or("");
    if !board.contains("nRF52840") {
        return Err(format!("drive board id \"{board}\" is not an nRF52840 XIAO; nothing written"));
    }
    let bdir = dir(data).join("backups").join(format!("{}-{}", crate::timefmt::now_secs(), board.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
    std::fs::create_dir_all(&bdir).map_err(|e| e.to_string())?;
    for f in ["CURRENT.UF2", "INFO_UF2.TXT"] {
        let _ = std::fs::copy(mount.join(f), bdir.join(f));
    }
    log.push(format!("backup in {}", bdir.display()));
    let target = mount.join(&name);
    {
        use std::io::Write;
        let mut fh = std::fs::File::create(&target).map_err(|e| format!("write to drive: {e}"))?;
        fh.write_all(&bytes).map_err(|e| format!("write to drive: {e}"))?;
        let _ = fh.sync_all();
    }
    log.push("image copied; the bootloader restarts the Zero".into());
    // The devices loop sees the new ALSA client (or boot id) and re-reads the Zero.
    let old_client = dm.client_of(key);
    let t = Instant::now();
    let after = loop {
        std::thread::sleep(Duration::from_secs(1));
        let k = dm.first_connected();
        let changed = k.as_ref().and_then(|k| dm.client_of(k)) != old_client;
        if changed && t.elapsed() > Duration::from_secs(3) {
            if let Some(h) = k.and_then(|k| dm.hello_of(&k)) {
                break Some(h.firmware);
            }
        }
        if t.elapsed() > Duration::from_secs(45) {
            break None;
        }
    };
    Ok(json!({ "ok": after.as_deref() == r.version.as_deref() || (r.version.is_none() && after.is_some()), "before": before, "after": after, "log": log }))
}
