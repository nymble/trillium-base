<!-- Copyright (C) 2026 Paul Lambert -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Security: threat model, defaults and gaps

Trillium Base is built for a trusted LAN (a home or studio network behind a router, nothing
forwarded from the internet). This page says what a unit exposes, who could attack it and how,
what already protects it, and what is still open, ranked. It is guidance, not a guarantee.

Measured on the lab unit (Debian 13, kernel 6.18.60 RT, console 0.4.0-beta.1) on 2026-11-24.
The SD image has fewer services than the lab unit; where they differ the tables say so. IT
Department owns hardening baselines, the network and backups: the items that are theirs are
listed at the end rather than repeated here.

## Assets and attackers

| Asset | Why it matters |
| --- | --- |
| The pairing token (`~/.config/pilab/console-token`) | Whoever holds it controls the console: sound, routing, settings, recording, the SSH switch |
| Shell as `lab` | `lab` has passwordless sudo, so shell access is root |
| The DAC and headphone output | A loud signal is a safety issue as well as a nuisance |
| Recordings, scenes, device profiles | The owner's work |
| The Trillium Zero and other USB devices | Settings can be written to a Zero over SysEx |
| The software supply chain | Crates, Debian packages, the image, updates |

| Attacker | Can reach | Cannot (assumed) |
| --- | --- | --- |
| Another device on the LAN (guest, compromised IoT) | Every listening port, mDNS, can sniff plain HTTP | Physical access |
| A web page in the owner's browser (malicious site or ad) | Anything the browser can reach from the LAN, including `http://tbase-xxxx.local:8410` | Read the token from another origin |
| Someone in Bluetooth range | BLE MIDI while a pairing window is open | The LAN |
| Someone who plugs in a USB device or takes the SD card | Everything on the card (token, SSH host keys, recordings) | |
| A malicious or compromised upstream package or crate | Whatever the unit installs or builds | |

## What a unit exposes

Listening sockets on the lab unit (`ss -tulpn`) and in the SD image:

| Port | Process | Lab unit | Image | Notes |
| --- | --- | --- | --- | --- |
| TCP 22 | sshd | all interfaces | all interfaces | Key-only; one authorized key on the lab unit |
| TCP 8410 | pilabd | all interfaces, plain HTTP | same | Token required except `/api/v1/info`, `/endpoints`, `/pair/redeem` and the static files |
| TCP 7000, UDP 319/320 | shairport-sync, nqptp | all interfaces, **enabled at boot** | not installed | AirPlay 2; no password, so any LAN device can play through the DAC |
| UDP 5353 | avahi | all interfaces | same | mDNS publishes host name, device id and version |
| TCP 5900 | wayvnc | 127.0.0.1 only | not installed | Reached through an SSH tunnel |
| UDP 9000 | (loopback only) | 127.0.0.1 and ::1 | | Not reachable from the LAN |
| Bluetooth | bluetoothd | powered, pairable, not advertising | adapter off (`AutoEnable=false`) | BLE MIDI needs an explicit window |

Not listening but present on the lab unit and unnecessary for a music server: `cups.socket`,
`ModemManager`, `nfs-blkmap`, `lightdm`, `udisks2` (auto-mount), `avahi` wide-area DNS-SD
(`enable-wide-area=yes`), and a desktop with `webkit2gtk`, `ghostscript` and `ffmpeg` that
make up most of the vulnerability list in `sbom/`. The image has none of these.
There is no host firewall on either.

## Threats, mitigations and what is left

### Console on port 8410

