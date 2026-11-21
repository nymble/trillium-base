// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Local-time formatting without extra crates.

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn local(t: i64) -> libc::tm {
    let mut tm: libc::tm = unsafe { core::mem::zeroed() };
    let tt: libc::time_t = t as libc::time_t;
    unsafe { libc::localtime_r(&tt, &mut tm) };
    tm
}

/// 2026-11-20T08:37:00-08:00
pub fn iso_local(t: i64) -> String {
    let tm = local(t);
    let off = tm.tm_gmtoff / 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        if off < 0 { '-' } else { '+' },
        off.abs() / 60,
        off.abs() % 60
    )
}

/// 20261120-083700
pub fn stamp(t: i64) -> String {
    let tm = local(t);
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

pub fn hms(t: i64) -> String {
    let tm = local(t);
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
