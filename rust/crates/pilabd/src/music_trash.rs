// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Deleting recordings, safely.
//!
//! "Delete" moves a take to `.trash/` inside the recordings folder it came from (a rename on
//! the same disk: instant, nothing copied, nothing lost). It can be restored until the trash
//! is emptied, which is the only permanent step and needs an explicit confirm. Only sources in
//! the Recordings collection qualify: published music (the Music collection) is never moved
//! or deleted from here. A take that is a folder (`pilab-rec-…/` with its WAV and
//! `record.toml`) moves as a whole. The library scan skips dot folders, so trashed takes drop
//! out of Recordings and come back on restore.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const TRASH: &str = ".trash";
pub const PURGE_CONFIRM: &str = "delete";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// Name of the entry inside .trash/
    pub id: String,
    pub take_id: String,
    pub title: String,
    /// Where it was (absolute), restored to the same place
    pub original: String,
    /// The recordings folder it belongs to
    pub root: String,
    pub deleted_unix: i64,
    pub bytes: u64,
    /// "folder" | "file"
    pub what: String,
}

fn size_of(p: &Path) -> u64 {
    if p.is_dir() {
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0)
    } else {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// What to move for a take file under `root`: its own folder when the take is a
/// folder of its own (holds record.toml, or is named like the file), else the file.
pub fn unit_for(file: &Path, root: &Path) -> Result<PathBuf, String> {
    let f = canon(file);
    let r = canon(root);
    if !f.starts_with(&r) || f == r {
        return Err("not inside a recordings folder".into());
    }
    if f.components().any(|c| c.as_os_str() == TRASH) {
        return Err("already in the trash".into());
    }
    let parent = f.parent().map(Path::to_path_buf).unwrap_or_default();
    if parent != r && parent.starts_with(&r) {
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let dir_name = parent.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if parent.join("record.toml").exists() || dir_name == stem {
            return Ok(parent);
        }
    }
    Ok(f)
}

/// Move a take into `root/.trash/`. Returns the entry written beside it.
pub fn trash(file: &Path, root: &Path, take_id: &str, title: &str, now: i64) -> Result<Entry, String> {
    let unit = unit_for(file, root)?;
    let r = canon(root);
    let tdir = r.join(TRASH);
    std::fs::create_dir_all(&tdir).map_err(|e| format!("trash folder: {e}"))?;
    let base = unit.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "take".into());
    let mut id = format!("{now}-{base}");
    let mut n = 1;
    while tdir.join(&id).exists() {
        n += 1;
        id = format!("{now}-{n}-{base}");
    }
    let bytes = size_of(&unit);
    let what = if unit.is_dir() { "folder" } else { "file" }.to_string();
    std::fs::rename(&unit, tdir.join(&id)).map_err(|e| format!("move to trash: {e}"))?;
    let e = Entry { id: id.clone(), take_id: take_id.into(), title: title.into(), original: unit.display().to_string(), root: r.display().to_string(), deleted_unix: now, bytes, what };
    let _ = std::fs::write(tdir.join(format!("{id}.json")), serde_json::to_string_pretty(&e).unwrap_or_default());
    Ok(e)
}

/// Everything in the trash of these roots, newest first.
pub fn list(roots: &[PathBuf]) -> Vec<Entry> {
    let mut v = vec![];
    for r in roots {
        let t = canon(r).join(TRASH);
        let Ok(rd) = std::fs::read_dir(&t) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                if let Some(en) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Entry>(&s).ok()) {
                    if t.join(&en.id).exists() {
                        v.push(en);
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| b.deleted_unix.cmp(&a.deleted_unix).then(b.id.cmp(&a.id)));
    v
}

fn find(roots: &[PathBuf], id: &str) -> Result<(PathBuf, Entry), String> {
    if id.is_empty() || id.contains('/') || id.starts_with('.') {
        return Err("bad trash id".into());
    }
    for r in roots {
        let t = canon(r).join(TRASH);
        if let Some(en) = std::fs::read_to_string(t.join(format!("{id}.json"))).ok().and_then(|s| serde_json::from_str::<Entry>(&s).ok()) {
            if t.join(id).exists() {
                return Ok((t, en));
            }
        }
    }
    Err("not found: no such item in the trash".into())
}

/// Put a trashed take back where it was.
pub fn restore(roots: &[PathBuf], id: &str) -> Result<Entry, String> {
    let (t, en) = find(roots, id)?;
    let dest = PathBuf::from(&en.original);
    if !dest.starts_with(canon(Path::new(&en.root))) {
        return Err("trash entry points outside its recordings folder".into());
    }
    if dest.exists() {
        return Err(format!("{} exists again; rename it first", dest.display()));
    }
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::rename(t.join(id), &dest).map_err(|e| format!("restore: {e}"))?;
    let _ = std::fs::remove_file(t.join(format!("{id}.json")));
    Ok(en)
}

/// Delete for good (one entry, or all when `id` is None).
pub fn purge(roots: &[PathBuf], id: Option<&str>) -> Result<(usize, u64), String> {
    let items: Vec<Entry> = match id {
        Some(i) => vec![find(roots, i)?.1],
        None => list(roots),
    };
    let (mut n, mut bytes) = (0, 0);
    for en in items {
        let t = PathBuf::from(&en.root).join(TRASH);
        let p = t.join(&en.id);
        let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        r.map_err(|e| format!("delete {}: {e}", en.id))?;
        let _ = std::fs::remove_file(t.join(format!("{}.json", en.id)));
        n += 1;
        bytes += en.bytes;
    }
    Ok((n, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pilab-trash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folder_take_round_trip() {
        let root = tmp("rt");
        let take = root.join("pilab-rec-20261125-084304");
        std::fs::create_dir_all(&take).unwrap();
        std::fs::write(take.join("pilab-rec-20261125-084304.wav"), b"RIFFdata").unwrap();
        std::fs::write(take.join("record.toml"), b"id = \"x\"\n").unwrap();
        let e = trash(&take.join("pilab-rec-20261125-084304.wav"), &root, "pilab-rec-20261125-084304", "Take 08:43:04", 100).unwrap();
        assert_eq!(e.what, "folder");
        assert!(!take.exists());
        let roots = vec![root.clone()];
        assert_eq!(list(&roots).len(), 1);
        restore(&roots, &e.id).unwrap();
        assert!(take.join("record.toml").exists());
        assert!(list(&roots).is_empty());
        // Trash then empty for good.
        let e = trash(&take.join("pilab-rec-20261125-084304.wav"), &root, "t", "t", 101).unwrap();
        assert_eq!(purge(&roots, Some(&e.id)).unwrap().0, 1);
        assert!(list(&roots).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_outside_the_recordings_folder() {
        let root = tmp("out");
        let music = tmp("music");
        std::fs::write(music.join("song.mp3"), b"x").unwrap();
        assert!(trash(&music.join("song.mp3"), &root, "x", "x", 1).is_err());
        assert!(music.join("song.mp3").exists());
        assert!(unit_for(&root, &root).is_err());
        assert!(restore(&[root.clone()], "../etc").is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&music);
    }

    #[test]
    fn loose_file_take_moves_alone() {
        let root = tmp("loose");
        std::fs::write(root.join("a.wav"), b"x").unwrap();
        std::fs::write(root.join("b.wav"), b"y").unwrap();
        let e = trash(&root.join("a.wav"), &root, "a", "a", 5).unwrap();
        assert_eq!(e.what, "file");
        assert!(root.join("b.wav").exists());
        restore(&[root.clone()], &e.id).unwrap();
        assert!(root.join("a.wav").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