| Threat | Today | Verdict |
| --- | --- | --- |
| LAN attacker guesses the token | 48 hex characters (192 bits), compared in constant time | Strong |
| LAN attacker sniffs the token | Plain HTTP; the token is sent on every request and, for the WebSocket, in the URL (`?token=`), which proxies and logs can record | **Open (S1)**: TLS is planned |
| Malicious web page makes the browser call the console (CSRF) | The token is an `Authorization` header, not a cookie, so a page cannot attach it; with no CORS headers the browser blocks cross-origin reads. **New:** a state-changing request or WebSocket upgrade whose `Origin` differs from `Host` is refused with 403 | Closed; defence in depth added |
| DNS rebinding (a hostile name re-pointed at the unit) | **New:** requests whose `Host` is not an IP, a single-label name, the unit's own name or a private suffix (`.local`, `.lan`, `.home.arpa`, `.internal`, `.ts.net`, `.localdomain`) get 403. `PILAB_ALLOWED_HOSTS` adds names | Closed for the API and the static files |
| Clickjacking, MIME sniffing, referrer leaks | **New:** `X-Frame-Options: DENY`, `frame-ancestors 'none'`, `nosniff`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` on API responses | Closed |
| Script injection into the console page steals the token from `localStorage` | The page has one external script and no inline script, but there is no Content-Security-Policy and 17 `innerHTML` assignments | **Open (S6)**: add a CSP and review the `innerHTML` uses |
| Pairing-code brute force | 8 characters from 36 (2.8e12), 5 minutes, single use, destroyed after 5 wrong tries, 0.7 s delay per miss | Strong. A LAN attacker can burn a code during the window (nuisance, not a break-in) |
| What a stolen token allows | Change sound, routing, settings and scenes, record, switch SSH and the AirPlay services (`sudo -n systemctl`), **set the recording folder to any path `lab` can write** (WAV files and `record.toml`), start Faust and SuperCollider | Medium (S4, S9) |
| One shared token, no expiry, no per-device revoke | `pair --rotate` replaces it for everyone | **Open (S7)** |

Verified on the deployed build (commit `45880d0`): `Host: evil.example.com` returns 403 before
authentication; a POST with `Origin: http://evil.example` returns 403 even with a valid token;
the same request with the page's own origin returns 200; a WebSocket upgrade with a foreign
`Origin` returns 403 and with the page's origin returns 101; the CLI (no `Origin`) and an IP-literal
`Host` are unaffected. The tests are in `rust/crates/pilabd/src/security.rs`.

### Pairing, Bluetooth, AirPlay, mDNS, SSH

| Area | Threat | Today | Verdict |
| --- | --- | --- | --- |
| QR pairing | Shoulder-surfing the HDMI code | Valid 5 minutes, once; the token is never in the QR or on screen | Acceptable; HDMI shows a fresh code on every boot, so a unit in a public space should not |
| BLE MIDI | Anyone in range pairs during the window and sends notes or SysEx | "Just works" pairing (no passkey), window time-boxed (default 10 minutes), adapter off in the image | **Open (S5)**: a bonded device stays bonded and can reconnect later; there is no list of bonds or way to remove one in the console. Require numeric comparison where the central supports it |
| AirPlay receiver | LAN device plays at will | No password; default volume -24 dB, DAC and master limits still apply | **Open (S2)**: ship a password option and make the receiver off by default on the lab unit as it is in the image |
| mDNS | Unit announces name, product, version, device id | Useful for discovery, gives an attacker the version | Accept; move the device id out of the TXT record later |
| SSH | Password guessing, key theft | Password and keyboard-interactive login off, root login key-only, no failed logins in three days. `X11Forwarding yes` and TCP forwarding on; `MaxAuthTries 6`; one authorized key on the lab unit although the docs advise two | Low; hand to IT (X11, forwarding, second key) |
| sudo | Any code running as `lab` is root | `NOPASSWD: ALL` for `lab` | **Open (S3)**: narrow it; see below |

### USB and SysEx

- A USB MIDI or audio device is handled as data: it is routed by the console's rules and cannot
  start a service. The Trillium Zero's serial ports are claimed by a udev rule so ModemManager
  does not probe them.
- Writing settings to a Zero over SysEx goes through `POST /devices/{key}/reapply` with
  `confirm: true`, writes the device's RAM only, and reads back to verify. Nothing writes to a
  device on plug-in. The Zero's diagnostics port is not opened.
