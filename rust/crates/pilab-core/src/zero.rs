// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Trillium Zero configuration protocol, host side: request frames, reply decoding,
//! stored profiles and the difference between a profile and the device.
//!
//! The device speaks MIDI System Exclusive on its second USB MIDI port:
//! `F0 7D 54 52 <version 01> <type> <request id> <payload> F7`, at most 64 bytes, all
//! data 7-bit, 14-bit values as (high 7, low 7). Replies set bit 6 of the type.
//! This module only encodes and decodes; sending is the caller's job. Nothing here
//! writes to a device unless the caller sends a frame built by [`set_config`] or [`action`].

use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;
pub const MAX_LEN: usize = 64;
const HEAD: [u8; 4] = [0xF0, 0x7D, 0x54, 0x52];

pub const T_HELLO: u8 = 0x01;
pub const T_GET_CONFIG: u8 = 0x02;
pub const T_SET_CONFIG: u8 = 0x03;
pub const T_ACTION: u8 = 0x04;
pub const T_TELEMETRY_START: u8 = 0x05;
pub const T_TELEMETRY_STOP: u8 = 0x06;
pub const T_GET_STATUS: u8 = 0x07;

/// Capability bits in the HELLO reply.
pub const CAP_SET_CONFIG: u8 = 0x01;
pub const CAP_ACTION: u8 = 0x02;
pub const CAP_PERSISTENT: u8 = 0x04;
pub const CAP_TELEMETRY: u8 = 0x08;
pub const CAP_PAGED: u8 = 0x10;
pub const CAP_PAGE2: u8 = 0x20;
pub const CAP_BOARD: u8 = 0x40;

fn frame(ty: u8, req: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(&HEAD);
    v.push(VERSION);
    v.push(ty & 0x7F);
    v.push(req & 0x7F);
    v.extend(payload.iter().map(|b| b & 0x7F));
    v.push(0xF7);
    v
}

pub fn hello(req: u8) -> Vec<u8> {
    frame(T_HELLO, req, &[VERSION])
}
/// Page 0 is sent with no payload, which every firmware accepts.
pub fn get_config(req: u8, page: u8) -> Vec<u8> {
    if page == 0 {
        frame(T_GET_CONFIG, req, &[])
    } else {
        frame(T_GET_CONFIG, req, &[page])
    }
}
pub fn get_status(req: u8) -> Vec<u8> {
    frame(T_GET_STATUS, req, &[])
}
pub fn telemetry_start(req: u8, hz: u8) -> Vec<u8> {
    frame(T_TELEMETRY_START, req, &[hz.clamp(1, 100)])
}
pub fn telemetry_stop(req: u8) -> Vec<u8> {
    frame(T_TELEMETRY_STOP, req, &[])
}

/// ACTION codes (firmware 0.8.0). 6 enters the bootloader and is never sent by this crate's helpers.
pub const ACTION_START_MIC: u8 = 7;
pub const ACTION_STOP_MIC: u8 = 8;

/// One ACTION frame. This can change device state (microphone power, and other codes).
pub fn action(req: u8, code: u8) -> Vec<u8> {
    frame(T_ACTION, req, &[code])
}
/// One SET_CONFIG frame (1..=16 parameters). This changes the device's settings.
pub fn set_config(req: u8, params: &[(u8, u16)]) -> Vec<u8> {
    let mut p = Vec::with_capacity(params.len() * 3);
    for &(id, v) in params.iter().take(16) {
        p.extend_from_slice(&[id, (v >> 7) as u8 & 0x7F, v as u8 & 0x7F]);
    }
    frame(T_SET_CONFIG, req, &p)
}

fn u14(hi: u8, lo: u8) -> u16 {
    ((hi as u16 & 0x7F) << 7) | (lo as u16 & 0x7F)
}
fn u21(a: u8, b: u8, c: u8) -> u32 {
    ((a as u32 & 0x7F) << 14) | ((b as u32 & 0x7F) << 7) | (c as u32 & 0x7F)
}
/// Signed 16-bit value sent as three 7-bit groups (bits 15–14, 13–7, 6–0).
fn i16_3(a: u8, b: u8, c: u8) -> i16 {
    (((a as u16 & 0x03) << 14) | ((b as u16 & 0x7F) << 7) | (c as u16 & 0x7F)) as i16
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u8,
    pub firmware: String,
    pub variant: u8,
    pub caps: u8,
    pub max_len: u8,
    pub params: u8,
    pub uptime_s: u32,
    pub boot_id: u16,
}

