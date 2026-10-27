# AirPods firmware notes

What the pods' own firmware says about the protocols calvados speaks, so
that it is checkable and others can carry on. Every claim names the build,
the image and the offset it was read from. Same confidence levels as
AGENTS.md: *Known* (read from code or data), *Observed* (seen on real pods),
*Inferred* (a guess; marked).

Apple's firmware is not in this repository; `tools/firmware/` fetches and
unpacks it. Prior work this builds on: [openulla] (the ULLA link, the AAP
framing, the AACP dispatch table; same build), [uarp-dump] / [ftab-dump]
(container formats), [linden/uarp] (SuperBinary metadata ids).

[openulla]: https://github.com/mstampfli/openulla
[uarp-dump]: https://github.com/19h/uarp-dump
[ftab-dump]: https://github.com/19h/ftab-dump
[linden/uarp]: https://github.com/linden/uarp

## Reproducing

```sh
tools/firmware/fetch.sh A3048                  # -> firmware/A3048/, prints hashes
tools/firmware/ghidra/import.sh firmware/A3048/parts/FTAB/rkof.bin
tools/firmware/ghidra/run.sh dec 0x6cf48       # decompile; modes in Dump.java
```

Apple's catalog (`mesu.apple.com/assets/com_apple_MobileAsset_UARP_<model>/`)
serves only the current build, so compare the hashes below before trusting
an offset. Older builds' URLs are listed by [uarp-dump] for some models.

## Builds analyzed

| | |
|---|---|
| Product | AirPods Pro 2 (USB-C), `AirPods3,1`, board `b698ap`, chip `0x2016` (H2) |
| Asset | `com_apple_MobileAsset_UARP_A3048` (A3048 the right bud; A2968 the case serves the same build; A3047, the left bud, returns 403) |
| Build | 9A348 = 9.0.348, train Gong, `SourceVersion` 3431000025000000, `BuildVersion` 6807, dated 2026-08-17 |
| Package | `…/MobileAssets2026/patches/082-16653/28D7B0C9-CA9C-4B3C-A8EE-41715E2763E6/com_apple_MobileAsset_UARP_A3048/5d0896d881bbc7e321f89b4ada11b31cf8b5569c.zip` on `updates.cdn-apple.com` |

BLAKE3 (`b3sum`):

| File | BLAKE3 |
|---|---|
| package zip | `be951de9e2e3aa3dc811e6d7777f725819d2496b9bcd36f91653978ddf3070d4` |
| `firmware.uarp` | `1d7f835c7b59878bb0d0d965d043268750f4b328cece18851ef960e9c3383811` |
| `FTAB/payload.bin` | `b1ad2b89493615c68d30145e52f0eec9b378b37fbb7ddb3288ab9dc905b29f03` |
| `rkof` | `eaac97a951cb077d1d32ef914292174cacf0382700a8027c26eb5b217a3c7b8c` |
| `acib` | `77a397e072ca043eb8570eec39a791ea2ef04aef3a4b1672cfb2d7ef44ce0d37` |
| `sbd1` | `3789f2129337ec840258e06dd8f53d11d5fd409a2b7315503c8f3563bb592a37` |
| `aopf` | `caf42a42e2f51ea13f5cd827befe13205e4a606fde06e17e9506c3d8c9c4e240` |
| `rkos` | `1db6c5d45a80dd6c476f23c3103ddd6612b522eef815a0bf9b46f2951f6b534a` |
| `rtpf` | `a7ae5f8da27c59810dc85e55318969deb0496af6dd94080188c620313acdd930` |

The pods tested with calvados (Observed, over GATT Device Information and
AACP `0x1D`) are older: model A3048 (the primary at the time), hardware
`3.36.32`, firmware `71.2256000021000000.6663`, i.e. 7.1. Offsets here are
from 9A348; the formats checked against those pods are marked Observed.
UARP names that build: running firmware 7.1.21.0, i.e. 7B21
(`tools/firmware/uarp_probe.py`, read-only).

