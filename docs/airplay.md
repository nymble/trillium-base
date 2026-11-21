# AirPlay on Trillium Base

Trillium Base can optionally be an AirPlay **receiver** (Macs, iPhones and iPads play through the DAC) and an AirPlay **sender** (anything in the PipeWire graph, such as a Faust or Cabbage patch, plays on AirPlay speakers on the LAN). Both are off on a fresh build, and each has its own on/off switch.

AirPlay is for listening, not for playing live. Both directions add about 1.5 to 2 seconds of buffering. Keep monitoring live synthesis on the DAC.

## Quick use

```sh
pilab-airplay status                  # services, listening ports, discovered speakers
pilab-airplay receiver on|off         # "pilab" appears in Apple AirPlay menus
pilab-airplay sender on|off           # AirPlay speakers appear as PipeWire sinks
pilab-airplay speakers                # list them
pilab-airplay send Office             # 6 s, 440 Hz, -20 dBFS test tone at speaker volume 0.25
pilab-airplay send Office synth-gui   # link a running node's outputs to the speaker
pilab-airplay send Office JUCEJack --volume 0.4
pilab-airplay unsend Office [node]    # remove those links
```

Every subcommand is idempotent. `on` also enables the unit so the choice survives a reboot, and `off` disables it. `send` leaves the node's existing links (for example to the DAC) in place, so the DAC plays live and the speaker plays the same audio about 2 s later. Speaker names match the AirPlay name, exactly or by prefix, without regard to case. Node names are the PipeWire port prefixes shown by `pw-link -o`.

## Receiver

| Piece | What |
| --- | --- |
| `shairport-sync` 5.6.3, built from source | AirPlay 2 receiver. Debian's 4.3.7 package is classic AirPlay only. |
| `nqptp` 1.2.9, built from source | PTP clock helper that AirPlay 2 needs. Debian 13 has no package. |
| `/etc/shairport-sync.conf` | Name `pilab`, `service_type = "airplay2"`, PipeWire backend, output fixed at 48 kHz, soxr interpolation, default volume -24 dB |
| `~/.config/systemd/user/shairport-sync.service` | User unit, because PipeWire is a user service. It waits up to 10 s for `nqptp.service`. |
| `nqptp.service` (system) | Installed by `make install` with `DynamicUser=yes` and `CAP_NET_BIND_SERVICE` (it binds UDP 319/320) |

Audio arrives as a PipeWire stream called **AirPlay (pilab)** (node `airplay-receiver`). It follows the default sink, the Katana DAC, and mixes with whatever else is playing. Output is pinned to 48 kHz so an AirPlay session never switches the DAC's rate under the synths. AirPlay 2 sends 44.1 kHz or 48 kHz, and shairport-sync resamples it with soxr. Volume comes from the sender's slider. `volume_range_db = 60` maps the slider to 60 dB of attenuation in software, applied before PipeWire.

`service_type` is pinned to `airplay2`. With the default `auto`, shairport-sync silently falls back to classic AirPlay (TCP 5000, name "pilab (Classic)") if nqptp is not up yet. That happened on the first start, when both units started at the same moment.

Build: `scripts/build-airplay.sh` (see BUILD.md section 10).

### How to try it

- **Mac:** Control Center → Sound → pick **pilab**, or System Settings → Sound → Output → **pilab**. In Music, use the AirPlay button.
- **iPhone/iPad:** Control Center → the AirPlay icon on the Now Playing tile → **pilab**.
- A pairing code is not needed (`acl=0`, no password).

What can be checked without a sender (all done on 2026-11-19):

```sh
pilab-airplay status                         # receiver active, TCP 7000 listening
avahi-browse -rt _airplay._tcp | grep -A3 pilab
dns-sd -B _airplay._tcp                      # on the Mac: "pilab" is listed
nc -z pilab.local 7000                       # on the Mac: port reachable
# GET /info over RTSP/1.0 on port 7000 returns 200 and a ~1.3 kB capability plist
```

