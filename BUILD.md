# Rebuilding Trillium Base (lab unit `pilab`) from a blank card

This is the path that produced the running machine on 2026-11-18, reconstructed from the files and packages that are actually on the Pi. Later this should become an Ansible playbook. Until then, follow the steps in order.

Do not flash a card from this document until that work is approved. This repo does not contain an image.

The login user is `lab`. SSH is key-only. The Pi is reached as `lab@pilab.local`.

## 1. Image

Use **Raspberry Pi OS Lite 64-bit, Trixie**, release date 6 Oct 2026:

- File: `2026-10-06-raspios-trixie-arm64-lite.img.xz`
- Official image directory: <https://downloads.raspberrypi.com/raspios_lite_arm64/images/raspios_lite_arm64-2026-10-06/>
- Direct download: <https://downloads.raspberrypi.com/raspios_lite_arm64/images/raspios_lite_arm64-2026-10-06/2026-10-06-raspios-trixie-arm64-lite.img.xz>
- Official SHA-256 (from the accompanying `.sha256` file):

      483db18a48da399b5b7022ffae9a07bc6d99daab30e593aedac09b69ff422843  2026-10-06-raspios-trixie-arm64-lite.img.xz

Verify before writing:

```sh
shasum -a 256 -c 2026-10-06-raspios-trixie-arm64-lite.img.xz.sha256
```

The first-boot `/boot/firmware/issue.txt` on the running machine is also in `boot/issue.txt` here ("Raspberry Pi reference 2026-10-06", pi-gen stage2). `/etc/os-release` reports Debian GNU/Linux 13 (trixie).

Write the image to the card with Raspberry Pi Imager or an equivalent tool. Then remount the boot partition (`bootfs`) and apply the first-boot files below **before** the first power-on.

## 2. First-boot firmware and cloud-init

### `config.txt`

Start from the image's stock `config.txt` and apply three changes (the running file is `boot/config.txt`):

1. Change `dtparam=audio=on` to `dtparam=audio=off` so the onboard headphone DAC is not loaded.
2. Under `[all]`, add:

       # Pi Lab: ES9038Q2M DAC HAT (Katana driver)
       dtoverlay=allo-katana-dac-audio

3. Leave the stock `[pi5]` line `dtoverlay=nospi10` in place. It was already in the 2026-10-06 image.

Do **not** add `kernel=kernel8_rt.img` yet. That file does not exist until the RT kernel package is installed in step 5; adding the line earlier can prevent a boot.

### `user-data`

Copy `boot/user-data` to the boot partition as `user-data`. Replace `<YOUR_SSH_PUBKEY>` with a single SSH public key line (for example `ssh-ed25519 AAAA... comment`). The committed file is a scrubbed copy: the live key is never stored in this repo.

What the file does:

- Hostname `pilab`, `manage_etc_hosts: true`, timezone `America/Los_Angeles`, US keyboard
- User `lab` in `users,adm,dialout,audio,netdev,video,plugdev,cdrom,games,input,gpio,spi,i2c,render,sudo`
- `lock_passwd: true`, `ssh_pwauth: false`, `enable_ssh: true`
- Passwordless sudo: `ALL=(ALL) NOPASSWD:ALL`

Also place an empty file named `ssh` on the boot partition (Raspberry Pi first-boot convention; `enable_ssh: true` should already be enough).

Do not copy `cmdline.txt` from another card. The stock command line is fine; the `root=PARTUUID=...` value is unique per card.

## 3. First boot and full upgrade

Boot the Pi with Ethernet. From a machine that holds the matching private key:

```sh
ssh lab@pilab.local
sudo apt-get -y -q -o Dpkg::Options::=--force-confold full-upgrade
sudo reboot
```

On 2026-11-18 this moved the stock kernels from 6.18.50 to 6.18.60, among other package updates. Reconnect after the reboot.

## 4. Latency tools, then a baseline (optional)

```sh
sudo apt-get install -y -q rt-tests stress-ng
```

`pilab-bench` (installed in step 6) writes cyclictest idle and loaded results under `/var/lib/pilab/bench/`. A pre-RT run under the same `cyclictest -m -S -p 95 -i 1000 -D 45` plus `stress-ng --cpu 4 --vm 2 --vm-bytes 512M --io 1` load saw a worst-case of **506 µs**. That earlier file was not kept; the saved RT result is in `bench/`.

