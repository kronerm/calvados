"""Offer a firmware package to AirPods and withdraw it before any firmware
data moves: the pods' answer to an offer, without updating them.

    python3 tools/firmware/uarp_offer.py MAC firmware.uarp [headers|metadata|full] [SECONDS]

Offers the SuperBinary (docs/uarp.md) and answers the pods' requests up to
a limit: `headers`, the SuperBinary header and payload headers; `metadata`,
also every payload's metadata (digests, personalization requests), i.e.
everything before the first payload's data; `full`, the whole package.
Below `full` it rescinds the offer at the first request beyond the limit,
which it leaves unanswered, and sends no firmware data. It never sends an
apply. The pods answer an offer while charging in the case, and keep a
withdrawn one until the case closes: close and reopen it before each run.

`full` is a firmware update: the primary stages the buds' image (FTAB)
in its inactive slot and reports "fully staged"; the tool stops there.
With both buds in the case, the case then copies the image to the other
bud and applies both (docs/sibling-and-case.md); `uarp_probe.py` shows the
staged, later the running version. A stall (no request for a minute)
withdraws the offer; rerun after closing and reopening the case.
"""
import struct
import sys
import time

import dbus
import dbus.mainloop.glib
from gi.repository import GLib

from uarp_probe import GATT, LAST, characteristic

ASSET = 1
# Seconds without a data request after which a transfer counts as stalled.
STALL = 60
# Generous for the GATT MTU (BlueZ reported 672); the header byte comes on top.
CHUNK = 500

NAMES = {
    0x00: "sync", 0x02: "version", 0x05: "offer", 0x06: "data request",
    0x09: "processing", 0x0D: "offer ack", 0x0E: "pause/resume ack",
    0x10: "rescind ack", 0x11: "solicitation", 0x13: "solicitation result",
}


class Session:
    def __init__(self, char, package, stage):
        self.char, self.package = char, package
        _, _, self.length = struct.unpack_from(">3I", package)
        self.version = package[12:28]
        _, _, headers_at, headers_length = struct.unpack_from(">4I", package, 28)
        self.payloads = headers_length // 0x28
        # Payload headers: [length 4] [tag 4] [version 16] [metadata at, length]
        # [data at, length]; the data starts after all metadata.
        data = min(struct.unpack_from(">I", package, headers_at + i * 0x28 + 32)[0]
                   for i in range(self.payloads))
        self.limit = {
            "headers": headers_at + headers_length,
            "metadata": data,
            "full": len(package),
        }[stage]
        self.full = stage == "full"
        self.last_request = None
        self.done = None
        self.msg_id = 0
        self.pending = bytearray()
        self.rescinded = False
        self.served = 0

    def send(self, kind, payload=b""):
        self.msg_id += 1
        message = struct.pack(">HHH", kind, len(payload), self.msg_id) + payload
        if not (self.full and kind == 0x07):
            print(f"-> {NAMES.get(kind, hex(kind))}: {message.hex(' ')[:120]}", flush=True)
        chunks = [message[i:i + CHUNK] for i in range(0, len(message), CHUNK)]
        for i, chunk in enumerate(chunks):
            header = LAST if i == len(chunks) - 1 else 0
            self.char.WriteValue(dbus.Array(bytes([header]) + chunk, signature="y"),
                                 {"type": "request"})

    def offer(self):
        self.send(0x00)
        self.send(0x01, struct.pack(">H", 4))
        self.send(0x05, struct.pack(">IHH", 0, 0x0001, ASSET) + self.version
                  + struct.pack(">IH", self.length, self.payloads))

    def received(self, raw):
        self.pending.extend(raw[1:])
        if not raw[0] & LAST:
            return
        message, self.pending = bytes(self.pending), bytearray()
        kind, length, msg_id = struct.unpack_from(">HHH", message)
        body = message[6:6 + length]
        if not (self.full and kind == 0x06):
            print(f"<- {NAMES.get(kind, hex(kind))} (id {msg_id}): {body.hex(' ')}", flush=True)
        if kind == 0x06:
            self.data_request(body)
        elif kind == 0x09:
            # Processing notification: acknowledge it, as a controller does.
            self.send(0x0F, body[:4])
            self.processed(struct.unpack_from(">H", body, 2)[0])

    def processed(self, flags):
        """1 staged; 2, 3, 4 with a reason above: denied, abandoned, corrupt."""
        outcome = {1: "fully staged", 2: "denied", 3: "abandoned", 4: "corrupt"}
        print(f"   {outcome.get(flags & 0xFF, 'processing')} (flags {flags:#06x})", flush=True)
        if flags & 0xFF in outcome:
            self.done = flags & 0xFF

    def data_request(self, body):
        asset, offset, wanted = struct.unpack_from(">HIH", body)
        if self.rescinded:
            return
        if asset != ASSET or offset + wanted > self.limit:
            print(f"   beyond the limit {self.limit:#x} ({offset:#x}+{wanted}): rescinding",
                  flush=True)
            self.rescinded = True
            self.send(0x0C, struct.pack(">H", ASSET))
            return
        data = self.package[offset:offset + wanted]
        self.last_request = time.monotonic()
        before = self.served * 20 // self.length
        self.served += len(data)
        if self.full and self.served * 20 // self.length > before:
            print(f"   {self.served * 100 // self.length}% of the package's size served"
                  f" (at {offset:#x})", flush=True)
        self.send(0x07, struct.pack(">HHIHH", 0, asset, offset, wanted, len(data)) + data)


def main(address, path, stage="headers", wait="30"):
    package = open(path, "rb").read()
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    char_path = characteristic(bus, address)
    char = dbus.Interface(bus.get_object("org.bluez", char_path), GATT)
    session = Session(char, package, stage)
    loop = GLib.MainLoop()

    def changed(interface, props, _invalidated):
        if interface == GATT and "Value" in props:
            session.received(bytes(props["Value"]))

    bus.add_signal_receiver(changed, "PropertiesChanged",
                            "org.freedesktop.DBus.Properties", path=char_path)
    char.StartNotify()
    GLib.timeout_add(500, lambda: session.offer() and False)

    def finish():
        # A staged image stays: that's the update.
        if not session.rescinded and session.done != 1:
            session.rescinded = True
            session.send(0x0C, struct.pack(">H", ASSET))
        # Long enough for the rescind's acknowledgement (0x10).
        GLib.timeout_add_seconds(3, loop.quit)
        return False

    def watch():
        stalled = (session.last_request is not None
                   and time.monotonic() - session.last_request > STALL)
        if session.done is not None or stalled:
            if stalled:
                print(f"   no request for {STALL} s: withdrawing", flush=True)
            finish()
            return False
        return True

    GLib.timeout_add_seconds(int(wait), finish)
    GLib.timeout_add_seconds(1, watch)
    try:
        loop.run()
    finally:
        char.StopNotify()
        print(f"served {session.served} bytes, all before {session.limit:#x}")


if __name__ == "__main__":
    main(*sys.argv[1:5])
