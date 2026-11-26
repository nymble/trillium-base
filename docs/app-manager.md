<!-- Copyright (C) 2026 Paul Lambert -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# App manager: adding and removing apps from the console

Design for installing, enabling, disabling and removing music, MIDI, analysis, SDR and
generative apps from Trillium Console, plus the prototype that exists today: a manifest format
and `trillium-base apps list|show|validate|enable|disable` for apps already on the unit.

## Goals and limits

- One place to see what runs on the unit, what it listens on, what it costs in CPU and memory,
  and to switch it on or off.
- An app declares itself in a manifest. The console and CLI trust the manifest only after it
  validates; an invalid manifest is reported, never half-applied.
- Real-time safety is a feature: the isolated CPU is for one real-time app at a time, and the
  manager says so before it enables a second.
- Security by default: an app that listens on the LAN says so and carries a plain-language risk
  line; radio is never switched on by a script.
- Not in scope for v0: arbitrary third-party code from the internet. The first catalogue is
  curated, signed and built by us.

## Prototype that exists today

| Piece | Where |
| --- | --- |
| Manifest schema 1 and validation (`pilab_core::apps`) | `rust/crates/pilab-core/src/apps.rs`, tests in `tests/apps.rs` |
| Manifests for AirPlay receiver, AirPlay sender, Bluetooth MIDI, Cabbage | `etc/trillium/apps.d/*.toml`, installed to `/etc/trillium/apps.d` by `scripts/install-console.sh` |
| CLI | `trillium-base apps list`, `show ID`, `validate FILE`, `enable ID [--dry-run] [--confirm-radio]`, `disable ID [--dry-run]` |

Manifests are read from `/etc/trillium/apps.d`, `/usr/share/trillium/apps.d` and
`~/.config/pilab/apps.d`; a later directory overrides an earlier one with the same id.
`PILAB_APPS_DIR` replaces the search path for tests.

What the prototype does and does not do:

- `list` and `show` read manifests and ask systemd for state. Checked on the lab unit: the three
  services show `enabled` and `active`, Cabbage shows `-` because it has no unit.
- `enable` and `disable` run `systemctl enable --now` or `disable --now` on the app's unit, with its
  required units (nqptp for AirPlay) brought up first. Disable stops only the app's own unit, since
  a shared requirement may serve another app. They were exercised with `--dry-run` only, so no
  running service was touched.
- Radio apps refuse without `--confirm-radio`, and unit names are validated so a manifest cannot
  smuggle options into `systemctl` (a test found and fixed a leading `-`).
- It does not install anything, does not edit the console, and does not yet apply the hardening
  drop-in; `show` prints the drop-in it would use.

## Manifest schema 1

TOML, one file per app. Fields marked * are required.

| Table and key | Meaning |
| --- | --- |
| `schema`* | `1` |
| `app.id`*, `name`*, `version`*, `summary`* | Stable id (lower-case, digits, dashes), display name, version, one line |
| `app.license`* | SPDX expression of the app itself |
| `app.kind`* | `audio`, `midi`, `control`, `network`, `analysis`, `sdr`, `generative`, `host` |
| `app.source` | `base` (in the image), `apt`, `oci`, `local-build` |
| `service.unit`, `scope`, `requires` | The systemd unit, `user` or `system`, and units to bring up first. Omit the table for apps started by hand |
| `resources.rt_class` | `none`, `soft` (CPUs 0-2, normal priority) or `rt` (needs the isolated CPU and an RT priority) |
| `resources.cpus`, `memory_max` | Affinity and a memory cap, applied through the drop-in |
| `network[]` | `proto`, `port`, `exposure` (`localhost`, `lan`, `outbound`), `note` |
| `audio.nodes`, `audio.autoconnect[]` | PipeWire nodes it creates and the links the patch panel makes when it starts |
| `midi.inputs`, `midi.outputs` | MIDI port names it exposes |
| `config.schema`, `config.file` | JSON Schema the console renders as a settings form, and the file the app reads |
| `security.risk` | Required when any port is `lan`; shown before enabling |
| `security.radio` | Transmits radio; needs explicit confirmation every time |
| `security.needs_root` | Needs a system-scope unit |
| `security.hardening` | `none`, `basic` or `strict`: which sandbox drop-in is generated |

Validation catches: wrong schema, bad id, missing licence or summary, unknown kind, source,
scope, class or exposure, a LAN port without a risk line, `needs_root` on a user unit, and unit
names that are not plain `*.service` names. All problems are reported together.

Example (shipped as `etc/trillium/apps.d/airplay-receiver.toml`):

```toml
schema = 1
[app]
id = "airplay-receiver"
name = "AirPlay receiver"
version = "5.6.3"
license = "MIT"
summary = "Phones and Macs on the network can play audio to the DAC"
kind = "audio"
source = "local-build"
[service]
unit = "shairport-sync.service"
requires = [{ unit = "nqptp.service", scope = "system" }]
[resources]
rt_class = "soft"
memory_max = "256M"
[[network]]
proto = "tcp"
port = 7000
exposure = "lan"
[audio]
autoconnect = [{ from = "shairport-sync:output_*", to = "Built-in Audio Stereo:playback_*" }]
[security]
risk = "Anyone on this network can play audio through the DAC unless a password is set."
hardening = "basic"
```

## Packaging: how apps arrive