## 5. PREEMPT_RT kernel

```sh
sudo apt-get install -y -q linux-image-rpi-v8-rt linux-headers-rpi-v8-rt
```

This installs `kernel8_rt.img` into `/boot/firmware/`. Then add this line under the existing `[pi5]` section of `/boot/firmware/config.txt`:

```
kernel=kernel8_rt.img
```

Back up `config.txt` first (the running machine has `config.txt.pre-rt`). Reboot. Confirm:

```sh
uname -a
# Linux pilab 6.18.60+rpt-rpi-v8-rt #1 SMP PREEMPT_RT Debian 1:6.18.60-1+rpt1 (2026-11-03) aarch64 GNU/Linux
```

After this, a `pilab-bench` run on 2026-11-18 recorded a worst-case of **94 µs** under the same load (idle max 14 µs). See `bench/20261118-1835-6.18.60+rpt-rpi-v8-rt.txt`. Temperature was 58.2 °C, `throttled=0x0`.

Create the bench directory before the first run (the script does not create it):

```sh
sudo mkdir -p /var/lib/pilab/bench
sudo chmod 755 /var/lib/pilab /var/lib/pilab/bench
```

The script writes as root if invoked with sudo, or needs the directory to be writable by `lab`. On the running machine the file is root-owned because the bench was run with sudo.

## 6. Audio tuning and PipeWire

Install the stack:

```sh
sudo apt-get install -y -q pipewire pipewire-jack pipewire-bin wireplumber rtkit alsa-utils
```

`pipewire-alsa` was installed in this step on 2026-11-18 and later removed as a side effect of the desktop install (step 9). The running machine uses `pipewire-pulse` (pulled in with the desktop) plus `pipewire-jack`. Do not rely on `pipewire-alsa` being present.

Install the config files from this repo, then enable linger and the governor service:

```sh
sudo install -m 644 etc/security/limits.d/95-audio.conf \
    /etc/security/limits.d/95-audio.conf
sudo install -m 644 etc/systemd/system/cpu-performance.service \
    /etc/systemd/system/cpu-performance.service
sudo install -d /etc/systemd/system.conf.d
sudo install -m 644 etc/systemd/system.conf.d/10-cpu-affinity.conf \
    /etc/systemd/system.conf.d/10-cpu-affinity.conf
sudo install -d /etc/pipewire/pipewire.conf.d
sudo install -m 644 etc/pipewire/pipewire.conf.d/10-lowlatency.conf \
    /etc/pipewire/pipewire.conf.d/10-lowlatency.conf

sudo systemctl enable --now cpu-performance.service
sudo loginctl enable-linger lab
```

What those files do:

- `95-audio.conf`: group `audio` gets `rtprio 95`, `memlock unlimited`, `nice -19`
- `cpu-performance.service`: oneshot that writes `performance` to every CPU's `scaling_governor`
- `10-cpu-affinity.conf`: systemd `CPUAffinity=0-2`, leaving CPU 3 for audio
- `10-lowlatency.conf`: 48 kHz, quantum 64, min 32, max 1024, allowed rates through 384 kHz

`CPUAffinity` in `system.conf.d` takes effect only after a reboot. Reboot, then confirm:

```sh
# governors
cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
# linger
loginctl show-user lab -p Linger    # Linger=yes
# ALSA: Katana present (card number may not be 3)
aplay -l
# PipeWire user services (after a login or linger)
systemctl --user is-enabled pipewire pipewire-pulse wireplumber
```

A short tone check, always by name:

```sh
speaker-test -D default:Katana -c 2 -t sine -f 440 -l 1
```

## 7. SuperCollider and the other synthesis tools

```sh
sudo apt-get install -y -q supercollider sc3-plugins puredata csound faust
sudo install -m 755 bin/sc-play bin/pilab-bench /usr/local/bin/
sudo -u lab mkdir -p /home/lab/patches
sudo -u lab cp home/patches/*.scd /home/lab/patches/
```

Versions on the running machine (see also `packages/key-packages.txt`):

