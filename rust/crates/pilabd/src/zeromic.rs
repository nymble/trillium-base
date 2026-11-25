// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Brings the Trillium Zero 16 kHz PCM test stream into PipeWire as a capture
//! source named "Trillium Zero Mic", resampled to 48 kHz. Not auto-routed to the
//! DAC. The process callback only reads a pre-filled ring; serial I/O stays off
//! the audio thread and off CPU 3.

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use pilab_core::pcm::{self, DEVICE_HZ};

/// JACK / PipeWire client name (appears as a source on the patch panel).
pub const CLIENT_NAME: &str = "Trillium Zero Mic";
/// Default device node (udev symlink).
pub const DEVICE: &str = "/dev/trillium-zero-pcm";
const RING: usize = 48_000; // 1 s at 48 kHz

/// Running capture into PipeWire.
pub struct Bridge {
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    reader: Option<JoinHandle<()>>,
    // Held so the JACK client stays active.
    _client: Option<jack::AsyncClient<(), Proc>>,
}

struct Proc {
    rx: rtrb::Consumer<f32>,
    dropped: Arc<AtomicU64>,
    ports: Vec<jack::Port<jack::AudioOut>>,
    scratch: [f32; 2048],
}

impl jack::ProcessHandler for Proc {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let n = ps.n_frames() as usize;
        let avail = self.rx.slots();
        if avail < n || n > self.scratch.len() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            for p in &mut self.ports {
                p.as_mut_slice(ps).fill(0.0);
            }
            return jack::Control::Continue;
        }
        for s in self.scratch[..n].iter_mut() {
            *s = self.rx.pop().unwrap_or(0.0);
        }
        for p in &mut self.ports {
            p.as_mut_slice(ps)[..n].copy_from_slice(&self.scratch[..n]);
        }
        jack::Control::Continue
    }
}

impl Bridge {
    /// Opens `device` (raw CDC ACM) and publishes stereo 48 kHz ports.
    pub fn start(device: &Path) -> Result<Bridge, String> {
        let path = device.to_path_buf();
        if !path.exists() {
            return Err(format!("{} is not present (is the Zero plugged in?)", path.display()));
        }
        let (client, _st) = jack::Client::new(CLIENT_NAME, jack::ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("PipeWire client: {e:?}"))?;
        if client.sample_rate() as u32 != pcm::HOST_HZ {
            return Err(format!("graph is {} Hz, expected {}", client.sample_rate(), pcm::HOST_HZ));
        }
        let fl = client.register_port("capture_FL", jack::AudioOut::default()).map_err(|e| format!("{e:?}"))?;
        let fr = client.register_port("capture_FR", jack::AudioOut::default()).map_err(|e| format!("{e:?}"))?;
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(RING);
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let active = client
            .activate_async((), Proc { rx, dropped: dropped.clone(), ports: vec![fl, fr], scratch: [0.0; 2048] })
            .map_err(|e| format!("activate: {e:?}"))?;
        let stop_r = stop.clone();
        let reader = std::thread::Builder::new()
            .name("zero-mic".into())
            .spawn(move || read_loop(path, &mut tx, &stop_r))
            .map_err(|e| e.to_string())?;
        Ok(Bridge { stop, dropped, reader: Some(reader), _client: Some(active) })
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(c) = self._client.take() {
            let _ = c.deactivate();
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_loop(path: PathBuf, tx: &mut rtrb::Producer<f32>, stop: &AtomicBool) {
    let mut file = loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match OpenOptions::new().read(true).custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK).open(&path) {
            Ok(f) => break f,
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    // Prefer blocking reads once we have the fd.
    let _ = unsafe {
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
        let mut fl = libc::fcntl(fd, libc::F_GETFL, 0);
        if fl >= 0 {
            fl &= !libc::O_NONBLOCK;
            libc::fcntl(fd, libc::F_SETFL, fl);
        }
    };
    let mut buf = [0u8; 1024];
    let mut parser = pcm::Parser::new();
    let mut prev = 0i16;
    while !stop.load(Ordering::Relaxed) {
        match file.read(&mut buf) {
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(n) => {
                for frame in parser.push(&buf[..n]) {
                    if frame.sample_rate != 0 && frame.sample_rate != DEVICE_HZ {
                        continue;
                    }
                    let up = pcm::upsample_16k_to_48k(&frame.samples, &mut prev);
                    for s in up {
                        if tx.push(s).is_err() {
                            // Ring full: drop this sample; the process side counts underruns separately.
                            break;
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
                if let Ok(f) = OpenOptions::new().read(true).custom_flags(libc::O_NOCTTY).open(&path) {
                    file = f;
                }
            }
        }
    }
}
