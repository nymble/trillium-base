// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Framed 16 kHz PCM test stream from a Trillium Zero (`TRPC` on the USB PCM port).
//!
//! This is a diagnostic CDC ACM transport, not USB Audio Class. Frame layout
//! (little endian), as used by firmware 0.8.0:
//!
//! | off | size | field |
//! |-----|------|-------|
//! | 0 | 4 | magic `TRPC` |
//! | 4 | 1 | version (1) |
//! | 5 | 1 | flags: bit 0 = a frame was dropped before this one |
//! | 6 | 2 | sample count N |
//! | 8 | 4 | sequence number |
//! | 12 | 4 | sample rate (Hz) |
//! | 16 | 2N | N × i16 mono samples |
//! | 16+2N | 2 | CRC-16/CCITT-FALSE over the earlier bytes |
//!
//! Typical blocks are 80 samples (5 ms at 16 kHz). 16 kHz triples exactly to 48 kHz.

/// Frame magic.
pub const MAGIC: [u8; 4] = *b"TRPC";
/// Format version.
pub const VERSION: u8 = 1;
/// Header length in bytes.
pub const HEADER_LEN: usize = 16;
/// Flag: the producer dropped at least one frame before this one.
pub const FLAG_DROPPED: u8 = 1;
/// Device capture rate.
pub const DEVICE_HZ: u32 = 16_000;
/// Host graph rate.
pub const HOST_HZ: u32 = 48_000;
/// Integer upsample factor (16000 × 3 = 48000).
pub const UPSAMPLE: usize = 3;

/// Encoded length for `n` samples.
pub const fn frame_len(n: usize) -> usize {
    HEADER_LEN + 2 * n + 2
}

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, no reflection).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// A validated frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub flags: u8,
    pub seq: u32,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

/// Decode one frame at the start of `data`. Returns the frame and its length, or
/// `None` if the buffer is short, the header is not a frame, or the CRC is wrong.
pub fn decode(data: &[u8]) -> Option<(Frame, usize)> {
    if data.len() < HEADER_LEN + 2 || data[..4] != MAGIC || data[4] != VERSION {
        return None;
    }
    let flags = data[5];
    let n = u16::from_le_bytes([data[6], data[7]]) as usize;
    let len = frame_len(n);
    if data.len() < len {
        return None;
    }
    let expect = u16::from_le_bytes([data[len - 2], data[len - 1]]);
    if crc16(&data[..len - 2]) != expect {
        return None;
    }
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let o = HEADER_LEN + 2 * i;
        samples.push(i16::from_le_bytes([data[o], data[o + 1]]));
    }
    Some((
        Frame {
            flags,
            seq: u32::from_le_bytes(data[8..12].try_into().ok()?),
            sample_rate: u32::from_le_bytes(data[12..16].try_into().ok()?),
            samples,
        },
        len,
    ))
}

/// Reassemble a byte stream into frames. Bytes that cannot start a frame are
/// skipped one at a time so a lost packet can resynchronise on the next `TRPC`.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
}

impl Parser {
    pub fn new() -> Parser {
        Parser { buf: Vec::with_capacity(512) }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut out = vec![];
        let mut i = 0;
        while i + HEADER_LEN + 2 <= self.buf.len() {
            if self.buf[i..i + 4] != MAGIC {
                i += 1;
                continue;
            }
            let n = u16::from_le_bytes([self.buf[i + 6], self.buf[i + 7]]) as usize;
            let need = frame_len(n);
            if i + need > self.buf.len() {
                break;
            }
            match decode(&self.buf[i..]) {
                Some((f, len)) => {
                    out.push(f);
                    i += len;
                }
                None => i += 1,
            }
        }
        if i > 0 {
            self.buf.drain(..i);
        }
        // Bound a desynchronised buffer.
        if self.buf.len() > 8192 {
            self.buf.clear();
        }
        out
    }
}

/// Triple 16 kHz mono to 48 kHz with linear interpolation. `prev` is the last
/// 16 kHz sample from the previous block (0 on the first block); it is updated.
pub fn upsample_16k_to_48k(samples: &[i16], prev: &mut i16) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() * UPSAMPLE);
    let mut a = *prev as f32 / 32768.0;
    for &s in samples {
        let b = s as f32 / 32768.0;
        out.push((2.0 * a + b) / 3.0);
        out.push((a + 2.0 * b) / 3.0);
        out.push(b);
        a = b;
    }
    if let Some(&last) = samples.last() {
        *prev = last;
    }
    out
}