impl Hello {
    pub fn pages(&self) -> Vec<u8> {
        let mut v = vec![0];
        if self.caps & CAP_PAGED != 0 {
            v.push(1);
        }
        if self.caps & CAP_PAGE2 != 0 {
            v.push(2);
        }
        v
    }
    pub fn imu(&self) -> bool {
        self.caps & CAP_BOARD == 0 || self.variant & 0x02 != 0
    }
    pub fn mic(&self) -> bool {
        self.caps & CAP_BOARD != 0 && self.variant & 0x04 != 0
    }
    pub fn ble(&self) -> bool {
        self.variant & 0x10 != 0
    }
    pub fn has_status(&self) -> bool {
        self.variant & 0x20 != 0
    }
    pub fn persistent(&self) -> bool {
        self.caps & CAP_PERSISTENT != 0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub generation: u16,
    pub persistent: bool,
    pub params: Vec<(u8, u16)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    /// "unknown" | "absent" | "present"
    pub battery: &'static str,
    pub charging: bool,
    pub usb_power: bool,
    pub battery_mv: u16,
    /// 0–100, None when unknown.
    pub charge_pct: Option<u8>,
    pub discoverable: bool,
    pub ble_midi: bool,
    /// "off" | "advertising" | "connected" | "streaming"
    pub link: &'static str,
    pub bt_reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub seq: u16,
    pub time_ms: u32,
    pub imu_valid: bool,
    pub gyro_calibrated: bool,
    pub touching: bool,
    pub safe_mode: bool,
    pub imu_state: u8,
    pub trill_state: u8,
    /// mg
    pub accel: [i16; 3],
    /// °/s
    pub gyro: [f32; 3],
    /// degrees
    pub pitch: f32,
    pub roll: f32,
    /// 0..1
    pub touch_pos: f32,
    pub touch_size: f32,
    pub taps: u16,
    pub skipped: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Hello(Hello),
    Config(Config),
    Ack { action: u8 },
    TelemetryAck { rate: u8 },
    Status(Status),
    Telemetry(Box<Telemetry>),
    Error { code: u8, detail: u8 },
    Other { ty: u8 },
}

/// A decoded reply with its request id.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub req: u8,
    pub reply: Reply,
}

pub fn error_name(code: u8) -> &'static str {
    match code {
        1 => "BadVersion",
        2 => "UnknownType",
        3 => "BadLength",
        4 => "UnknownParam",
        5 => "OutOfRange",
        6 => "TooLong",
        7 => "UnknownAction",
        8 => "DuplicateParam",
        _ => "Unknown",
    }
}