- SuperCollider 3.13.0 (`1:3.13.0+repack-3`)
- Pure Data 0.55.2, Csound 6.18.1, Faust 2.79.3

### SuperCollider quirks (required)

These are not optional polish; without them a patch is silent or hangs.

1. **Start `scsynth` separately.** If `sclang` is allowed to boot the server, `scsynth` inherits sclang's UDP port 57120 and sclang never hears the server come up. `sc-play` therefore starts `scsynth -u 57110` itself and the patch talks to it with `Server.remote`.
2. **Call `s.initTree` in the patch** after connecting (see `home/patches/hello.scd`). Without it, synths are created but produce no sound.
3. **Set `SC_JACK_DEFAULT_OUTPUTS`** so the server connects to the DAC. PipeWire currently names the Katana sink `Built-in Audio Stereo`. `sc-play` exports:

       SC_JACK_DEFAULT_OUTPUTS="Built-in Audio Stereo:playback_FL,Built-in Audio Stereo:playback_FR"

   and runs `pw-jack taskset -c 3 scsynth -u 57110 -z 64 -o 2 -i 0`.

Play the test chord (default timeout 60 seconds):

```sh
sc-play /home/lab/patches/hello.scd
```

`sc-play` also sets `QT_QPA_PLATFORM=minimal` so sclang does not need a display.

## 8. Desktop and VNC

```sh
sudo apt-get install -y -q rpd-wayland-core rpd-preferences wayvnc
```

This pulls in labwc, LightDM, and related session pieces. Then configure autologin the way the running machine is set:

- systemd default target is `graphical.target`
- LightDM is the display manager. In `/etc/lightdm/lightdm.conf` under `[Seat:*]`:

      greeter-session=pi-greeter-labwc
      user-session=rpd-labwc
      autologin-user=lab
      autologin-session=rpd-labwc

  The first two may already be set by the package. The `autologin-*` lines were required.
- Install `etc/systemd/system/getty@tty1.service.d/autologin.conf` so tty1 also autologins as `lab`.

```sh
sudo install -d /etc/systemd/system/getty@tty1.service.d
sudo install -m 644 etc/systemd/system/getty@tty1.service.d/autologin.conf \
    /etc/systemd/system/getty@tty1.service.d/autologin.conf
```

### Why the system wayvnc is disabled

Pi OS ships `wayvnc.service` (runs as user `vnc`, via `/usr/sbin/wayvnc-run.sh`) and `wayvnc-control.service` (`/usr/sbin/wayvnc-control.py`). The control helper disconnects VNC clients whose username does not match the desktop session user. A Mac viewer connecting through an SSH tunnel has no such username, so the session is opened and immediately kicked. A brief experiment with `realvnc-vnc-server` (7.13.1.57) was also tried and then purged; do not install it.

```sh
sudo apt-get purge -y -q realvnc-vnc-server   # no-op if it was never installed
sudo systemctl disable --now wayvnc.service wayvnc-control.service
```

`/etc/wayvnc/config` on the running machine still exists and is bound to `127.0.0.1` with `enable_auth=false` (see `etc/wayvnc/config`). That file is only used if the system service is turned back on; the TLS key paths it names must never be committed. Install it as defense in depth:

```sh
sudo install -m 644 etc/wayvnc/config /etc/wayvnc/config
```

### User service (the working approach)

Copy the user config and unit, then enable the service. Linger (step 6) is required so the unit can start at boot:

```sh
sudo -u lab install -d /home/lab/.config/wayvnc /home/lab/.config/systemd/user
sudo -u lab install -m 644 home/.config/wayvnc/config \
    /home/lab/.config/wayvnc/config
sudo -u lab install -m 644 home/.config/systemd/user/wayvnc-lab.service \
    /home/lab/.config/systemd/user/wayvnc-lab.service
sudo -u lab XDG_RUNTIME_DIR=/run/user/$(id -u lab) \
    systemctl --user enable --now wayvnc-lab.service
```

`wayvnc-lab.service` waits for `$XDG_RUNTIME_DIR/wayland-0`, then runs `wayvnc --gpu` with the user config (`127.0.0.1:5900`, no auth). It is reachable only through an SSH tunnel.

Reboot once so LightDM autologin and the user unit both come up cleanly. Confirm:

