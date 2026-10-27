"""Ask AirPods about their firmware over UARP inside AACP (opcode 0x4F),
read-only: the route an Apple host uses (docs/uarp.md).

    python3 tools/firmware/uarp_aacp_probe.py AA:BB:CC:DD:EE:FF

Opens its own AACP channel (L2CAP PSM 0x1001), so stop the calvados daemon
first. Sends the AACP handshake, then Sync, version discovery and the
information requests (tags 1–9) as 0x4F frames, nothing that offers,
transfers or applies firmware. A 0x4F frame reaching a bud that isn't
primary hits an abort in the firmware: don't move the buds meanwhile.
"""
import socket
import struct
import sys
import time

from uarp_probe import describe, message

PSM = 0x1001
HANDSHAKE = bytes.fromhex("00000400010002000000000000000000")
DATA = bytes.fromhex("04000400")
UARP_OPCODE = 0x4F


def frame(uarp):
    return DATA + struct.pack("<HH", UARP_OPCODE, len(uarp)) + uarp


def uarp_messages(data):
    """The UARP messages in received AACP frames (several may come at once)."""
    while (at := data.find(DATA + b"\x4f\x00")) >= 0:
        length = struct.unpack_from("<H", data, at + 6)[0]
        yield data[at + 8:at + 8 + length]
        data = data[at + 8 + length:]


def main(address):
    sock = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_SEQPACKET, socket.BTPROTO_L2CAP)
    sock.settimeout(5)
    sock.connect((address, PSM))
    sock.send(HANDSHAKE)
    time.sleep(1)
    queries = [message(0, 1), message(1, 2, struct.pack(">H", 9))]
    queries += [message(3, 3 + tag, struct.pack(">I", tag)) for tag in range(1, 10)]
    for query in queries:
        sock.send(frame(query))
        time.sleep(0.3)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        try:
            received = sock.recv(4096)
        except socket.timeout:
            break
        for uarp in uarp_messages(received):
            print(describe(uarp), flush=True)
    sock.close()


if __name__ == "__main__":
    main(sys.argv[1])
