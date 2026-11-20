// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! AKWF single-cycle library browsing and table loading.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pilab_synth::{TableSet, Wavetable};
use serde::Serialize;

#[derive(Serialize)]
pub struct Group {
    pub name: String,
    pub count: usize,
}

pub struct Library {
    pub root: PathBuf,
    cache: Mutex<HashMap<String, Arc<Wavetable>>>,
    sets: Mutex<HashMap<String, Arc<TableSet>>>,
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with('/') && !rel.split('/').any(|c| c == ".." || c.is_empty())
}

impl Library {
    pub fn new(root: PathBuf) -> Library {
        Library { root, cache: Mutex::new(HashMap::new()), sets: Mutex::new(HashMap::new()) }
    }

    pub fn available(&self) -> bool {
        self.root.is_dir()
    }

    pub fn groups(&self) -> Vec<Group> {
        let mut v = vec![];
        if let Ok(rd) = fs::read_dir(&self.root) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let count = fs::read_dir(e.path()).map(|r| r.flatten().filter(|f| is_wav(&f.path())).count()).unwrap_or(0);
                    if count > 0 {
                        v.push(Group { name, count });
                    }
                }
            }
        }
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn files(&self, group: &str) -> Result<Vec<String>, String> {
        if !safe_rel(group) || group.contains('/') {
            return Err("bad group".into());
        }
        let mut v: Vec<String> = fs::read_dir(self.root.join(group))
            .map_err(|e| e.to_string())?
            .flatten()
            .filter(|f| is_wav(&f.path()))
            .map(|f| format!("{group}/{}", f.file_name().to_string_lossy()))
            .collect();
        v.sort();
        Ok(v)
    }

    pub fn load(&self, rel: &str) -> Result<Arc<Wavetable>, String> {
        if !safe_rel(rel) {
            return Err("bad path".into());
        }
        if let Some(t) = self.cache.lock().unwrap().get(rel) {
            return Ok(t.clone());
        }
        let bytes = fs::read(self.root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let t = Arc::new(Wavetable::from_wav(rel, &bytes)?);
        let mut c = self.cache.lock().unwrap();
        if c.len() > 256 {
            c.clear();
        }
        c.insert(rel.to_string(), t.clone());
        Ok(t)
    }

    /// All cycles of a group as one frame set, in file order (slow the first time; cached).
    pub fn load_group(&self, group: &str) -> Result<Arc<TableSet>, String> {
        if let Some(t) = self.sets.lock().unwrap().get(group) {
            return Ok(t.clone());
        }
        let files = self.files(group)?;
        if files.is_empty() {
            return Err(format!("{group}: no waveforms"));
        }
        let mut frames = Vec::with_capacity(files.len());
        for f in &files {
            let bytes = fs::read(self.root.join(f)).map_err(|e| format!("{f}: {e}"))?;
            frames.push(Wavetable::from_wav(f, &bytes)?);
        }
        let set = Arc::new(TableSet { name: group.to_string(), frames });
        let mut c = self.sets.lock().unwrap();
        if c.len() > 24 {
            c.clear();
        }
        c.insert(group.to_string(), set.clone());
        Ok(set)
    }

    /// Decimated raw cycles of every file in a group, for thumbnails and the stacked view.
    pub fn group_frames(&self, group: &str, points: usize) -> Result<Vec<(String, Vec<f32>)>, String> {
        let mut out = vec![];
        for f in self.files(group)? {
            let bytes = fs::read(self.root.join(&f)).map_err(|e| e.to_string())?;
            let s = pilab_synth::wavetable::parse_wav(&bytes).unwrap_or_default();
            let n = s.len().max(1);
            let pts = if s.is_empty() { vec![0.0; points] } else { (0..points).map(|i| s[i * n / points.max(1)]).collect() };
            out.push((f, pts));
        }
        Ok(out)
    }

    /// Raw cycle (decimated) for drawing.
    pub fn preview(&self, rel: &str, points: usize) -> Result<Vec<f32>, String> {
        if !safe_rel(rel) {
            return Err("bad path".into());
        }
        let bytes = fs::read(self.root.join(rel)).map_err(|e| e.to_string())?;
        let s = pilab_synth::wavetable::parse_wav(&bytes)?;
        let n = s.len().max(1);
        Ok((0..points).map(|i| s[i * n / points.max(1)]).collect())
    }

    pub fn source_note(&self) -> String {
        fs::read_to_string(self.root.join("SOURCE.txt")).unwrap_or_default()
    }
}

fn is_wav(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}
