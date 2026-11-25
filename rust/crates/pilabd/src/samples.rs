// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Sample banks installed by scripts/fetch-samples.sh: the index, and an instrument cache
//! (banks stay in RAM once loaded; the Pi has plenty).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pilab_synth::{sampler, Instrument};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bank {
    pub id: String,
    pub title: String,
    pub category: String,
    pub drum: bool,
    pub sfz: String,
    pub license: String,
    pub bytes: u64,
    pub files: u64,
    pub source: String,
    /// Attribution the licence requires (CC BY); shown wherever the bank is listed.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub credit: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Index {
    items: Vec<Bank>,
}

pub struct SampleLib {
    pub root: PathBuf,
    cache: Mutex<HashMap<String, Arc<Instrument>>>,
}

impl SampleLib {
    pub fn new(root: PathBuf) -> SampleLib {
        SampleLib { root, cache: Mutex::new(HashMap::new()) }
    }

    pub fn banks(&self) -> Vec<Bank> {
        std::fs::read_to_string(self.root.join("samples/index.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Index>(&s).ok())
            .map(|i| i.items)
            .unwrap_or_default()
    }

    pub fn bank(&self, id: &str) -> Option<Bank> {
        self.banks().into_iter().find(|b| b.id == id)
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.cache.lock().unwrap().contains_key(id)
    }

    pub fn loaded_bytes(&self) -> usize {
        self.cache.lock().unwrap().values().map(|i| i.bytes).sum()
    }

    /// Loads (or returns the cached) instrument for a bank id. Slow on first use (tens to
    /// a few hundred ms): call from a blocking context, never the audio thread.
    pub fn load(&self, id: &str) -> Result<Arc<Instrument>, String> {
        if let Some(i) = self.cache.lock().unwrap().get(id) {
            return Ok(i.clone());
        }
        let b = self.bank(id).ok_or_else(|| format!("sample bank \"{id}\" is not installed (run scripts/fetch-samples.sh)"))?;
        if b.sfz.contains("..") || b.sfz.starts_with('/') {
            return Err("bad sfz path in index".into());
        }
        let inst = Arc::new(sampler::load_sfz(&self.root.join(&b.sfz))?);
        self.cache.lock().unwrap().insert(id.to_string(), inst.clone());
        Ok(inst)
    }
}