A headless playback test was not possible. PipeWire's RAOP client speaks classic AirPlay with uncompressed PCM, which shairport-sync's AirPlay 2 mode accepts but cannot decode ("error during decoding"). pyatv uses NTP timing, which shairport-sync's AirPlay 2 mode does not implement ("501 Not Implemented"). Real Apple senders use AirPlay 2 with PTP and are the target.

## Sender

PipeWire ships `libpipewire-module-raop-discover` and `libpipewire-module-raop-sink`. These find AirPlay speakers over mDNS and create one PipeWire sink per speaker. On Trillium Base they run in a **separate PipeWire client process** (`pipewire -c pilab-airplay-sender.conf`, user unit `pilab-airplay-sender.service`), not as a drop-in in the main `pipewire.conf.d`. That way, turning the sender on or off never restarts the main realtime graph, so running synths keep playing.

Speaker sinks get `priority.session = 100`, so WirePlumber never picks a network speaker as the default sink. The DAC stays the default. The Pi's own receiver is excluded (`raop.discover-local` defaults to false), so it cannot loop to itself.

### HomePod User-Agent fix

PipeWire 1.4.2 hardcodes `User-Agent: PipeWire/1.4.2` in its RAOP client. HomePods on HomePodOS 27.x answer `403 Forbidden` without a `CSeq` header, and PipeWire then waits forever after `OPTIONS`. With an Apple-style User-Agent the same request gets `200 OK`. Newer PipeWire adds a `raop.user-agent` property for this. Until Trillium Base has one, `pilab-airplay sender on` builds a private module directory:

- `~/.local/lib/pilab-pw-modules/` holds symlinks to every system PipeWire module.
- It also holds a copy of `libpipewire-module-raop-sink.so` with the User-Agent string changed to `AirPlay/366.0`, with the same byte length and NUL padding.
- The sender unit sets `PIPEWIRE_MODULE_DIR` to that directory. The system module and the main PipeWire are untouched.
- The copy is rebuilt automatically whenever the system module is newer, for example after a PipeWire upgrade. If a future PipeWire changes the string, the patch step fails loudly. In that case, switch to `raop.user-agent` in `create-stream` instead.

### Test result: Office (2026-11-19)

- Discovery: 6 speakers (`Office`, `Office (2)`, `Kitchen`, `Media`, `Yurt`, and a Mac). `Office` and `Office (2)` are first-generation HomePods (`AudioAccessory1,1`, AirTunes 1005.13.2, OS 27.2.1), probably a stereo pair with `Office` as leader.
- Stock PipeWire: TCP connect, then `OPTIONS` was never answered (403 to the PipeWire UA). No audio.
- With the UA fix, the full session completed: `OPTIONS 200`, `ANNOUNCE 200`, `SETUP 200` (server, control and timing ports), `RECORD 200`, `SET_PARAMETER 200`. RTP audio flowed for the whole tone. Encryption: none (`et=0` is offered). Password or PIN: none needed.
- The receiver latency negotiated was 66150 frames at 44.1 kHz = **1.5 s**, plus PipeWire's queue. Expect 1.5 to 2 s from the graph to the room.
- Audibility needs a person in the room. Some HomePodOS 27.0 builds accepted unencrypted RAOP but stayed silent, and 27.2 reportedly fixed that. If Office stays silent while the session runs, the next step is AirPlay 2 with encryption (HAP pairing). PipeWire 1.4 cannot do that; it needs a newer PipeWire or a different client.
- The realtime graph was unaffected. DAC xruns were +0 during sender discovery, sends and links, the quantum stayed 64 at 48 kHz, and the speaker sink joined as a resampled 44.1 kHz follower.

### Volume

