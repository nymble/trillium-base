<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Plugging in USB audio and MIDI devices

What the unit does when an audio interface, microphone or MIDI controller is plugged in
while it is running. In short: **new devices are found and shown, but audio devices are
never used for the synth unless you choose them.** MIDI controllers are connected to the
synth's MIDI inputs automatically.

## Audio interfaces (Scarlett 2i2, USB mics, …)

| What | Happens on plug-in |
|---|---|
| Kernel | The card appears (`/proc/asound/cards`). |
| PipeWire / WirePlumber | Creates its outputs (sinks) and inputs (sources) within a second or two. |
| Default output | Stays the DAC. `~/.config/wireplumber/wireplumber.conf.d/51-pilab-dac.conf` (written by `scripts/install-dac-pin.sh` from the `audio.dac_sink` setting) gives the DAC, matched by node **name** (not card number), session and driver priority 3000; USB devices get about 1000. |
| Synth output | Never moves. pilabd links the synth's outputs to the configured port names (`audio.output_ports`). If the DAC node disappears and comes back (WirePlumber restart, replug), pilabd re-links it by name within about 2 s and logs "DAC reappeared". |
| Synth clock | The DAC drives the synth's graph. When graphs join (the console's meters tap a USB device, or music plays on DAC and USB together), the DAC stays the driver and the USB device follows, resampled by PipeWire. |
| Input strip | Connects only the configured source (`input.source`, a glob). A new microphone or interface input is not used until you pick it on the Devices page. |
| Music player | Keeps playing where it was going (the DAC by default). The new output appears in the Music page's output picker and the CLI's `trillium-base music outputs`, with an event "audio output added: …". |
| Not picked up | If the kernel has a USB card that PipeWire has not set up after about 10 s, pilabd logs a warning and the output picker shows it. Fix: replug, or `systemctl --user restart wireplumber` (the synth re-links by itself). |

Seen on a Pi with a WirePlumber that had been up for five days: a Scarlett 2i2 plugged in
appeared in the kernel but not in PipeWire until WirePlumber was restarted. The warning
above exists for that case.

## MIDI controllers

The MIDI router (`midi.autoconnect`, on by default) connects every new MIDI source to the
next free synth MIDI input (1-7), except sources matching `midi.ignore`
(default `*Midi Through*`). Removing a device frees its input. Turn `midi.autoconnect` off
to connect controllers by hand on the MIDI page. Bluetooth MIDI works the same way
(see [bluetooth-midi.md](bluetooth-midi.md)).

## Focusrite Scarlett 2i2 (3rd gen), as measured on the Pi

- USB 1235:8210, class-compliant; the kernel's Scarlett Gen 2/3 mixer driver is loaded.
- Playback: 2 channels (Line/Headphones 1-2), S32_LE (24-bit), 44.1 / 48 / 88.2 / 96 /
  176.4 / 192 kHz. Capture: 2 channels (Mic/Line 1 and 2), same formats.
- PipeWire (UCM "HiFi" profile): one stereo sink, "Scarlett 2i2 3rd Gen Headphones / Line
  1-2", and two mono sources (Mic1, Mic2). Runs at the graph's 48 kHz.
- No MIDI ports.
- Use: a second music output (Music page → output picker), or an input for recording
  (Devices page → input source). It never takes over the synth on its own.