| Option | For | Against | Verdict |
| --- | --- | --- | --- |
| Debian packages from a signed apt repository we run | Native to the base, dependency handling, `apt remove` is clean, security updates arrive the same way, our SBOM tooling already reads dpkg | We maintain a repository and signing key; packages must follow Debian policy | **Recommended for v1.** Each app is a `.deb` that ships its manifest in `/usr/share/trillium/apps.d` and its unit, depends on libraries from Debian, and never starts itself on install |
| OCI containers (podman) | Isolation, third-party apps with awkward dependencies, easy rollback | Real-time audio, ALSA/PipeWire sockets and `SCHED_FIFO` from a container need care; adds a runtime to the image; the isolated CPU is harder to guarantee | Later, for apps that are not real-time (analysis, web tools). Keep the same manifest with `source = "oci"` |
| systemd portable services or sysext | Image-like add-ons without a container runtime | Less tooling and fewer examples on Debian | Watch |
| Build on the unit (today: shairport-sync, nqptp, Cabbage) | Works now | Slow, unsigned, pulls a compiler onto the unit | Replace with `.deb` builds in CI |

The console never runs `apt` itself. A small root helper (the same one that replaces the broad
`sudo` rule, see `docs/security.md` S3) accepts only: `install ID`, `remove ID`, `upgrade ID`, where
`ID` is in the signed catalogue. It runs `apt-get` with the trillium repository pinned and nothing
else.

## Lifecycle

States: **available** (in the catalogue), **installed** (package present, manifest registered),
**enabled** (unit enabled), **running**, **failed**.

| Action | What happens |
| --- | --- |
| Install | Console shows the manifest (ports, risk, RT class, licence, size). Owner confirms. Helper verifies the catalogue signature, installs the package, registers the manifest, does **not** enable it |
| Enable | Resource check (second `rt` app, memory budget, port clash), LAN-exposure confirm with the risk line, radio confirm, install the hardening drop-in, `systemctl enable --now`, then the patch panel applies `audio.autoconnect` |
| Disable | `systemctl disable --now`, remove the links it made. Settings stay |
| Uninstall | Disable, `apt-get remove` (keeps the config). *Remove and delete data* is a second button with a typed confirm |
| Upgrade | Catalogue shows a newer version; the helper upgrades, keeps the previous package for rollback, restarts if the app was running |
| Failure | The console shows `failed` with the last journal lines; the app is not restarted in a loop (the unit has a start limit) |

## Resources and real-time

- One `rt` app at a time on CPU 3. The manager warns on a second and refuses if the first is the
  synth, unless the owner confirms. `soft` apps are pinned to CPUs 0-2.
- Memory caps come from `memory_max`; a leak ends the app, not the synth.
- The patch panel remembers links per app id, so a relaunched Cabbage gets its two links back
  (robustness R7).

## Sandboxing

`security.hardening` selects a generated systemd drop-in (`trillium-base apps show ID` prints it):

| Level | Options |
| --- | --- |
| `none` | Nothing, for apps that need the whole machine (desktop hosts) |
| `basic` | `NoNewPrivileges`, `PrivateTmp`, `ProtectSystem=full`, `ProtectControlGroups`, `LockPersonality`, plus `MemoryMax` |
| `strict` | `basic` plus `ProtectKernelTunables/Modules/Logs`, `RestrictNamespaces`, `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK`, `SystemCallFilter=@system-service`, an empty capability set, and `RestrictRealtime` only for non-real-time apps. `MemoryDenyWriteExecute` is never set because Csound and Faust JIT |

## Console surface

An **Apps** page lists cards: name, version, state, one-line risk, ports, CPU class, and a switch.
Details show the manifest, the generated drop-in, logs, a settings form from `config.schema`, and
the SBOM entry (package, version, licence). It sits beside the existing Services view, which
becomes a read-only view of system services. API (new, not built):

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/apps` | Manifests with state and resource use |
| `POST /api/v1/apps/{id}/enable` and `/disable` | `{confirm?, confirm_radio?}`; same checks as the CLI |
| `GET /api/v1/apps/catalog` | Signed catalogue entries not yet installed |
| `POST /api/v1/apps/{id}/install` and `/remove` | Through the helper only |
| `GET/PUT /api/v1/apps/{id}/config` | Validated against `config.schema` |

## Supply chain and licences

- The catalogue is a signed index (minisign, offline key) listing package names, versions and
  SHA-256 hashes. The helper refuses an unsigned or unknown entry. The same key signs the image
  and the SBOMs (`docs/security.md` S8).
- Every package carries its licence in the manifest. A catalogue build fails on an app with a
  copyleft licence linked into our commercial-licensed code; GPL apps run as separate processes and
  are fine (`sbom/trillium-base-0.4.0-beta.1-licenses.md`).
- `tools/sbom.sh` regenerates the SBOM for the installed set, so the SBOM follows what the owner
  installed.

## Next steps

1. Install the hardening drop-in on enable, and the patch-panel autoconnect rules (needs the
   console's patch-panel code, which another worker is changing).
2. `GET /api/v1/apps` and the Apps page.
3. `.deb` builds for shairport-sync, nqptp and Cabbage in CI, then the signed catalogue and helper.
4. Candidates once the base is in place, from the survey of open-source audio software: Pure Data,
   SuperCollider and Csound as runtimes (already installed on the lab unit), a Faust runner, an SDR
   receiver, a spectrum and loudness analyser, and a generative sequencer.
