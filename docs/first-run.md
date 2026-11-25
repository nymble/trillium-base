<!-- Copyright (C) 2026 M-of-N -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# First boot to first sound

A walkthrough of the path a new owner takes with the SD image (`docs/image.md`), where it
hurts, and a setup wizard that removes the friction. The target is **flash to first sound in
ten minutes** with no terminal.

## The path today

| Step | What the owner does | Where it can go wrong |
| --- | --- | --- |
| 1. Get the image | Download `trillium-base-<version>.img.xz` and its `.sha256`, check with `shasum -a 256 -c` | The checksum sits beside the file, so it proves the download, not the publisher (security gap S8) |
| 2. Flash | Raspberry Pi Imager: *Choose OS, Use custom*, pick the file and the card. In OS customisation set user `lab`, enable SSH with a public key | Forgetting customisation leaves no way in except a keyboard and monitor, or putting `authorized_keys` on the boot partition; Imager's Wi-Fi and host name settings are ignored |
| 3. First boot | Ethernet in, HDMI optional, power on. A minute or two longer than later boots (host keys, filesystem resize, host name `tbase-xxxx`) | Nothing shows on a headless unit to say it is still working; Wi-Fi-only owners are stuck |
| 4. Pair | HDMI shows a pairing QR (5 minutes, once). Scan it with a phone, or open `http://tbase-xxxx.local:8410/` and type the 8-character code. Without HDMI: `ssh lab@tbase-xxxx.local trillium-base pair --qr` | `.local` does not resolve on some Android and Windows setups; the code expires; a second browser needs the SSH command again |
| 5. First sound | Plug speakers or headphones into the DAC, play a note from a USB keyboard or the console | There is no guided test. A wrong or missing DAC overlay gives silence with no explanation. The master is -18 dB, which on sensitive headphones is quiet and on a hot amp is not |
| 6. Make it theirs | Rename, set the time zone, decide on AirPlay, Bluetooth, SSH, backups | None of it is offered; the defaults are chosen for them, and the unit has no password for the local console |

## Friction points, ranked

| # | Friction | Why it matters | Fix |
| --- | --- | --- | --- |
| F1 | No guided first sound or DAC check | The most common failure is silence, and nothing explains it | Wizard step 3 below |
| F2 | Ethernet only; no Wi-Fi provisioning | Many studios have no cable near the unit | Accept `wifi` in the boot-partition file (`tbase-setup.toml`, below) and a Settings page for NetworkManager; radio on only after the owner presses the button |
| F3 | Pairing depends on HDMI or SSH | Headless units cannot be paired from a browser | Let the owner choose the first code when flashing: `pair_code` in `tbase-setup.toml` (below) becomes a one-time code valid for one hour from first boot and is deleted from the card. A code printed by the unit at boot cannot work headless, because the owner would have to pull the card to read it |
| F4 | A new browser or phone needs SSH to pair again | The QR flow is the only path | Settings, *Pair another device*: shows a fresh code and QR once you are already paired |
| F5 | `.local` names fail on some networks | Android and Windows without Bonjour | Put the IP address in the QR and on the HDMI banner next to the name |
| F6 | The console is plain HTTP and browsers say "Not secure" | Looks wrong, and the token is exposed on the LAN (S1) | TLS with a pinned device certificate; the QR carries the certificate hash |
| F7 | No time zone or clock prompt | Recordings and logs are stamped wrongly; with no NTP the clock can be years off (R6) | Wizard step 2 |
| F8 | Security choices are invisible | AirPlay, Bluetooth and SSH are on or off without the owner knowing (S2) | Wizard step 5 |
| F9 | No in-console update | The owner re-flashes or uses the developer deploy | Update page when A/B and signed releases exist; until then, show the running and latest version |
| F10 | The Devices and Signals pages assume the owner knows MIDI routing | A keyboard that plays nothing is a support ticket | Wizard step 4: list what is plugged in, route the first MIDI source to the synth, show a live meter |

## Proposed setup wizard

A resumable, skippable flow at `/#setup`, shown on the first visit after pairing and from
Settings afterwards. State lives in `~/.config/pilab/setup.toml` (which steps are done), so it
survives a reload. Every step uses calls the console already has, except where marked new.

| Step | The owner sees | Uses |
| --- | --- | --- |
| 1. Pair | The code or QR they just used, the unit name and version, "this browser is paired" | `/pair/redeem` (done), `/info` |
| 2. Name and clock | Unit name (renames `tbase-xxxx`), time zone list, "clock synchronised: yes / no, set it from this browser" | new: `/system/identity`, `/system/time` |
| 3. Sound check | "Is your speaker or headphone plugged into the DAC? Turn it down." A **Play test tone** button at -30 dBFS for two seconds, then *I heard it* or *No sound*, which opens a checklist: DAC detected (`aplay -l`), overlay line present, output linked, master and sink volume | `/audition/play`, `/audio/devices`, `/graph`, `/mixer` |
| 4. Play something | Detected USB and Bluetooth MIDI and audio devices, the first MIDI source routed to Trillium Synth automatically, a live level meter, "play a key" | `/devices`, `/midi/inputs`, `/meters` |
| 5. Network and access | Wi-Fi join (if the unit has a radio and the owner presses *Turn on Wi-Fi*), AirPlay receiver on or off with a password field, Bluetooth pairing window button, SSH keys (add, list), "who can reach this unit" summary | `/services/{id}`, new: AirPlay password, key list |
| 6. Save it | Where backups go (NAS or USB), a first backup, the app list with switches (`docs/app-manager.md`), "done" | new: `/backup`, `/apps` |

Rules for the wizard:

- Nothing transmits (Bluetooth advertising, Wi-Fi access point) until the owner presses the button
  for it in that step.
- Nothing changes the master level above -18 dBFS or the DAC volume; the test tone is fixed at
  -30 dBFS and fades in.
- Every step can be skipped, and Settings shows what was skipped.
- Each step shows the command line equivalent in a collapsed box, so the wizard teaches the CLI.

## Boot-partition setup file

For units with no screen, the owner can drop a `tbase-setup.toml` on the card's boot partition,
next to `authorized_keys`. `trillium-firstboot` reads it once, applies it and deletes it:

```toml
# every key is optional
name = "studio-1"            # host name, replaces tbase-xxxx
pair_code = "..."            # 12+ characters you choose; one-time, valid 1 hour from first boot, then removed
timezone = "America/Los_Angeles"

[wifi]                        # joins the network; radio stays off if the table is absent
ssid = "..."
psk = "..."                   # removed from the card once applied

[services]
airplay = false
bluetooth = false
```

The Wi-Fi key and the pairing code are secrets and are deleted from the card after use. Without the file nothing
changes from today.

## Success measures

- Flash to first sound without a terminal, median under ten minutes, on a Mac and on a phone.
- No step needs SSH.
- "Silence" support cases end at the sound-check checklist.