- A hostile USB device (BadUSB, a keyboard that types) is not stopped by the console. The lab unit
  auto-mounts storage through `udisks2`; the image does not run a desktop. Keep USB ports to
  devices you own.
- MIDI bytes from any source go to the synth through the routing map. SysEx reaches only devices
  that a route or a profile names.

### Supply chain and updates

| Link | State | Gap |
| --- | --- | --- |
| Rust crates | 97 components, `Cargo.lock` committed, built with `--locked`; OSV scan on 2026-11-24: no known vulnerabilities; ZPOD crates pinned to a commit | No `cargo-vet`/`cargo-deny` policy in CI (S10) |
| Debian packages | Signed archive (`Signed-By` keyrings), pulled over HTTP; 21 security updates pending on the lab unit, 6 High krb5 CVEs in the published image | No unattended security updates (S3b); no repo of our own |
| Image | Built with rpi-image-gen from this repository; SHA-256 beside the download | **Open (S8)**: the checksum is hosted next to the file, so it proves integrity, not origin. Sign releases (minisign, offline key) and publish the public key in the repository and on the site. The image has not yet been boot-tested on hardware |
| Source builds (shairport-sync, nqptp, Cabbage) | Built on the unit from pinned tags | No checksum or signature check of the tarballs; list them in `sbom/` (done) |
| Updates | Developer deploys from a Mac over SSH; single boot slot | Signed update client and A/B boot (`docs/roadmap-base.md`) |
| Secure boot | Not yet (listed for a later release). OTP key writes are irreversible, so not before the update path is stable | Planned |

## Gaps, ranked

Fix column: **now** was done in this change; **next** is the next build; **owner** needs a
decision from the owner; **IT** goes to IT Department.

| # | Gap | Severity | Fix |
| --- | --- | --- | --- |
| S1 | Plain HTTP carries the bearer token; WebSocket puts it in the URL | High on a shared LAN | **next**: TLS with a device certificate pinned by device id (in the QR); move the WebSocket token to a first message or subprotocol (touches `app.js`: coordinate with the console work) |
| S2 | AirPlay receiver enabled with no password on the lab unit; Bluetooth powered and pairable on the lab unit | High for headphones | **next**: `shairport-sync` password setting in the console; both off by default on every unit. The image is already off |
| S3 | `lab` has `NOPASSWD: ALL` and `pilabd` runs unsandboxed as `lab`, so a bug in `pilabd` is root. `NoNewPrivileges` cannot be set while `pilabd` shells out to `sudo systemctl` for the SSH switch | High | **next**: replace the blanket rule with one sudoers file that allows exactly `systemctl enable|disable --now ssh.service`, or a small root helper behind a socket; then add `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=read-only` with `ReadWritePaths=~/.config/pilab ~/.local/share/pilab`, `PrivateTmp`, `RestrictAddressFamilies` to the unit. **owner** (the owner): keep passwordless sudo on the lab unit? |
| S3b | No unattended security updates; 21 pending on the lab unit | High | **IT**: `unattended-upgrades` for the security origin on the lab unit; for the image, rebuild monthly and on any Critical |
| S4 | Token holder can set `recording.location` anywhere `lab` can write | Medium | **next**: accept only paths under `~/.local/share/pilab`, `/srv`, `/media`, `/mnt` |
| S5 | BLE MIDI "just works" pairing; bonds persist and cannot be listed or removed | Medium | **next**: numeric-comparison agent, bond list and remove in the console; 5-minute default window |
| S6 | No Content-Security-Policy; `innerHTML` used 17 times; token in `localStorage` | Medium | **next**: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'` after an escape review of the `innerHTML` call sites. Held back now because the console files are being reworked |
| S7 | One token, never expires, no per-device revoke | Medium | **next**: per-device tokens issued by QR pairing, listed and revocable in Settings; expiry on idle |
| S8 | Release authenticity: SHA-256 only; image unverified on hardware | Medium | **owner**: approve an offline minisign key; **next**: sign `.img.xz`, SBOM and the checksum file; boot-test on a spare card before any non-beta tag |
| S9 | Service switches run `systemd-run` and `sudo` from the web process | Medium | folded into S3 |
| S10 | No dependency policy in CI | Low | **next**: run `tools/sbom.sh` and `cargo deny check` in CI (see `sbom/README.md`) |
| S11 | No host firewall | Medium on a shared LAN | **IT**: nftables baseline allowing 22, 8410, 5353 (and 7000, 319, 320 only if AirPlay is on) from the LAN only |
| S12 | Unneeded services and packages on the lab unit (cups, ModemManager, nfs-blkmap, lightdm, udisks2, wide-area mDNS, X11 forwarding) | Medium | **IT**: disable or remove; the image does not have them |
| S13 | The pair-code failure counter is global, so a LAN attacker can keep a code from being used | Low | **next**: count per source address, keep the global limit as a backstop |
| S14 | The image carries tools a music server does not need (`binutils`, `curl`, `wget`, `flashrom`, `stress-ng`, `nano`, `net-tools`, cloud-init and Python 3) | Low | **next**: drop `flashrom`, `binutils`, `net-tools`; keep `curl` only if a feature needs it; confirm cloud-init and Python are needed after first boot |