`send` sets the speaker sink volume (default 0.25 on WirePlumber's cubic scale, about -36 dB) and re-applies it 1 s and 3 s after the stream starts, because HomePods can ignore a volume sent before audio starts. PipeWire forwards sink volume to the speaker as AirPlay volume.

## Ports and services (for IT Department and the console security view)

The Pi has no host firewall (no nftables rules, no ufw), so the LAN can reach everything below. Nothing is forwarded at the router.

| Mode | Process | Listens on | Notes |
| --- | --- | --- | --- |
| Receiver | `shairport-sync` (user `lab`) | **TCP 7000**, all interfaces, IPv4 and IPv6 | AirPlay 2 RTSP/HTTP control (`/info`, pairing, `SETUP`) |
| Receiver | `shairport-sync` | ephemeral TCP and UDP **during a session only** | AirPlay 2 event, buffered-audio and realtime-audio channels, negotiated in `SETUP` |
| Receiver | `nqptp` (dynamic system user) | **UDP 319, 320**, all interfaces | PTP event and general messages |
| Receiver | `nqptp` | UDP 9000 on 127.0.0.1 and ::1 only | control from shairport-sync |
| Receiver | `avahi-daemon` (already running) | UDP 5353 | advertises `_airplay._tcp` and `_raop._tcp` as **pilab** |
| Sender | `pipewire -c pilab-airplay-sender.conf` | nothing while idle | mDNS browse only, through avahi |
| Sender | same | **during a send:** UDP 6001 (control, connected to the speaker), UDP 6002 (timing, listens on 0.0.0.0), one ephemeral UDP port (audio) | outbound TCP to the speaker's port 7000 (RTSP) |

When both are off, nothing extra listens (`pilab-airplay status` shows no rows), and the only remaining AirPlay-related service is avahi.

Risks to show in a console:

- The receiver accepts any device on the LAN with no password, so anyone on the network can play audio through the DAC at up to the sender's volume. shairport-sync supports `password` and an access list if that is needed.
- It interrupts an existing session (`allow_session_interruption = "yes"`).
- The receiver runs as `lab`, the audio user, not root.

## Licensing

| Component | License | How it is used |
| --- | --- | --- |
| shairport-sync | Mostly **MIT** (core files; see the header of `shairport.c`). Bundled parts carry their own licenses: ALAC decoder (BSD-style), tinysvcmdns (BSD-3), tinyhttp, FFTConvolver (MIT), pair_ap and evrtsp (BSD-3 and per-file). The top-level `COPYING` says "refer to the individual source files". | Built unmodified from tag 5.6.3 and run as a separate service |
| nqptp | **GPL-2.0** | Built unmodified from tag 1.2.9 and run as a separate service. Shared memory is the only interface to shairport-sync. Redistributing a Trillium Base image that includes the binary means offering its source (pin the tag and ship or link it). |
| PipeWire RAOP modules | MIT | System package. Trillium Base ships a one-string User-Agent patch of a copy, generated on the device rather than distributed. |
| FFmpeg libraries (libavcodec etc.) | LGPL-2.1+ as packaged by Debian | dynamically linked |

None of these are linked into Trillium Base's own code. They run as separate processes behind `pilab-airplay`. That wrapper is the pattern to keep for third-party services, especially GPL ones.

## Recovering from an xrun storm (seen during this work)

Twice during the AirPlay work, the Katana sink got into a state where it logged about 10 xruns per second and PipeWire's data loop ran at about 50% CPU, even after the AirPlay services were stopped. Both times it started during heavy CPU load on cores 0–2: a `pip install` once, and a PipeWire debug run that wrote about 30k log lines. The data-loop thread runs on CPUs 0–2, not on the reserved CPU 3. It recovers without restarting anything:

```sh
S=alsa_output.platform-soc_107c000000_sound.stereo-fallback
pactl suspend-sink $S 1; sleep 1; pactl suspend-sink $S 0
pw-top        # ERR column for the DAC should stop climbing
```

This is not specific to AirPlay. It may explain an earlier report of a synth that "stopped after some use". A follow-up should pin PipeWire's data loop to CPU 3 and add an xrun watchdog.