/// Decodes one complete SysEx frame. Returns None for anything that is not a reply of
/// this protocol (other SysEx, requests, wrong version, malformed).
pub fn parse(b: &[u8]) -> Option<Message> {
    if b.len() < 8 || b[..4] != HEAD || *b.last()? != 0xF7 || b[4] != VERSION {
        return None;
    }
    let ty = b[5];
    if ty & 0x40 == 0 {
        return None;
    }
    let req = b[6];
    let p = &b[7..b.len() - 1];
    let reply = match ty {
        0x41 if p.len() >= 13 => Reply::Hello(Hello {
            version: p[0],
            firmware: format!("{}.{}.{}", p[1], p[2], p[3]),
            variant: p[4],
            caps: p[5],
            max_len: p[6],
            params: p[7],
            uptime_s: u21(p[8], p[9], p[10]),
            boot_id: u14(p[11], p[12]),
        }),
        0x42 if p.len() >= 3 && (p.len() - 3) % 3 == 0 => Reply::Config(Config {
            generation: u14(p[0], p[1]),
            persistent: p[2] != 0,
            params: p[3..].chunks(3).map(|c| (c[0], u14(c[1], c[2]))).collect(),
        }),
        0x44 if !p.is_empty() => Reply::Ack { action: p[0] },
        0x45 if !p.is_empty() => Reply::TelemetryAck { rate: p[0] },
        0x47 if p.len() >= 7 && p[0] == 1 => Reply::Status(Status {
            battery: match p[1] & 0x03 {
                1 => "absent",
                2 => "present",
                _ => "unknown",
            },
            charging: p[1] & 0x04 != 0,
            usb_power: p[1] & 0x08 != 0,
            battery_mv: u14(p[2], p[3]),
            charge_pct: (p[4] <= 100).then_some(p[4]),
            discoverable: p[5] & 0x01 != 0,
            ble_midi: p[5] & 0x02 != 0,
            link: match p[6] & 0x07 {
                1 => "advertising",
                2 => "connected",
                3 => "streaming",
                _ => "off",
            },
            bt_reason: match (p[6] >> 3) & 0x07 {
                0 => "running",
                1 => "firmware without Bluetooth",
                2 => "no battery",
                3 => "turned off",
                4 => "Bluetooth stack unavailable",
                5 => "checking for a battery",
                _ => "unknown",
            },
        }),
        0x50 if p.len() >= 46 && p[0] == 1 => {
            let fl = p[6];
            Reply::Telemetry(Box::new(Telemetry {
                seq: u14(p[1], p[2]),
                time_ms: u21(p[3], p[4], p[5]),
                imu_valid: fl & 1 != 0,
                gyro_calibrated: fl & 2 != 0,
                touching: fl & 4 != 0,
                safe_mode: fl & 16 != 0,
                imu_state: p[7],
                trill_state: p[8],
                accel: [i16_3(p[9], p[10], p[11]), i16_3(p[12], p[13], p[14]), i16_3(p[15], p[16], p[17])],
                gyro: [
                    i16_3(p[18], p[19], p[20]) as f32 / 10.0,
                    i16_3(p[21], p[22], p[23]) as f32 / 10.0,
                    i16_3(p[24], p[25], p[26]) as f32 / 10.0,
                ],
                pitch: i16_3(p[27], p[28], p[29]) as f32 / 100.0,
                roll: i16_3(p[30], p[31], p[32]) as f32 / 100.0,
                touch_pos: u14(p[33], p[34]) as f32 / 16383.0,
                touch_size: u14(p[35], p[36]) as f32 / 16383.0,
                taps: u14(p[39], p[40]),
                skipped: u14(p[42], p[43]),
            }))
        }
        0x7F if p.len() >= 2 => Reply::Error { code: p[0], detail: p[1] },
        _ => Reply::Other { ty },
    };
    Some(Message { req, reply })
}

/// Page that holds a parameter id (pages are fixed by the protocol).
pub fn page_of(id: u8) -> u8 {
    match id {
        0..=13 => 0,
        14..=30 => 1,
        _ => 2,
    }
}

/// Parameter names for display, by id.
pub fn param_name(id: u8) -> &'static str {
    match id {
        1 => "Touch voice channel",
        2 => "Base note",
        3 => "Range (semitones)",
        4 => "Scale",
        5 => "Pitch mode (bend / steps)",
        6 => "Pitch-bend range",
        7 => "Velocity at onset",
        8 => "Motion controllers",
        9 => "Rotation for CC = 127 (°/s)",
        10 => "Forward tilt for CC = 127 (°)",
        11 => "Tap percussion",
        12 => "Percussion channel",
        13 => "Percussion note",
        14 => "Mode",
        15 => "Controller A source",
        16 => "Controller A CC",
        17 => "Controller B source",
        18 => "Controller B CC",
        19 => "Controller deadzone (%)",
        20 => "Controller smoothing (ms)",
        21 => "Tilt roll range (°)",
        22 => "Tilt gate angle (°)",
        23 => "Step hysteresis (%)",
        24 => "Tilt pressure CC",
        25 => "Tap threshold (centi-g)",
        26 => "Tap spacing (ms)",
        27 => "Flick axis",
        28 => "Flick threshold (°/s)",
        29 => "Flick spacing (ms)",
        30 => "Flick base note",
        31 => "Strike directions",
        32 => "Strike down note",
        33 => "Strike up note",
        34 => "Strike left note",
        35 => "Strike right note",
        36 => "Strike forward note",
        37 => "Strike back note",
        38 => "Tilt + strike note length (ms)",
        39 => "Tilt + strike octave shift",
        40 => "Tilt + strike finer zones",
        41 => "Tilt + strike chord tone",
        _ => "",
    }
}

