// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use pilab_core::apps::Manifest;

const GOOD: &str = r#"
schema = 1
[app]
id = "airplay-receiver"
name = "AirPlay receiver"
version = "5.6.3"
license = "MIT"
summary = "Plays AirPlay 2 audio through the DAC"
kind = "audio"
[service]
unit = "shairport-sync.service"
[[network]]
proto = "tcp"
port = 7000
exposure = "lan"
[security]
risk = "Anyone on this network can play audio unless a password is set."
"#;

#[test]
fn good_manifest_parses_with_defaults() {
    let m = Manifest::parse(GOOD).unwrap();
    assert_eq!(m.app.source, "base");
    assert_eq!(m.service.as_ref().unwrap().scope, "user");
    assert_eq!(m.resources.rt_class, "none");
    assert_eq!(m.security.hardening, "none");
    assert!(!m.security.radio);
}

#[test]
fn problems_are_all_reported() {
    let bad = GOOD.replace("airplay-receiver", "Air Play").replace("MIT", "").replace("risk = \"Anyone on this network can play audio unless a password is set.\"", "");
    let e = Manifest::parse(&bad).unwrap_err();
    assert!(e.contains("app.id"), "{e}");
    assert!(e.contains("app.license"), "{e}");
    assert!(e.contains("security.risk"), "{e}");
}

#[test]
fn unit_names_cannot_smuggle_arguments() {
    for u in ["x.service --now", "../x.service", "x.timer", "-x.service", "x y.service"] {
        let m = GOOD.replace("shairport-sync.service", u);
        assert!(Manifest::parse(&m).is_err(), "{u:?} must be refused");
    }
}

#[test]
fn unknown_schema_and_fields() {
    assert!(Manifest::parse(&GOOD.replace("schema = 1", "schema = 2")).is_err());
    assert!(Manifest::parse(&GOOD.replace("kind = \"audio\"", "kind = \"toaster\"")).is_err());
}

#[test]
fn shipped_manifests_are_valid() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../etc/trillium/apps.d");
    let found = pilab_core::apps::load_dir(&dir);
    assert!(!found.is_empty(), "no manifests in {dir:?}");
    for r in found {
        if let Err((p, e)) = r {
            panic!("{}: {e}", p.display());
        }
    }
}
