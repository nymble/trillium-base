<!-- Copyright (C) 2026 M-of-N -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Bluetooth LE MIDI

## What is in place (checked 2026-11-20 on the lab unit)

| Piece | State |
| --- | --- |
| BlueZ | 5.82 (Raspberry Pi build). `bluetoothd` is built with MIDI support (`--enable-midi`, links the ALSA sequencer MIDI event coder), so BLE MIDI *peripherals* the Pi connects to as central (keyboards, controllers) appear as MIDI ports |
| PipeWire / WirePlumber | The bluez5 plugin runs a **BLE-MIDI GATT server**: the BLE-MIDI service UUID `03b80e5a-ede8-4b33-a751-6ce34ec4c700` is registered with BlueZ and shows up as the PipeWire node `bluez_midi.server` ("BLE MIDI 1", `Midi/Bridge`) |
| Trillium Synth | pilabd autoconnects `BLE MIDI 1:out -> Trillium Synth:in_1` (MIDI input 1); it is visible and re-patchable in the console patch panel |
| Advertising | **Not advertised by default**: the GATT service exists but no LE advertisement, so a Mac or iPhone cannot discover the unit. `tools/ble-midi-advertise.py` (installed as `/usr/local/sbin/trillium-ble-midi`) adds a time-boxed advertisement and pairing agent |
| Adapter | Powered on, not discoverable (classic), pairable; `bluetooth.service` enabled. "Bluetooth off by default" is a pending item for the image |

## Connecting a Mac (the unit as BLE-MIDI peripheral)

1. On the unit, open a pairing window (10 minutes by default):

   ```
   sudo trillium-ble-midi --minutes 15
   ```

   It prints `advertising BLE MIDI as "Trillium Base"`. Leave it running; Ctrl-C ends it early.
2. On the Mac: **Audio MIDI Setup** → **Window → Show MIDI Studio** → click the
   **Bluetooth** button in the toolbar ("Configure Bluetooth").
3. "Trillium Base" appears in the list → **Connect**. It then shows as a Bluetooth device in
   MIDI Studio. If macOS asks to pair, accept (the unit accepts "just works" pairing only
   while the window is open).
4. Send notes to the "Trillium Base" MIDI destination from any MIDI app (Logic, Ableton, a
   virtual keyboard app). From Terminal, with `sendmidi` (Homebrew `gbevin/tools/sendmidi`):

   ```
   sendmidi dev "Trillium Base" on 60 100
   sendmidi dev "Trillium Base" off 60 0
   ```
5. Trillium Console shows the notes on MIDI input 1 (MIDI activity) and the synth plays
   through the DAC. Pairing persists, so later connections only need step 1 (the
   advertisement) if the Mac has to find the unit again.

## Connecting an iPhone or iPad

iOS has no system-wide BLE-MIDI pairing screen; each app connects. Open the pairing window
on the unit (step 1 above), then in a MIDI controller app use its Bluetooth MIDI device
list (often "Bluetooth MIDI Devices" in its MIDI settings) and pick "Trillium Base". The
app's MIDI output then reaches Trillium Synth the same way.

## Notes

- The pairing window accepts any device in radio range for its duration; that is why it is
  time-boxed and off by default.
- `bluetoothctl show` → "Advertising Features: ActiveInstances" is 1 while the window is
  open and 0 otherwise.
- A BLE MIDI keyboard the Pi connects to (Pi as central) is handled by BlueZ/PipeWire as a
  separate `bluez_midi` node and is autoconnected to the next free synth input.

## Planned

- Bluetooth off by default in the image (`AutoEnable=false`), powered on only for a pairing
  window or when the user enables BLE MIDI in the console.
- A console switch for the pairing window (calls the same advertisement code).
