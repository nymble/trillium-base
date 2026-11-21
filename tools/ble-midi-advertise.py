#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Advertise this unit as a Bluetooth LE MIDI peripheral for a limited time.

PipeWire's bluez5 plugin (WirePlumber, "BLE MIDI 1" / bluez_midi.server) already registers
the BLE-MIDI GATT service with BlueZ, but nothing advertises it, so a Mac or iPhone cannot
find the unit. This tool registers an LE advertisement carrying the BLE-MIDI service UUID
and the product name, plus a temporary "just works" pairing agent, then removes both when
the time runs out or on Ctrl-C / SIGTERM.

    sudo tools/ble-midi-advertise.py [--minutes 10] [--name "Trillium Base"]

While it runs, anyone in radio range can pair and send MIDI, which is why it is time-boxed
and off by default. If the Bluetooth adapter is off (the Trillium Base image boots with
Bluetooth off), it is powered on and stays on after the window closes, so a paired Mac or
iPhone keeps its MIDI connection; `bluetoothctl power off` or a reboot turns it off again.
"""
import argparse
import signal
import sys

import dbus
import dbus.exceptions
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

BLUEZ = "org.bluez"
MIDI_SERVICE = "03b80e5a-ede8-4b33-a751-6ce34ec4c700"
ADV_PATH = "/org/trillium/ble_midi/adv0"
AGENT_PATH = "/org/trillium/ble_midi/agent"


class Advertisement(dbus.service.Object):
    IFACE = "org.bluez.LEAdvertisement1"

    def __init__(self, bus, name):
        self.name = name
        super().__init__(bus, ADV_PATH)

    def props(self):
        return {
            "Type": dbus.String("peripheral"),
            "ServiceUUIDs": dbus.Array([MIDI_SERVICE], signature="s"),
            "LocalName": dbus.String(self.name),
            "Discoverable": dbus.Boolean(True),
            "Includes": dbus.Array(["tx-power"], signature="s"),
        }

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        if iface != self.IFACE:
            raise dbus.exceptions.DBusException("org.freedesktop.DBus.Error.InvalidArgs", "no such interface")
        return self.props()

    @dbus.service.method(IFACE, in_signature="", out_signature="")
    def Release(self):
        print("advertisement released by BlueZ")


class JustWorksAgent(dbus.service.Object):
    """NoInputNoOutput agent: accepts pairing requests while the tool runs."""

    IFACE = "org.bluez.Agent1"

    @dbus.service.method(IFACE, in_signature="", out_signature="")
    def Release(self):
        pass

    @dbus.service.method(IFACE, in_signature="os", out_signature="")
    def AuthorizeService(self, device, uuid):
        print(f"authorize {uuid} for {device}")

    @dbus.service.method(IFACE, in_signature="o", out_signature="")
    def RequestAuthorization(self, device):
        print(f"pairing accepted: {device}")

    @dbus.service.method(IFACE, in_signature="ou", out_signature="")
    def RequestConfirmation(self, device, passkey):
        print(f"pairing confirmed: {device}")

    @dbus.service.method(IFACE, in_signature="", out_signature="")
    def Cancel(self):
        print("pairing cancelled")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--minutes", type=float, default=10.0)
    ap.add_argument("--name", default="Trillium Base")
    ap.add_argument("--adapter", default="/org/bluez/hci0")
    a = ap.parse_args()

    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    adapter = bus.get_object(BLUEZ, a.adapter)
    props = dbus.Interface(adapter, dbus.PROPERTIES_IFACE)
    if not props.Get("org.bluez.Adapter1", "Powered"):
        print("bluetooth adapter was off: powering it on (stays on until `bluetoothctl power off` or reboot)")
        props.Set("org.bluez.Adapter1", "Powered", dbus.Boolean(True))
    props.Set("org.bluez.Adapter1", "Pairable", dbus.Boolean(True))

    adv = Advertisement(bus, a.name)
    agent = JustWorksAgent(bus, AGENT_PATH)
    agents = dbus.Interface(bus.get_object(BLUEZ, "/org/bluez"), "org.bluez.AgentManager1")
    advm = dbus.Interface(adapter, "org.bluez.LEAdvertisingManager1")
    loop = GLib.MainLoop()

    agents.RegisterAgent(AGENT_PATH, "NoInputNoOutput")
    agents.RequestDefaultAgent(AGENT_PATH)
    advm.RegisterAdvertisement(ADV_PATH, {}, reply_handler=lambda: print(f"advertising BLE MIDI as \"{a.name}\" for {a.minutes:g} min"),
                               error_handler=lambda e: (print(f"advertisement failed: {e}"), loop.quit()))

    def stop(*_):
        loop.quit()
        return False

    GLib.timeout_add(int(a.minutes * 60_000), stop)
    for s in (signal.SIGINT, signal.SIGTERM):
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, s, stop)
    loop.run()
    for fn in (lambda: advm.UnregisterAdvertisement(ADV_PATH), lambda: agents.UnregisterAgent(AGENT_PATH)):
        try:
            fn()
        except dbus.exceptions.DBusException:
            pass
    print("stopped advertising; pairing agent removed")
    del agent


if __name__ == "__main__":
    main()