/// One stored parameter value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub id: u8,
    pub value: u16,
}

/// Settings snapshot of one device, kept on the Base (the firmware keeps settings in
/// RAM only, so a replug restores the firmware defaults).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub key: String,
    pub label: String,
    pub usb_id: String,
    pub firmware: String,
    /// Unix seconds when the snapshot was read.
    pub captured: u64,
    pub params: Vec<Param>,
    /// Start the onboard 16 kHz microphone test stream when this profile is applied
    /// (ACTION 7). Off at firmware boot; stored here so a reconnect can turn it back on.
    pub mic: bool,
    /// Do not send this profile automatically when the device connects (default: the
    /// Base re-applies it, since the firmware forgets its settings on every power cycle).
    pub manual: bool,
    /// Name of the mapping preset this profile came from, if any ("" = custom).
    pub preset: String,
}

impl Profile {
    pub fn from_params(key: &str, label: &str, usb_id: &str, firmware: &str, captured: u64, params: &[(u8, u16)]) -> Profile {
        let mut p: Vec<Param> = params.iter().map(|&(id, value)| Param { id, value }).collect();
        p.sort_by_key(|x| x.id);
        p.dedup_by_key(|x| x.id);
        Profile { key: key.into(), label: label.into(), usb_id: usb_id.into(), firmware: firmware.into(), captured, params: p, mic: false, manual: false, preset: String::new() }
    }
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }
    pub fn from_toml(s: &str) -> Result<Profile, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }
}

/// Parameters whose stored value differs from the device. Ids the device does not
/// report (older firmware) are skipped, never sent.
pub fn diff(profile: &Profile, device: &[(u8, u16)]) -> Vec<(u8, u16)> {
    profile
        .params
        .iter()
        .filter_map(|p| device.iter().find(|(id, _)| *id == p.id).and_then(|&(_, v)| (v != p.value).then_some((p.id, p.value))))
        .collect()
}

/// SET_CONFIG frames for a change list: one page per frame (so each reply covers what
/// it changed), at most 16 parameters each. Request ids start at `req`.
pub fn set_config_frames(req: u8, changes: &[(u8, u16)]) -> Vec<Vec<u8>> {
    let mut out = vec![];
    let mut r = req;
    for page in 0..=2u8 {
        let on_page: Vec<(u8, u16)> = changes.iter().copied().filter(|(id, _)| page_of(*id) == page).collect();
        for chunk in on_page.chunks(16) {
            out.push(set_config(r, chunk));
            r = (r + 1) & 0x7F;
        }
    }
    out
}

/// Parses hex bytes ("F0 7D ... F7") as printed by ALSA tools.
pub fn parse_hex(s: &str) -> Option<Vec<u8>> {
    s.split_whitespace().map(|h| u8::from_str_radix(h, 16).ok()).collect()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

/// One row of a Zero's sensor inventory.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sensor {
    pub id: &'static str,
    pub label: &'static str,
    /// "running" | "on" | "present" (all fine), "off" | "not_fitted" | "absent" | "unavailable"
    /// | "checking" (neutral), "lost" | "disabled" (needs a look).
    pub state: &'static str,
    pub detail: String,
}

/// Telemetry frames needed before a touch strip that never reported is called "not fitted".
pub const TRILL_NOT_FITTED_AFTER: usize = 20;