```sh
systemctl is-enabled wayvnc.service wayvnc-control.service   # disabled
systemctl --user is-active wayvnc-lab.service                # active
ss -ltn | grep 5900                                          # 127.0.0.1:5900
```

### Mac viewer

TigerVNC Viewer is required. Copy `mac/pilab-desktop.command` to `~/bin/` and to the Desktop (or any convenient place), then double-click it. The script:

1. Opens `ssh -L 5901:127.0.0.1:5900 lab@pilab.local` if nothing is already listening on 5901
2. Launches `/Applications/TigerVNC.app/Contents/MacOS/vncviewer localhost::5901`
3. Closes the throwaway Terminal window that a `.command` file leaves behind

## 9. Package list

`packages/uname.txt` and `packages/apt-mark-showmanual.txt` are a snapshot of the running machine. `packages/key-packages.txt` lists versions of the stack above. `apt-mark showmanual` includes the base Lite image plus everything installed in these steps; use it as a reference, not as a blind `apt-get install` list.

## 10. What is intentionally not in this repo

- Disk images and SD-card snapshots (later work, stored outside git)
- SSH private keys, the live authorized public key, SSH host keys, wayvnc TLS certificates
- Radio configuration or any transmit setup
- Knowledge-base cards (owned separately)

Refresh the tracked files from a running Pi with `scripts/pull-from-pi.sh`.

## Faust

Console playback first. Install the JACK and ALSA headers, then the helper:

```sh
sudo apt-get install -y -q libjack-jackd2-dev libasound2-dev
sudo install -m 755 bin/faust-play /usr/local/bin/faust-play
```

`faust-play file.dsp [seconds]` builds the patch with `faust2jackconsole`, pins it to CPU 3, and links both channels with `pw-link`. Only `out_0` auto-connects to the DAC, so the helper always links `out_1` as well. A short test tone is in `home/faust/tone.dsp`.

### Faust GUI

For a Qt window on the labwc desktop plus a browser control page, install the Qt 5, HTTP, and OSC build deps and the helper:

```sh
sudo apt-get install -y -q qtbase5-dev qt5-qmake libmicrohttpd-dev liblo-dev
sudo install -m 755 bin/faust-gui /usr/local/bin/faust-gui
```

`qtwayland5` is already present with the desktop and is required for the Qt window to use Wayland. Copy `home/faust/synth.dsp` to `~/faust/synth.dsp` on the Pi.

Usage:

```sh
faust-gui ~/faust/synth.dsp
faust-gui file.dsp --no-qt      # no Qt window; web page and OSC only
faust-gui file.dsp --no-web     # Qt window and OSC, no HTTP page
faust-gui --stop
```

`faust-gui` builds with `faust2jaqt -httpd -osc`, caches the binary as `<name>-gui` next to the `.dsp` file, and rebuilds only when the patch or the build flags are newer. It joins the labwc session (`WAYLAND_DISPLAY=wayland-0`, `XDG_RUNTIME_DIR`, the session bus), starts the app with `QT_QPA_PLATFORM=wayland` (falls back to `xcb` on Xwayland if Wayland fails), runs it under `pw-jack` pinned to CPU 3, links both outputs to the DAC, and prints the web URL.

The HTTP control page listens on all interfaces at port 5510 by default:

    http://pilab.local:5510/

OSC is also enabled (UDP ports 5510–5512). Ctrl-C, or `faust-gui --stop`, quits a running instance.

`~/faust/synth.dsp` is a simple subtractive voice (sawtooth into a Moog-style low-pass, with an LFO on the cutoff). The gate checkbox defaults to off, so it starts silently. The Faust Qt window's default size can clip some knobs; drag a corner to resize, or use the browser page, which shows every control.

`pw-link` reports "File exists" when `out_0` has already auto-connected; the helper treats that as success so `out_1` still gets linked to the right channel.

## Cabbage (Csound instrument GUI)

Csound itself needs nothing extra: `pw-jack csound file.csd` with `-odac -+rtaudio=jack -b 64 -B 256` in `<CsOptions>` plays on the DAC (test file `home/csound/hello.csd`).

