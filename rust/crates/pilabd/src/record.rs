// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Local recording of the synth output (32-bit float WAV) with a small record card
//! (`record.toml`) per take. Recordings are masters until filed to the NAS Archive.

use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::audio::RtShared;
use crate::timefmt;

/// Stop before a WAV reaches the 4 GiB RIFF limit.
const MAX_BYTES: u64 = 3_500_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordCard {
    pub id: String,
    pub created: String,
    pub created_unix: i64,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String,
    pub bytes: u64,
    pub source: String,
    pub scene: String,
    /// "master" until a verified copy is filed on the NAS Archive.
    pub role: String,
    pub filed: bool,
    pub filed_to: String,
    /// SPDX id or LicenseRef-*; never guessed.
    pub rights: String,
    pub dropped_frames: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecState {
    pub recording: bool,
    pub id: String,
    pub seconds: f64,
    pub path: String,
    pub last_error: String,
}

struct Active {
    file: File,
    dir: PathBuf,
    card: RecordCard,
    frames: u64,
}

pub struct Recorder {
    shared: Arc<RtShared>,
    sr: u32,
    active: Mutex<Option<Active>>,
    last_error: Mutex<String>,
}

fn wav_header(f: &mut File, sr: u32, data_bytes: u32) -> std::io::Result<()> {
    let ch = 2u16;
    let bits = 32u16;
    let block = ch * bits / 8;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_bytes).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&ch.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * block as u32).to_le_bytes())?;
    f.write_all(&block.to_le_bytes())?;
    f.write_all(&bits.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

impl Recorder {
    pub fn new(shared: Arc<RtShared>, sr: u32) -> Recorder {
        Recorder { shared, sr, active: Mutex::new(None), last_error: Mutex::new(String::new()) }
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }

    pub fn state(&self) -> RecState {
        let a = self.active.lock().unwrap();
        RecState {
            recording: a.is_some(),
            id: a.as_ref().map(|x| x.card.id.clone()).unwrap_or_default(),
            seconds: a.as_ref().map(|x| x.frames as f64 / self.sr as f64).unwrap_or(0.0),
            path: a.as_ref().map(|x| x.dir.display().to_string()).unwrap_or_default(),
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }

    pub fn start(&self, base: &Path, scene: &str) -> Result<String, String> {
        let mut a = self.active.lock().unwrap();
        if a.is_some() {
            return Err("already recording".into());
        }
        let now = timefmt::now_secs();
        let id = format!("pilab-rec-{}", timefmt::stamp(now));
        let dir = base.join(&id);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut file = File::create(dir.join(format!("{id}.wav"))).map_err(|e| e.to_string())?;
        wav_header(&mut file, self.sr, 0).map_err(|e| e.to_string())?;
        let card = RecordCard {
            id: id.clone(),
            created: timefmt::iso_local(now),
            created_unix: now,
            sample_rate: self.sr,
            channels: 2,
            format: "wav-f32le".into(),
            source: "Trillium Synth output (post master, limiter)".into(),
            scene: scene.to_string(),
            role: "master".into(),
            filed: false,
            rights: "LicenseRef-proprietary".into(),
            ..Default::default()
        };
        self.shared.rec_dropped.store(0, Ordering::Relaxed);
        *a = Some(Active { file, dir, card, frames: 0 });
        self.shared.rec_armed.store(true, Ordering::Release);
        *self.last_error.lock().unwrap() = String::new();
        Ok(id)
    }

    /// Drains the ring into the open file. Call often (every ~50 ms).
    pub fn drain(&self, rx: &mut rtrb::Consumer<f32>) {
        let mut a = self.active.lock().unwrap();
        let n = rx.slots();
        if n == 0 {
            return;
        }
        let Ok(chunk) = rx.read_chunk(n - n % 2) else { return };
        if let Some(act) = a.as_mut() {
            let (s1, s2) = chunk.as_slices();
            let mut buf = Vec::with_capacity((s1.len() + s2.len()) * 4);
            for v in s1.iter().chain(s2.iter()) {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            if let Err(e) = act.file.write_all(&buf) {
                *self.last_error.lock().unwrap() = format!("write failed: {e}");
            }
            act.frames += ((s1.len() + s2.len()) / 2) as u64;
        }
        chunk.commit_all();
    }

    /// Bytes written so far by the open take (0 if idle).
    pub fn bytes(&self) -> u64 {
        self.active.lock().unwrap().as_ref().map(|a| a.frames * 8).unwrap_or(0)
    }

    pub fn too_big(&self) -> bool {
        self.bytes() >= MAX_BYTES
    }

    pub fn stop(&self, rx: &mut rtrb::Consumer<f32>) -> Result<RecordCard, String> {
        self.shared.rec_armed.store(false, Ordering::Release);
        std::thread::sleep(std::time::Duration::from_millis(30));
        self.drain(rx);
        let mut a = self.active.lock().unwrap();
        let Some(mut act) = a.take() else { return Err("not recording".into()) };
        let data = (act.frames * 8).min(u32::MAX as u64 - 64) as u32;
        act.file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        wav_header(&mut act.file, self.sr, data).map_err(|e| e.to_string())?;
        act.file.flush().map_err(|e| e.to_string())?;
        act.card.duration_s = act.frames as f64 / self.sr as f64;
        act.card.bytes = data as u64 + 44;
        act.card.dropped_frames = self.shared.rec_dropped.load(Ordering::Relaxed);
        let body = toml::to_string_pretty(&act.card).map_err(|e| e.to_string())?;
        fs::write(act.dir.join("record.toml"), format!("# pilab recording card (Dublin Core-style catalog fields)\n{body}")).map_err(|e| e.to_string())?;
        Ok(act.card)
    }

    pub fn set_error(&self, e: String) {
        *self.last_error.lock().unwrap() = e;
    }
}

/// All record cards under `base`, newest first.
pub fn list(base: &Path) -> Vec<RecordCard> {
    let mut v = vec![];
    if let Ok(rd) = fs::read_dir(base) {
        for e in rd.flatten() {
            let p = e.path().join("record.toml");
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(c) = toml::from_str::<RecordCard>(&s) {
                    v.push(c);
                }
            }
        }
    }
    v.sort_by(|a, b| b.created_unix.cmp(&a.created_unix));
    v
}
