// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Trillium Zero protocol codec, checked against replies captured from a device on
//! firmware 0.8.0 (USB-only build) and against the frame layout.

use pilab_core::zero::*;

fn hx(s: &str) -> Vec<u8> {
    parse_hex(s).unwrap()
}

#[test]
fn request_frames() {
    assert_eq!(to_hex(&hello(0x11)), "F0 7D 54 52 01 01 11 01 F7");
    assert_eq!(to_hex(&get_config(0x12, 0)), "F0 7D 54 52 01 02 12 F7");
    assert_eq!(to_hex(&get_config(0x14, 1)), "F0 7D 54 52 01 02 14 01 F7");
    assert_eq!(to_hex(&get_status(0x13)), "F0 7D 54 52 01 07 13 F7");
    assert_eq!(to_hex(&telemetry_start(0x16, 10)), "F0 7D 54 52 01 05 16 0A F7");
    assert_eq!(to_hex(&telemetry_stop(0x17)), "F0 7D 54 52 01 06 17 F7");
    assert_eq!(to_hex(&set_config(0x34, &[(11, 0)])), "F0 7D 54 52 01 03 34 0B 00 00 F7");
    assert_eq!(to_hex(&set_config(1, &[(9, 360)])), "F0 7D 54 52 01 03 01 09 02 68 F7");
    assert!(set_config(1, &[(1, 0); 20]).len() <= MAX_LEN);
}

#[test]
fn hello_reply() {
    let m = parse(&hx("F0 7D 54 52 01 41 11 01 00 08 00 2E 7B 40 29 00 03 4A 52 6D F7")).unwrap();
    assert_eq!(m.req, 0x11);
    let Reply::Hello(h) = m.reply else { panic!() };
    assert_eq!(h.firmware, "0.8.0");
    assert_eq!(h.params, 41);
    assert_eq!(h.uptime_s, 458);
    assert_eq!(h.boot_id, (0x52 << 7) | 0x6D);
    assert_eq!(h.pages(), vec![0, 1, 2]);
    assert!(h.imu() && h.mic() && h.has_status() && !h.ble() && !h.persistent());
}

#[test]
fn config_pages() {
    let p0 = parse(&hx("F0 7D 54 52 01 42 12 00 00 00 01 00 00 02 00 30 03 00 18 04 00 00 05 00 00 06 00 18 07 00 1E 08 00 01 09 02 68 0A 00 3C 0B 00 00 0C 00 09 0D 00 27 F7")).unwrap();
    let Reply::Config(c) = p0.reply else { panic!() };
    assert_eq!(c.generation, 0);
    assert!(!c.persistent);
    assert_eq!(c.params.len(), 13);
    assert_eq!(c.params[1], (2, 48));
    assert_eq!(c.params[8], (9, 360));
    assert_eq!(c.params[10], (11, 0));
    let p1 = parse(&hx("F0 7D 54 52 01 42 14 00 00 00 0E 00 00 0F 00 01 10 00 4A 11 00 02 12 00 01 13 00 00 14 00 00 15 00 2D 16 00 0F 17 00 14 18 00 00 19 00 50 1A 00 78 1B 00 00 1C 03 10 1D 02 2C 1E 00 3C F7")).unwrap();
    let Reply::Config(c1) = p1.reply else { panic!() };
    assert_eq!(c1.params.len(), 17);
    assert_eq!(c1.params[2], (16, 74));
    assert_eq!(c1.params[14], (28, 400));
}

#[test]
fn status_reply() {
    let m = parse(&hx("F0 7D 54 52 01 47 13 01 0A 1D 42 21 00 08 F7")).unwrap();
    let Reply::Status(s) = m.reply else { panic!() };
    assert_eq!(s.battery, "present");
    assert!(s.usb_power && !s.charging);
    assert_eq!(s.battery_mv, 3778);
    assert_eq!(s.charge_pct, Some(33));
    assert_eq!(s.link, "off");
    assert_eq!(s.bt_reason, "firmware without Bluetooth");
}

#[test]
fn telemetry_frame() {
    let m = parse(&hx("F0 7D 54 52 01 50 00 01 00 00 1D 12 73 0B 02 03 03 7D 3C 03 7D 45 00 07 26 00 00 14 00 00 01 00 00 05 00 0D 6A 03 71 48 00 00 00 00 64 75 00 0E 01 00 00 00 00 F7")).unwrap();
    let Reply::Telemetry(t) = m.reply else { panic!() };
    assert!(t.imu_valid && t.gyro_calibrated && !t.touching);
    assert_eq!(t.imu_state, 2);
    assert_eq!(t.trill_state, 3);
    assert_eq!(t.accel, [-324, -315, 934]);
    assert!((t.pitch - 17.70).abs() < 0.001);
    assert!((t.roll + 18.48).abs() < 0.001);
    assert!((t.gyro[0] - 2.0).abs() < 0.001);
    assert_eq!(t.taps, 14);
}

#[test]
fn acks_errors_and_foreign_sysex() {
    assert_eq!(parse(&hx("F0 7D 54 52 01 44 21 01 F7")).unwrap().reply, Reply::Ack { action: 1 });
    assert_eq!(parse(&hx("F0 7D 54 52 01 45 16 0A 05 36 01 F7")).unwrap().reply, Reply::TelemetryAck { rate: 10 });
    assert_eq!(parse(&hx("F0 7D 54 52 01 7F 05 04 0B F7")).unwrap().reply, Reply::Error { code: 4, detail: 11 });
    // A request (bit 6 clear), another manufacturer and a short frame are not replies.
    assert!(parse(&hx("F0 7D 54 52 01 01 11 01 F7")).is_none());
    assert!(parse(&hx("F0 43 10 4C 00 00 7E 00 F7")).is_none());
    assert!(parse(&hx("F0 7D 54 F7")).is_none());
}

#[test]
fn profile_diff_and_reapply_frames() {
    let stored = Profile::from_params("zero-1-1", "Trillium Zero", "1209:0003", "0.8.0", 1, &[(11, 0), (4, 2), (16, 74), (40, 1), (99, 5)]);
    let device = vec![(4, 0), (11, 1), (16, 74), (40, 0)];
    let d = diff(&stored, &device);
    // Id 99 is not on the device and is never sent.
    assert_eq!(d, vec![(4, 2), (11, 0), (40, 1)]);
    let frames = set_config_frames(0x20, &d);
    assert_eq!(frames.len(), 2);
    assert_eq!(to_hex(&frames[0]), "F0 7D 54 52 01 03 20 04 00 02 0B 00 00 F7");
    assert_eq!(to_hex(&frames[1]), "F0 7D 54 52 01 03 21 28 00 01 F7");
    let t = stored.to_toml().unwrap();
    assert_eq!(Profile::from_toml(&t).unwrap(), stored);
    assert!(diff(&stored, &[(4, 2), (11, 0), (16, 74), (40, 1)]).is_empty());
}

#[test]
fn start_and_stop_mic_actions() {
    let on = pilab_core::zero::action(0x21, pilab_core::zero::ACTION_START_MIC);
    assert_eq!(on, [0xF0, 0x7D, 0x54, 0x52, 0x01, 0x04, 0x21, 0x07, 0xF7]);
    let off = pilab_core::zero::action(0x22, pilab_core::zero::ACTION_STOP_MIC);
    assert_eq!(off, [0xF0, 0x7D, 0x54, 0x52, 0x01, 0x04, 0x22, 0x08, 0xF7]);
    let p = pilab_core::zero::Profile::default();
    assert!(!p.mic);
}