/// Sensor inventory derived from HELLO, status and the live telemetry ring. A touch strip
/// that stays in the firmware's retry state is "not fitted": many Zeros are MCU-only and
/// that is normal, not a fault. It is a fault ("lost") only if it ran earlier this session.
pub fn sensors(hello: Option<&Hello>, status: Option<&Status>, frames: &[Telemetry], mic: bool, trill_seen_running: bool) -> Vec<Sensor> {
    let last = frames.last();
    let imu = {
        let (state, detail) = match (hello, last) {
            (Some(h), _) if !h.imu() => ("not_fitted", "No motion sensor on this board.".to_string()),
            (_, None) => ("checking", "Waiting for sensor frames.".to_string()),
            (_, Some(t)) => match t.imu_state {
                2 if t.gyro_calibrated => ("running", "Accelerometer and gyro running, calibrated (taps and strikes armed after about 1 s still).".to_string()),
                2 => ("running", "Running; the gyro is still calibrating (hold the Zero still).".to_string()),
                1 | 0 => ("checking", "Starting.".to_string()),
                4 => ("disabled", "Disabled (safe mode).".to_string()),
                5 => ("not_fitted", "No motion sensor answered at boot.".to_string()),
                _ => ("lost", "Not responding; the firmware is retrying.".to_string()),
            },
        };
        Sensor { id: "imu", label: "Motion (IMU)", state, detail }
    };
    let trill = {
        let (state, detail) = match last {
            None => ("checking", "Waiting for sensor frames.".to_string()),
            Some(t) => match t.trill_state {
                2 => ("running", "Touch strip running.".to_string()),
                1 => ("checking", "Starting.".to_string()),
                4 => ("disabled", "Disabled (safe mode).".to_string()),
                5 => ("not_fitted", "Not fitted. Notes come from the motion sensors.".to_string()),
                _ if trill_seen_running => ("lost", "The touch strip ran earlier and stopped answering; check its cable.".to_string()),
                _ if frames.len() >= TRILL_NOT_FITTED_AFTER => ("not_fitted", "No touch strip detected. Normal for a motion-only Zero; notes come from the motion sensors.".to_string()),
                _ => ("checking", "Looking for a touch strip.".to_string()),
            },
        };
        Sensor { id: "trill", label: "Touch strip (Trill)", state, detail }
    };
    let micr = match hello {
        Some(h) if h.mic() => Sensor { id: "mic", label: "Microphone", state: if mic { "on" } else { "off" }, detail: if mic { "16 kHz test stream running.".into() } else { "Test stream off (it starts from the stored profile).".into() } },
        _ => Sensor { id: "mic", label: "Microphone", state: "unavailable", detail: "This firmware has no microphone stream.".into() },
    };
    let batt = match status {
        Some(s) if s.battery == "present" => Sensor { id: "battery", label: "Battery", state: "present", detail: format!("{}% · {} mV{}", s.charge_pct.map(|p| p.to_string()).unwrap_or_else(|| "?".into()), s.battery_mv, if s.charging { " · charging" } else { "" }) },
        Some(s) if s.battery == "absent" => Sensor { id: "battery", label: "Battery", state: "absent", detail: format!("No pack reading (switch off or unplugged); running from USB, {} mV.", s.battery_mv) },
        _ => Sensor { id: "battery", label: "Battery", state: "checking", detail: "No status yet.".into() },
    };
    vec![imu, trill, micr, batt]
}

/// Which sensor path can produce notes with the current settings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NotePath {
    /// A strike (touch, tap or tilt-strike) can play a note now.
    pub strike_ready: bool,
    pub summary: String,
    pub hint: String,
    /// Channel (0-based) and note a strike would play, for a host-side rehearsal.
    pub probe: [u8; 2],
}

fn param(params: &[(u8, u16)], id: u8) -> u16 {
    params.iter().find(|(i, _)| *i == id).map(|x| x.1).unwrap_or(0)
}

