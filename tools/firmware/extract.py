"""Extract a UARP SuperBinary's payloads and their rkosftab entries.

    python3 -I extract.py firmware.uarp OUTDIR

Writes OUTDIR/<payload tag>/payload.bin and, for firmware tables (FTAB,
R1FW, STFC, STFW), OUTDIR/<payload tag>/<entry tag>.bin. Formats:
docs/firmware.md.
"""
import os
import struct
import sys

FTAB_MAGIC = b"rkosftab"


def payloads(blob):
    """SuperBinary header (big-endian): payload headers at +36, each
    [length 4] [tag 4] … [offset 4] [size 4] at +32."""
    first, total = struct.unpack_from(">2I", blob, 36)
    at = first
    while at < first + total:
        length, tag = struct.unpack_from(">I4s", blob, at)
        offset, size = struct.unpack_from(">2I", blob, at + 32)
        yield tag.decode("ascii", "replace"), blob[offset:offset + size]
        at += length


def entries(table):
    """rkosftab (little-endian): magic at +0x20, 16-byte entries from +0x30,
    [tag 4] [offset 4] [size 4] [pad 4]; empty entries (sbd2) are kept."""
    if table[0x20:0x28] != FTAB_MAGIC:
        return
    for at in range(0x30, len(table) - 15, 16):
        tag, offset, size, _ = struct.unpack_from("<4sIII", table, at)
        if not all(32 <= c < 127 for c in tag):
            return
        yield tag.decode(), table[offset:offset + size]


def main(uarp, out):
    for tag, data in payloads(open(uarp, "rb").read()):
        folder = os.path.join(out, tag)
        os.makedirs(folder, exist_ok=True)
        open(os.path.join(folder, "payload.bin"), "wb").write(data)
        parts = [(t, d) for t, d in entries(data) if d]
        print(f"{tag} {len(data)}" + "".join(f"\n  {t} {len(d)}" for t, d in parts))
        for entry, part in parts:
            open(os.path.join(folder, entry + ".bin"), "wb").write(part)


if __name__ == "__main__":
    main(*sys.argv[1:3])
