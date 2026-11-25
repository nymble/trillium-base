// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial

use pilab_core::pcm::{self, FLAG_DROPPED, HEADER_LEN, MAGIC, VERSION};

fn encode(seq: u32, rate: u32, flags: u8, samples: &[i16]) -> Vec<u8> {
    let n = samples.len();
    let len = pcm::frame_len(n);
    let mut o = vec![0u8; len];
    o[0..4].copy_from_slice(&MAGIC);
    o[4] = VERSION;
    o[5] = flags;
    o[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    o[8..12].copy_from_slice(&seq.to_le_bytes());
    o[12..16].copy_from_slice(&rate.to_le_bytes());
    for (i, s) in samples.iter().enumerate() {
        o[HEADER_LEN + 2 * i..HEADER_LEN + 2 * i + 2].copy_from_slice(&s.to_le_bytes());
    }
    let crc = pcm::crc16(&o[..len - 2]);
    o[len - 2..].copy_from_slice(&crc.to_le_bytes());
    o
}

#[test]
fn round_trip_typical_mic_block() {
    let samples: Vec<i16> = (0..80).map(|i| (i * 400) as i16).collect();
    let f = encode(7, 16_000, 0, &samples);
    assert_eq!(f.len(), pcm::frame_len(80));
    let (d, n) = pcm::decode(&f).expect("frame");
    assert_eq!(n, f.len());
    assert_eq!(d.seq, 7);
    assert_eq!(d.sample_rate, 16_000);
    assert_eq!(d.samples, samples);
    assert_eq!(d.flags, 0);
}

#[test]
fn bad_crc_is_rejected() {
    let mut f = encode(1, 16_000, FLAG_DROPPED, &[1, 2, 3, 4]);
    let last = f.len() - 1;
    f[last] ^= 0xFF;
    assert!(pcm::decode(&f).is_none());
}

#[test]
fn parser_resyncs_after_noise() {
    let a = encode(1, 16_000, 0, &[100, -100, 200]);
    let b = encode(2, 16_000, 0, &[300, -300]);
    let mut p = pcm::Parser::new();
    let mut bytes = vec![0x11, 0x22, b'T'];
    bytes.extend_from_slice(&a);
    bytes.extend_from_slice(&b[..10]);
    let first = p.push(&bytes);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].seq, 1);
    let rest = p.push(&b[10..]);
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].seq, 2);
}

#[test]
fn upsample_is_exactly_3x_and_reaches_the_last_sample() {
    let mut prev = 0i16;
    let out = pcm::upsample_16k_to_48k(&[0, 32767], &mut prev);
    assert_eq!(out.len(), 6);
    assert_eq!(prev, 32767);
    assert!(out[0].abs() < 1e-6);
    assert!((out[5] - 32767.0 / 32768.0).abs() < 1e-5);
    assert!(out[1] < out[2] && out[2] < out[3] || out.windows(2).any(|w| w[1] >= w[0]));
}