For instrument panels, Trillium Base runs the **Cabbage 2 IDE** (2.10.7, tag `v2-master-final`, commit `fe44ff9194373c883b60f118688ead06b519714d`) built from source on JUCE 6.1.2, linked against Debian's Csound 6.18 (`libcsound64`, double samples). There is no ARM Linux package. Cabbage 3 was not used: it is a VS Code extension plus a webview server and needs Csound 7.

Build and install (about 13 minutes with `-j3`, 850 MB of sources and objects in `~/src/cabbage2`, about 50 MB installed):

```sh
scp scripts/build-cabbage.sh patches/cabbage-linux-aarch64.patch lab@pilab.local:/tmp/
ssh lab@pilab.local 'sh /tmp/build-cabbage.sh /tmp/cabbage-linux-aarch64.patch'
ssh lab@pilab.local 'sudo tee /usr/local/bin/cabbage >/dev/null && sudo chmod 755 /usr/local/bin/cabbage' < bin/cabbage
ssh lab@pilab.local 'mkdir -p ~/.local/share/applications ~/csound && cat > ~/.local/share/applications/cabbage.desktop' < home/applications/cabbage.desktop
scp home/csound/*.csd lab@pilab.local:csound/
```

The script installs the apt build deps, clones Cabbage and JUCE, applies Cabbage's own JUCE patches (as its `build.py` does), applies `patches/cabbage-linux-aarch64.patch`, builds only the IDE target with Ninja, and installs:

- `/opt/cabbage/bin/Cabbage` (the IDE)
- `/usr/share/doc/cabbage/Examples` and `/usr/share/cabbage/Themes` (these paths are hard-coded in the Linux build)
- `/usr/share/icons/hicolor/512x512/apps/cabbage.png`

What the patch changes, and why:

1. **VST2 SDK.** `juce_set_vst2_sdk_path` fails configure when the SDK is absent, so it is skipped if missing, and VST2 plugin hosting is off (`JUCE_PLUGINHOST_VST=0`). VST3 hosting stays on.
2. **JACK.** JUCE 6 defaults to `JUCE_JACK=0` on Linux, so the IDE could only use ALSA. The patch sets `JUCE_JACK=1`. JUCE loads `libjack.so.0` at runtime, which `pw-jack` points at PipeWire.
3. **Autoplay.** With `CABBAGE_AUTOPLAY=1`, a `.csd` given on the command line is compiled and its instrument window opened 1.5 s after start, the same as pressing Play. Synthetic key presses (`wlrctl`) do not reach the Xwayland window, so this is the only reliable way to start an instrument without a mouse.

`build.py` is not used. It switches the checkout to the `develop` branch, downloads the VST SDK, CabbageRack (x86_64 only) and the manual, and builds every plugin target. No `-m64`/`-msse` changes were needed; JUCE 6.1.2 builds cleanly with GCC 14 on aarch64 apart from the juceaide C++17 flag the script adds. `libcsnd-dev` is required because Cabbage includes `csound.hpp`.

### Running it

```sh
cabbage                       # opens ~/csound/cabbage-demo.csd and starts it (detached)
cabbage ~/csound/other.csd    # any instrument; hands it to a running copy if one is open
cabbage file.csd -f           # stay in the foreground
CABBAGE_AUTOPLAY=0 cabbage    # open the editor without starting the instrument
cabbage --stop
```

