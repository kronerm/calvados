"""Ask AirPods about their firmware over UARP (docs/uarp.md), read-only.

    python3 tools/firmware/uarp_probe.py AA:BB:CC:DD:EE:FF

Sends Sync, version discovery and the information requests (tags 1–9),
nothing that offers, transfers or applies firmware. Needs the pods
connected (BlueZ serves their GATT over the classic link), dbus-python and
PyGObject.
"""
import struct
import sys

import dbus
import dbus.mainloop.glib
from gi.repository import GLib

UARP = "94110001-6d9b-4225-a4f1-6a4a7f01b0de"
GATT = "org.bluez.GattCharacteristic1"
LAST = 0x01  # GATT fragment header: the message's last fragment
STRING_TAGS = {1: "manufacturer", 2: "model", 3: "serial", 4: "hardware", 9: "tag 9"}
VERSION_TAGS = {5: "running firmware", 6: "staged firmware", 7: "statistics", 8: "last error"}


def characteristic(bus, address):
    device = "/dev_" + address.upper().replace(":", "_")
    objects = dbus.Interface(bus.get_object("org.bluez", "/"),
                             "org.freedesktop.DBus.ObjectManager").GetManagedObjects()
    for path, interfaces in objects.items():
        if device in path and interfaces.get(GATT, {}).get("UUID") == UARP:
            return path
    sys.exit(f"no UARP characteristic for {address}: connected?")


def message(kind, msg_id, payload=b""):
    return struct.pack(">HHH", kind, len(payload), msg_id) + payload


def describe(data):
    kind, length, _ = struct.unpack_from(">HHH", data)
    body = data[6:6 + length]
    if kind == 2:
        return f"protocol version {struct.unpack_from('>H', body, 2)[0]}"
    if kind == 4 and len(body) >= 10:
        tag, size = struct.unpack_from(">II", body, 2)
        value = body[10:10 + size]
        if tag in STRING_TAGS:
            return f"{STRING_TAGS[tag]}: {value.decode('ascii', 'replace')!r}"
        words = struct.unpack(f">{size // 4}I", value[:size // 4 * 4])
        return f"{VERSION_TAGS.get(tag, f'tag {tag}')}: {'.'.join(map(str, words))}"
    return f"type {kind:#06x}: {body.hex(' ')}"


def main(address):
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    path = characteristic(bus, address)
    char = dbus.Interface(bus.get_object("org.bluez", path), GATT)
    loop = GLib.MainLoop()
    pending = bytearray()

    def changed(interface, props, _invalidated):
        if interface != GATT or "Value" not in props:
            return
        raw = bytes(props["Value"])
        pending.extend(raw[1:])
        if raw[0] & LAST:
            print(describe(bytes(pending)), flush=True)
            pending.clear()

    bus.add_signal_receiver(changed, "PropertiesChanged",
                            "org.freedesktop.DBus.Properties", path=path)
    char.StartNotify()
    queue = [message(0, 1), message(1, 2, struct.pack(">H", 9))]
    queue += [message(3, 3 + tag, struct.pack(">I", tag)) for tag in range(1, 10)]

    def send():
        if not queue:
            GLib.timeout_add_seconds(2, loop.quit)
            return False
        data = bytes([LAST]) + queue.pop(0)
        char.WriteValue(dbus.Array(data, signature="y"), {"type": "request"})
        return True

    GLib.timeout_add(500, send)
    GLib.timeout_add_seconds(20, loop.quit)
    try:
        loop.run()
    finally:
        char.StopNotify()


if __name__ == "__main__":
    main(sys.argv[1])
