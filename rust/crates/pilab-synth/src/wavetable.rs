// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Mipmapped single-cycle wavetables (built from AKWF-style WAV cycles).
//!
//! A cycle of any length is analysed with a DFT, then resynthesised into
//! [`TABLE_LEN`]-sample tables that each keep only the harmonics that stay below
//! Nyquist for a frequency band. Built off the audio thread.

pub const TABLE_LEN: usize = 2048;

/// An ordered set of single-cycle frames (an AKWF group) scanned by a position control.
pub struct TableSet {
    pub name: String,
    pub frames: Vec<Wavetable>,
}

impl TableSet {
    /// Crossfades the two frames around `pos` (0..1). Allocation-free.
    #[inline]
    pub fn sample(&self, pos: f32, level: usize, phase: f32) -> f32 {
        let n = self.frames.len();
        if n == 0 {
            return 0.0;
        }
        let x = pos.clamp(0.0, 1.0) * (n - 1) as f32;
        let i = (x as usize).min(n - 1);
        let f = x - i as f32;
        let a = self.frames[i].sample(level, phase);
        if f <= 0.0 || i + 1 >= n {
            a
        } else {
            a + (self.frames[i + 1].sample(level, phase) - a) * f
        }
    }

    #[inline]
    pub fn level_for_dt(&self, dt: f32) -> usize {
        self.frames.first().map(|f| f.level_for_dt(dt)).unwrap_or(0)
    }
}

pub struct Wavetable {
    pub name: String,
    /// (max harmonic, samples with one guard sample), highest harmonic count first.
    levels: Vec<(u32, Box<[f32]>)>,
}

impl core::fmt::Debug for Wavetable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Wavetable({}, {} levels)", self.name, self.levels.len())
    }
}

impl Wavetable {
    /// Builds a mipmapped table from one cycle of samples (normalised to peak 1).
    pub fn from_cycle(name: &str, cycle: &[f32]) -> Result<Wavetable, String> {
        let n = cycle.len();
        if n < 8 {
            return Err("cycle too short".into());
        }
        let max_h = (n / 2).saturating_sub(1).clamp(1, TABLE_LEN / 2 - 1);
        // DFT, harmonics 1..=max_h (DC dropped).
        let mut re = vec![0f64; max_h + 1];
        let mut im = vec![0f64; max_h + 1];
        for k in 1..=max_h {
            let (mut a, mut b) = (0f64, 0f64);
            let w = core::f64::consts::TAU * k as f64 / n as f64;
            for (i, &x) in cycle.iter().enumerate() {
                let th = w * i as f64;
                a += x as f64 * th.cos();
                b -= x as f64 * th.sin();
            }
            re[k] = 2.0 * a / n as f64;
            im[k] = 2.0 * b / n as f64;
        }
        let mut limits = vec![max_h as u32];
        let mut p = 1u32 << (31 - (max_h as u32).leading_zeros());
        if p as usize == max_h {
            p >>= 1;
        }
        while p >= 1 {
            limits.push(p);
            if p == 1 {
                break;
            }
            p >>= 1;
        }
        Ok(Self::from_harmonics(name, &re, &im, &limits, true))
    }

    /// Builds tables from harmonic coefficients: sample(m) = sum_k re[k] cos + (-im[k]) sin,
    /// one table per entry of `limits` (descending top harmonic).
    pub fn from_harmonics(name: &str, re: &[f64], im: &[f64], limits: &[u32], normalise: bool) -> Wavetable {
        let mut cos_t = vec![0f64; TABLE_LEN];
        let mut sin_t = vec![0f64; TABLE_LEN];
        for i in 0..TABLE_LEN {
            let th = core::f64::consts::TAU * i as f64 / TABLE_LEN as f64;
            cos_t[i] = th.cos();
            sin_t[i] = th.sin();
        }
        let top = re.len().min(im.len()).saturating_sub(1);
        let mut levels = Vec::with_capacity(limits.len());
        let mut scale = 1.0f64;
        for (li, &h) in limits.iter().enumerate() {
            let h = (h as usize).min(top).min(TABLE_LEN / 2 - 1);
            let mut t = vec![0f32; TABLE_LEN + 1];
            for m in 0..TABLE_LEN {
                let mut y = 0f64;
                for k in 1..=h {
                    let idx = (k * m) % TABLE_LEN;
                    y += re[k] * cos_t[idx] - im[k] * sin_t[idx];
                }
                t[m] = y as f32;
            }
            if li == 0 && normalise {
                let peak = t.iter().fold(0f32, |a, &v| a.max(v.abs()));
                scale = if peak > 1e-9 { 1.0 / peak as f64 } else { 1.0 };
            }
            for v in t.iter_mut() {
                *v = (*v as f64 * scale) as f32;
            }
            t[TABLE_LEN] = t[0];
            levels.push((h as u32, t.into_boxed_slice()));
        }
        Wavetable { name: name.to_string(), levels }
    }