The launcher joins the labwc session (`WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`, because JUCE 6 is X11-only and runs on labwc's Xwayland), makes sure `~/.config/Cabbage2/Cabbage2.settings` selects the **JACK** device at 48 kHz with the DAC as output (otherwise JUCE picks ALSA), starts Cabbage under `pw-jack` pinned to CPU 3, and links `JUCEJack:out_1/out_2` to the DAC's `playback_FL/FR`. JUCE also auto-connects `in_1` to the first capture port (the USB sound card); instruments that do not read input ignore it. A **Cabbage** entry also appears in the desktop menu. Log: `/tmp/cabbage.log`.

Do not run Cabbage on JUCE's ALSA device. The first test launch used ALSA (24 channels through `pipewire-alsa`, pinned to CPU 3), and the Pi rebooted about a minute later. The cause was not confirmed because the journal is not persistent, but the JACK setup has run stably since.

`home/csound/cabbage-demo.csd` is a two-saw voice through a Moog ladder filter, with Freq, Cutoff and Resonance knobs, a Volume slider capped at 0.1, and a Gate checkbox. The Gate starts off, so the instrument is silent until you tick it. In the instrument's `<CsOptions>`, `-n -d` is correct: the Cabbage host does the audio I/O, so do not add `-odac`.

## AirPlay receiver and sender (optional)

Full notes, ports, test results and licensing: [docs/airplay.md](./docs/airplay.md).

Build the receiver (nqptp 1.2.9 and shairport-sync 5.6.3 with AirPlay 2 and the PipeWire backend; about 1 minute):

```sh
scp scripts/build-airplay.sh lab@pilab.local:/tmp/ && ssh lab@pilab.local 'sh /tmp/build-airplay.sh'
```

Install the config, units and toggle:

```sh
scp etc/shairport-sync.conf lab@pilab.local:/tmp/ && ssh lab@pilab.local 'sudo install -m 644 /tmp/shairport-sync.conf /etc/shairport-sync.conf'
ssh lab@pilab.local 'mkdir -p ~/.config/systemd/user ~/.config/pipewire'
scp home/.config/systemd/user/shairport-sync.service home/.config/systemd/user/pilab-airplay-sender.service lab@pilab.local:.config/systemd/user/
scp home/.config/pipewire/pilab-airplay-sender.conf lab@pilab.local:.config/pipewire/
ssh lab@pilab.local 'sudo tee /usr/local/bin/pilab-airplay >/dev/null && sudo chmod 755 /usr/local/bin/pilab-airplay' < bin/pilab-airplay
ssh lab@pilab.local 'systemctl --user daemon-reload'
```

Both stay off until switched on:

```sh
pilab-airplay receiver on     # advertises "pilab"; plays into the default sink (the DAC)
pilab-airplay sender on       # also builds ~/.local/lib/pilab-pw-modules (HomePod User-Agent fix)
pilab-airplay status
```

Notes:

- Debian's `shairport-sync` package (4.3.7) is classic AirPlay only, and there is no `nqptp` package, so both come from source. `systemd-dev` is needed to build on Trixie. Its dry run installed only that package.
- shairport-sync must be a **user** service, because PipeWire is one. `nqptp` is a system service (UDP 319/320). The user unit waits for it, and the config pins `service_type = "airplay2"` so a race never silently produces a classic-only receiver.
- The sender runs as its own PipeWire client process, so switching it never restarts the realtime graph.

## Trillium Console (Trillium Synth and web console)

Needs PipeWire with `pipewire-jack` (step 6) and avahi. From the Mac, in this repo:

```sh
sh scripts/deploy-console.sh ~/path/to/zpod     # a zpod clone, or a bundle of the pinned commit
```

It copies the repo to `~/src/pilab` on the Pi (tar over ssh), sends the ZPOD crates as a
git bundle (the Pi has no GitHub credentials), and runs `scripts/install-console.sh`
there, which:

1. installs `pkg-config` and `libjack-jackd2-dev` if missing, and a minimal Rust
   toolchain with rustup in `~/.rustup` / `~/.cargo` if `cargo` is missing;
2. builds `pilabd` and `pilabctl` in release mode with `--locked`, using
   `~/.cache/pilab/zpod.git` as the ZPOD source and `~/.cache/pilab/target` as the build
   directory (about 2 minutes on a Pi 5 the first time);
3. installs both binaries to `~/.local/bin`;
4. installs `home/.config/systemd/user/pilabd.service`, enables and restarts it;
5. writes `/etc/avahi/services/pilab-console.service` with sudo, from `pilabd mdns-service`;
6. fetches AKWF into `~/.local/share/pilab/library/akwf` if it is not there
   (`scripts/fetch-akwf.sh`, about 6 MB of WAVs, CC0).

Check:

```sh
systemctl --user status pilabd
pilabctl status
pw-jack jack_lsp -c 'Trillium Synth'   # out_l/out_r linked to the DAC, MIDI sources on in_N
avahi-browse -rt _pilab._tcp
```

Then pair a browser with `trillium-base pair` (alias of `pilabctl pair`) and follow docs/console-test-plan.md.
Logs go to the journal (`journalctl --user -u pilabd` or
`journalctl _SYSTEMD_USER_UNIT=pilabd.service`).