### Version strings

`<major><minor>.<SourceVersion>.<BuildVersion>`: 9A348 is
`90.3431000025000000.6807` (its `bver` entry: `…|date:2026 8 17|updv:348`).
The same string is GATT `2A26` and AACP `0x1D` slot 4.

## Containers

- `firmware.uarp`: a UARP SuperBinary, big-endian. Payload headers start at
  the offset in the header's +36 (length at +40); each is `[length 4]
  [tag 4] …` with the payload's offset and size at +32. Payloads (9A348):
  `AUxx` (audio assets), `CMAP`, `CTAB`, `FTAB`, `NERF`, `R1FW` (recovery),
  `STFC`, `STFW`, `TMAP`.
- `FTAB` (and `R1FW`, `STFC`, `STFW`): `rkosftab` tables, little-endian;
  magic at +0x20, 16-byte entries from +0x30: `[tag 4] [offset 4] [size 4]
  [pad 4]`, offsets from the table's start. `sbd2` is an empty entry
  mid-table (stopping at the first empty entry misses `rkof`).
- `BuildManifest.plist`: Image4, every entry `Personalize = True` (see the
  update path below).

### Images in `FTAB`

| Tag | Size | What |
|---|---|---|
| `rkof` | 2088320 | RTKitOS features: the application. Bluetooth host stack, AAP/AACP, audio manager, A2DP, UARP, crash logs. Everything below is in here unless named otherwise. |
| `acib` | 1066208 | the Bluetooth controller (LMP, the radio) |
| `sbd1` | 1462016 | DSP (audio graph, ULLA receive clients) |
| `aopf` | 596768 | always-on processor: sensors, charging, battery gauge |
| `rkos` | 134176 | RTKitOS |
| `rtpf` | 201312 | real-time processor |
| `phyb`, `apmu`, `btpw`, `dstn`, `ls*`, `L*`, `lpda`, `bver` | | PHY, PMU, power tables, DSP assets, version |

All code images: ARM Thumb-2 (Cortex-M), little-endian; analyzed loaded at
0, so addresses are file offsets. Strings include ~270 source file names
(assert paths), which is how modules are found.

## Other models and builds

Every model has its own asset (`A2618` is the Lightning AirPods Pro 2;
cases have their own; other generations use H1, a different chip), and the
layout moves between builds. To find a landmark in another build, search
for what's named under "found by", not the address.

## AAP and AACP (`rkof`, 9A348)

AAP runs on L2CAP PSM 0x1001: `[frame type 2] [service 2]` + body, type 4
data; AACP is **service 4** (our handshake is openulla's "open service 4"),
the sibling bud is service 0x10 (openulla).

| What | Where | Found by |
|---|---|---|
| AACP receive: opcode dispatch, `TBH` over opcodes 1–0x64 | `0x6cf48`, table `0x6cf4c` | openulla 2.4; `dfe8 10f0` (`tbh [pc, r0, lsl #1]`) after `cmp r0, #0x64` |
| shared "ignore" handler (39 opcodes) | `0x6d218` | most common table target |
| AACP send: `(session, buffer, length incl. opcode, opcode)` | `0x6e55c` | callee of the status reply `0x6e9cc` (opcode 0x37) |

Opcodes with an inbound handler (Known): `01 03 05 07 09 0a 0d 0f 10 12 14
15 16 1a 1b 1e 1f 20 21 22 24 25 26 29 2a 2c 2d 30 31 35 36 38 3a 3c 3e 3f
40 42 43 44 45 46 48 4a 4b 4c 4d 4e 4f 52 53 54 56 57 58 59 60 61 62 63
64`. Everything calvados sends (`09 0f 10 29 30 4d`) is handled.

### Frames the pods send

From the callers of `0x6e55c` (lengths include the 2-byte opcode; a frame
on the wire is the 4-byte AAP header plus that). These are what
`protocol::frame_length` uses.

| Opcode | Sender | Body after the opcode | Status |
|---|---|---|---|
| 0x02 | `0x85620` | `[count]` + `[id] [value]` records: 4-byte value for ids 3–8 and 0x30, 1 byte for the other known ids and 0x80–0xF0, none otherwise | Observed (29 records on 7.1, 38 in 9A348); left variable |
| 0x04 | `0x86e94` | `[count]` + 5 bytes each | Observed |
| 0x06 | `0x6e72c` | 2 | Observed |
| 0x08 | `0x6e7a0` | 4 | Observed |
| 0x09 | `0x6e804` | 5 | Observed |
| 0x0C | `0x874e0` | 8 | Observed |
| 0x0E | `0x875e8` | 7 | Observed |
| 0x11 | `0xcb5e8` | `[mac 6] [x] [len 2 LE]` + len | Observed |
| 0x13 | `0x6e854` | 7 | Known |
| 0x15 | `0x87788` | 6 | Known |
| 0x19 | `0x6e8a8` | 2 | Observed |
| 0x1D | `0x88734` | `02 [n 2 LE] [2 bytes]` + 15 slots (n counts from byte 3); see below | Known; strings Observed |
| 0x21 | `0x88650` | `01 [x] [len] 00` + len | Known |
| 0x25, 0x47 | `0xcc884`, `0xd5560` | none | Known |
| 0x2B | `0x6cc22` | history, see AGENTS.md | Observed |
| 0x2E | `0xb72a4` | `01 [x] [count]` + 8 bytes each | Observed |
| 0x31 | `0x6e92c` | `[count]` + `[type 2 LE] [len 2 LE]` + key | Known; Observed for count 2 |
| 0x37 | `0x6e9cc` | status, 4 (openulla) | Known |
| 0x40 | `0x6df48` | 6 | Known |
| 0x4B, 0x55, 0x60 | `0x95b1c`, `0xd31a8`, `0xd3cac` | 4 | 0x4B, 0x55 Observed |
| 0x4C, 0x52, 0x57 | `0xc84e0`, `0xd16ec`, `0x6ea90` | `[len 2 LE]` + len | 0x52 Observed (`03 00 02 01 xx`) |
| 0x4E | `0x88d40` | 8 | Observed |
| 0x53 | `0x88df8` | 134 (EQ) | Observed |
| 0x62, 0x63 | `0xc8884`, `0x6eb5e` | no length field found; left variable | |
| 0x17 | `0x6e198` (own sender) | `[class 4 LE] [len 2 LE]` + len (class is 2 bytes for AAP versions ≤ 1.1; openulla 2.6) | Observed |

`0x1D` slots, each NUL-terminated, a lone NUL when empty; slots 11 and 12
are 16 binary bytes (+ NUL), so a string parser must stop before them
[`0x88734`: lengths `0x10` for indices 0xb and 0xc]. Slot meanings
(Observed, the order of the strings seen with no empty slot): 0 name,
1 model, 2 manufacturer, 3 serial, 4 and 5 firmware version, 6 `1.0.0`,
7 the updater's bundle id (`com.apple.accessory.updater.app.71`), 8 and 9
the buds' serials, 10 a number (7 digits).

## GATT

The pods serve GATT over classic Bluetooth too (ATT on PSM 0x1F), and BlueZ
discovers it on connect: it's under
`/org/bluez/hci0/dev_<MAC>/serviceXXXX` (Observed, 7.1). The firmware's
service names [`0x1bedb8`: `Invalid DOAP AHPS FMNS UARPS DIS AIS TPS HRS
POSH SECURE_SENSOR GATT`] index the registration calls.

| Service | Characteristics | What |
|---|---|---|
| `4715650b-5e9d-4ac2-b898-a4fc0aa5df78` | `94110001-6d9b-4225-a4f1-6a4a7f01b0de`: write, indicate, signed writes | **UARP** (firmware update), see below. Definition at `0x1c1488` (UUIDs big-endian, properties `0x68` at `0x1c14c0`), registered as service 4 (`UARPS`) at `0x8b3e0` behind a feature bit |
| `0000fd44-…` (Apple, Find My) | `4f860001`–`4f860005-943b-49ef-bed4-2f730304427a`: write, indicate | FMNS |
| `0000180a-…` Device Information | `2A50` PnP, `2A29` "Apple Inc.", `2A24` "A3048", `2A27` "3.36.32", `2A26` firmware | DIS (Observed reads) |
| `00001804-…` TX power, `00001801-…` GATT | | |
| `9bd708d7-64c7-4e9f-9ded-f6b6c4551967`, `7798082b-b7b7-45a6-9933-563492efe04e`, `87290102-3c51-43b1-a1a9-11b9dc38478b` | several each | Apple-private, not looked at |

## Firmware update (UARP)

- **Transport**: the GATT characteristic above, each write and indication
  prefixed with a fragment header byte (bit 0: last fragment); probing it
  works from Linux (`tools/firmware/uarp_probe.py`, read-only). UARP registers per
  transport id in a 51-slot registry [`0x84a04`; id list at `0x1c2230`:
  `00`–`11 18`–`1e 2d`–`30 15`]; registration at `0x15418e`–`0x154202`:
  accessory role (a host updating these pods) on ids `0x10 0x11 0x09 0x02`,
  controller role (these pods updating others) on `0x01 0x1a 0x1b 0x2d`–`0x30`,
  `0x18 0x19` with another handler, downstream on `0x03`. Id 0x10 is set up
  right after the GATT service [`0x8b3f6`]. Which ids are the other bud and
  the case is not established.
- **Receive path**: `0xbe378(endpoint, transport, data, length, role)` →
  accessory `0x76e80` → UARP core `0x103f10`. Messages start with three
  big-endian u16s: type, payload length, message id; the core switches on
  the type, 0–0x26. Every message type, the handshake (protocol version 6
  at most here), the information tags, the offer → pull → stage → apply
  flow and the personalization asset: [uarp.md](uarp.md).
- **Personalization (Image4)**: the BuildManifest marks every entry
  `Personalize = True`, but the buds' images come with an Apple-signed
  manifest that binds no ECID and no nonce, valid for every pair of the
  model; only the case's UWB firmware (`R1FW`) asks for a ticket (TSS
  prefix `Rap`). The `AudioAP1` request format, if ever needed, is
  libtatsu's `tss_request_add_durant_tags`. Safety, A/B slots and the
  steps: [update.md](update.md).
- The package is a whole image, not a delta (`RestoreBehavior = Erase`, every
  image present), and Apple's catalog has only the current build, so a host
  that can update one old version can update any.

## Crash logs

`rkof` keeps core dumps and crash logs (`crash_log.cpp`, `CrashLog.cpp`,
"coredump core %u section %u size %zu offset %zu", "coredump aborted during
previous boot", `CrashReporterKey`, a halt-or-reboot crash policy; strings
around `0x1dd35e`–`0x1ddb7a`, `0x1c3410`). How a host fetches them is not
established; Inferred: through the AACP `0x17` wrapper's endpoints (`AP AOP
RTP BTM DSP1 DSP2`) that Apple hosts use for analytics.

## Open

- UARP: see the open list in [uarp.md](uarp.md).
- How a host reads a crash log.
- What 0x62/0x63 carry, and the `0x02` record ids.
- The takeover reboots (AGENTS.md "Pod reboots"): [ownership.md](ownership.md)
  has the ownership arbiter, the AudioSource mask, and the aborts on those
  paths (a two-host record pool that never checks for duplicates, a
  connection tracker that aborts on a third host), none yet tied to a
  takeover; who accepts a second host's AVDTP Start and suspends the first
  is not found.