    pub fn from_wav(name: &str, bytes: &[u8]) -> Result<Wavetable, String> {
        let s = parse_wav(bytes)?;
        Self::from_cycle(name, &s)
    }

    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    pub fn level_harmonics(&self, level: usize) -> u32 {
        self.levels[level.min(self.levels.len() - 1)].0
    }

    /// Mip level for a phase increment (cycles/sample): the richest table whose top
    /// harmonic stays below Nyquist.
    #[inline]
    pub fn level_for_dt(&self, dt: f32) -> usize {
        let allowed = if dt > 0.0 { (0.5 / dt) as u32 } else { u32::MAX };
        for (i, (h, _)) in self.levels.iter().enumerate() {
            if *h <= allowed {
                return i;
            }
        }
        self.levels.len() - 1
    }

    /// Linear-interpolated sample at phase 0..1.
    #[inline]
    pub fn sample(&self, level: usize, phase: f32) -> f32 {
        let t = &self.levels[level.min(self.levels.len() - 1)].1;
        let x = phase * TABLE_LEN as f32;
        let i = (x as usize).min(TABLE_LEN - 1);
        let f = x - i as f32;
        t[i] + (t[i + 1] - t[i]) * f
    }

    /// Decimated full-bandwidth cycle for drawing.
    pub fn preview(&self, points: usize) -> Vec<f32> {
        let t = &self.levels[0].1;
        (0..points).map(|p| t[p * TABLE_LEN / points.max(1)]).collect()
    }
}

/// Third-octave mip limits from `top` harmonics down to 1.
pub fn third_octave_limits(top: u32) -> Vec<u32> {
    let mut v = vec![];
    let mut k = 0;
    loop {
        let h = (top as f64 * (-(k as f64) / 3.0).exp2()).floor() as u32;
        if h < 1 {
            break;
        }
        if v.last() != Some(&h) {
            v.push(h);
        }
        if h == 1 {
            break;
        }
        k += 1;
    }
    v
}

/// Analytic band-limited tables for the classic shapes (built once, shared).
pub struct Bands {
    /// 2ph-1 ramp (Fourier series), used for saw and, as a difference of two saws, for
    /// square and pulse.
    pub saw: Wavetable,
    /// 4|ph-0.5|-1 triangle.
    pub tri: Wavetable,
}

impl Bands {
    pub fn build() -> Bands {
        let top = (TABLE_LEN / 2 - 1) as u32;
        let n = top as usize + 1;
        let mut zero = vec![0f64; n];
        let mut saw_im = vec![0f64; n];
        let mut tri_re = vec![0f64; n];
        for k in 1..n {
            // 2ph-1 = -(2/pi) sum sin(2 pi k ph)/k ; our builder adds -im*sin.
            saw_im[k] = 2.0 / (core::f64::consts::PI * k as f64);
            if k % 2 == 1 {
                tri_re[k] = 8.0 / (core::f64::consts::PI.powi(2) * (k * k) as f64);
            }
        }
        let limits = third_octave_limits(top);
        let saw = Wavetable::from_harmonics("saw", &zero, &saw_im, &limits, false);
        zero.iter_mut().for_each(|v| *v = 0.0);
        let tri = Wavetable::from_harmonics("triangle", &tri_re, &zero, &limits, false);
        Bands { saw, tri }
    }

    /// The process-wide shared copy.
    pub fn shared() -> std::sync::Arc<Bands> {
        static B: std::sync::OnceLock<std::sync::Arc<Bands>> = std::sync::OnceLock::new();
        B.get_or_init(|| std::sync::Arc::new(Bands::build())).clone()
    }
}

/// Minimal RIFF/WAVE reader: PCM 8/16/24/32-bit or IEEE float 32/64, first channel.
pub fn parse_wav(b: &[u8]) -> Result<Vec<f32>, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12;
    let (mut fmt_tag, mut chans, mut bits) = (0u16, 0u16, 0u16);
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = &b[pos + 8..(pos + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            fmt_tag = u16::from_le_bytes([body[0], body[1]]);
            chans = u16::from_le_bytes([body[2], body[3]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
            if fmt_tag == 0xFFFE && body.len() >= 26 {
                fmt_tag = u16::from_le_bytes([body[24], body[25]]);
            }
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + len + (len & 1);
    }
    let d = data.ok_or("no data chunk")?;
    if chans == 0 {
        return Err("no fmt chunk".into());
    }
    let bps = (bits as usize).div_ceil(8);
    let frame = bps * chans as usize;
    if frame == 0 {
        return Err("bad format".into());
    }
    let mut out = Vec::with_capacity(d.len() / frame);
    for f in d.chunks_exact(frame) {
        let s = &f[..bps];
        let v = match (fmt_tag, bits) {
            (1, 8) => (s[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
            (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
            (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
            (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            (3, 64) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32,
            _ => return Err(format!("unsupported WAV format tag {fmt_tag}, {bits} bits")),
        };
        out.push(v);
    }
    Ok(out)
}