/// Note sources for the live settings, given which sensors are running.
pub fn note_path(params: &[(u8, u16)], trill_running: bool, imu_running: bool) -> NotePath {
    let mode = param(params, 14);
    let ch = param(params, 12) + 1;
    let perc = param(params, 11) == 1;
    let dirs = param(params, 31);
    let mut parts: Vec<String> = vec![];
    let mut strike = false;
    match mode {
        0 => {
            if trill_running {
                parts.push(format!("touch strip → notes on channel {}", param(params, 1) + 1));
                strike = true;
            }
            if perc && imu_running {
                if dirs == 0 {
                    parts.push(format!("IMU tap → note {} on channel {ch}", param(params, 13)));
                } else {
                    parts.push(format!("IMU strike ({dirs} directions) → notes on channel {ch}"));
                }
                strike = true;
            }
        }
        1 | 2 if imu_running => parts.push("tilt → notes (continuous, not strikes)".into()),
        3 if imu_running => {
            parts.push("IMU strike → the tilt-selected note".into());
            strike = true;
        }
        _ => {}
    }
    if parts.is_empty() {
        let hint = if mode == 0 && !trill_running {
            "No touch strip, and tap percussion (parameter 11) is off, so a strike plays nothing. Turn tap percussion on, or use tilt-strike mode (parameter 14 = 3).".to_string()
        } else if !imu_running {
            "The motion sensor is not running.".to_string()
        } else {
            "Nothing is mapped to notes.".to_string()
        };
        NotePath { strike_ready: false, summary: "no note source enabled".into(), hint, probe: [0, 60] }
    } else {
        let probe = if perc && imu_running && mode == 0 {
            [(param(params, 12) & 15) as u8, (if dirs == 0 { param(params, 13) } else { param(params, 32) } & 127) as u8]
        } else if trill_running && mode == 0 {
            [(param(params, 1) & 15) as u8, (param(params, 3) & 127) as u8]
        } else {
            [0, 60]
        };
        NotePath { strike_ready: strike, summary: parts.join("; "), hint: String::new(), probe }
    }
}

#[cfg(test)]
mod sensor_tests {
    use super::*;

    fn frame(imu: u8, trill: u8) -> Telemetry {
        Telemetry { seq: 0, time_ms: 0, imu_valid: true, gyro_calibrated: true, touching: false, safe_mode: false, imu_state: imu, trill_state: trill, accel: [0; 3], gyro: [0.0; 3], pitch: 0.0, roll: 0.0, touch_pos: 0.0, touch_size: 0.0, taps: 0, skipped: 0 }
    }
    fn st(v: &[Sensor], id: &str) -> &'static str {
        v.iter().find(|s| s.id == id).unwrap().state
    }

    #[test]
    fn trill_in_retry_is_not_fitted_after_enough_frames() {
        let few: Vec<Telemetry> = (0..5).map(|_| frame(2, 3)).collect();
        assert_eq!(st(&sensors(None, None, &few, false, false), "trill"), "checking");
        let many: Vec<Telemetry> = (0..30).map(|_| frame(2, 3)).collect();
        let s = sensors(None, None, &many, false, false);
        assert_eq!(st(&s, "trill"), "not_fitted");
        assert_eq!(st(&s, "imu"), "running");
    }

    #[test]
    fn trill_that_ran_and_stopped_is_lost() {
        let many: Vec<Telemetry> = (0..30).map(|_| frame(2, 3)).collect();
        assert_eq!(st(&sensors(None, None, &many, false, true), "trill"), "lost");
        assert_eq!(st(&sensors(None, None, &[frame(2, 2)], false, true), "trill"), "running");
    }

    #[test]
    fn note_path_without_a_strip_needs_tap_percussion() {
        let off = vec![(11u8, 0u16), (14, 0), (12, 9), (13, 39), (31, 0)];
        let p = note_path(&off, false, true);
        assert!(!p.strike_ready);
        assert!(p.hint.contains("tap percussion"));
        let on = vec![(11u8, 1u16), (14, 0), (12, 9), (13, 39), (31, 0)];
        let p = note_path(&on, false, true);
        assert!(p.strike_ready);
        assert!(p.summary.contains("note 39 on channel 10"));
        assert!(note_path(&[(14, 3)], false, true).strike_ready);
        assert!(!note_path(&[(14, 1)], false, true).strike_ready);
    }
}
