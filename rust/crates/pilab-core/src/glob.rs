// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Case-insensitive `*` / `?` glob matching for MIDI source names.

pub fn matches(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::matches;
    #[test]
    fn globs() {
        assert!(matches("Trillium*", "Midi-Bridge:Trillium 0 (capture)") == false);
        assert!(matches("*Trillium*", "Midi-Bridge:Trillium 0 (capture)"));
        assert!(matches("*trillium*", "TRILLIUM"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "abbc"));
        assert!(matches("*", ""));
    }
}