## Fixed in this change

- Host and Origin guard, security headers, and tests (`rust/crates/pilabd/src/security.rs`), deployed to the lab unit.
- SBOMs, vulnerability scans and a licence summary (`sbom/`, `tools/sbom.sh`).
- Settings and state files are written atomically (a power cut cannot leave half a file) and a
  settings file that fails to parse is kept as `settings.toml.bad-<time>` instead of being
  overwritten by defaults.
- `nqptp.service` restarts on failure (repository copy; installed with the next image or by IT).

## Configuration the guard accepts

| Variable | Meaning |
| --- | --- |
| `PILAB_ALLOWED_HOSTS` | Extra `Host` names, comma-separated, exact or `*.example.com` |
| `PILAB_ALLOWED_ORIGINS` | Extra full origins, for example `https://studio.example` |

Set them in a drop-in for the user unit (`systemctl --user edit pilabd` and add
`Environment=PILAB_ALLOWED_HOSTS=studio.example.com`). A request refused by the guard returns a
JSON message that names the variable.

## Access model guidance (unchanged)

- Key-only SSH: `PasswordAuthentication no`, `KbdInteractiveAuthentication no`, root login
  disabled or key-only, passwords locked for root and the service user.
- Authorize at least two SSH keys so one lost key does not lock you out. Recovery without SSH:
  a keyboard and monitor, or add a key to `authorized_keys` by mounting the boot media on another
  machine (`authorized_keys` at the top of the boot partition is picked up at boot).
- Passwordless sudo and desktop autologin are convenient on a lab unit. Turn them off on a unit
  that leaves a trusted space.
- Credentials for optional services (for example a NAS account) belong in root-only files that
  the console never reads; the console shows only the file path.
- Never commit secrets, private keys, SSH host keys or disk images to this repository.
- Do not forward port 8410 from the internet. Use a VPN such as Tailscale for remote access.

## Items for IT Department

1. Host firewall baseline (S11) and an allowlist for the optional AirPlay ports.
2. `unattended-upgrades` (security origin) on the lab unit; apply the 21 pending updates (S3b).
3. Remove or disable `cups`, `ModemManager`, `nfs-blkmap`, `lightdm`, `udisks2` auto-mount and wide-area
   mDNS on the lab unit (S12); `X11Forwarding no`, `AllowTcpForwarding no`, a second authorized key.
4. Replace `NOPASSWD: ALL` for `lab` with the narrow sudoers rule, once the owner agrees (S3).
5. Backups of `~/.config/pilab` (token, settings, scenes, device profiles) and `~/.local/share/pilab`
   (recordings, library): see `docs/robustness.md`.
6. Ownership of the monthly Debian package scan: `tools/sbom.sh --unit lab@host`.
