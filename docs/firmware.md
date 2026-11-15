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
[AppleDB]: https://github.com/littlebyteorg/appledb
[ftab-dump]: https://github.com/19h/ftab-dump
[linden/uarp]: https://github.com/linden/uarp

## Reproducing

```sh
tools/firmware/fetch.sh A3048                  # -> firmware/A3048/, prints hashes
tools/firmware/ghidra/import.sh firmware/A3048/parts/FTAB/rkof.bin
tools/firmware/ghidra/run.sh dec 0x6cf48       # decompile; modes in Dump.java
```

Apple's catalog (`mesu.apple.com/assets/com_apple_MobileAsset_UARP_<model>/`)
lists only the current build, so compare the hashes below before trusting
an offset. Older packages stay on `updates.cdn-apple.com`; their URLs are
community-archived: [AppleDB] keeps one JSON per build,
`osFiles/Bluetooth Headset Firmware/<major>x/<build>.json`, whose `sources`
map each model (`deviceMap`, e.g. `AirPodsPro1,2-usbc-case` = A2968) to its
zip (7B21's case package came from there, docs/case.md), and [uarp-dump]
lists some. A case that was never updated runs its factory build, not the
buds' (ours: 7B21, its USB `bcdDevice` 0x4708 = STFC 71.8), so analyze the
build it runs ([case.md](case.md)).

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

The pods tested with calvados (Observed, GATT Device Information and AACP
`0x1D`): model A3048 (the primary at the time), hardware `3.36.32`. They
ran 7.1 (`71.2256000021000000.6663`; UARP: 7.1.21.0, i.e. 7B21,
`tools/firmware/uarp_probe.py`) until Oct 27, then 9A348 (updated over
GATT); 9B47 was staged over AACP on Nov 14 ([uarp.md](uarp.md)). Offsets
here are from 9A348 unless a build is named.

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

**9B47** (9.1.47, train GongB, 2026-10-23, `updv` 47; Apple: "bug fixes"),
diffed against 9A348 (shift map, masked byte compare, all 83 HSM tables,
landmark decompiles). Only `rkof` and `aopf` changed (the rest: version
stamps); `aopf` isn't analyzed. `rkof` code moved by 2-58 bytes
(AppleAPHostLibWx 901.222 → 910.6), and none of what calvados relies on
changed [Known]: `stream_sync_a2dp.cpp:429` (stub 0xe8f18, still 3000 then
2500 ms), paging (guard 0x99978, tables 0x1c2a68 and 0x1c2a8c unmoved), the
arbiter (0x1c5a9c unmoved), the UARP aborts (0xbea50, 0xbcd62/0xbcdaa), the
gate (0xc6184), the AACP dispatch (0x6cf48 unmoved, identical). Changed:
- the bud's wired UART frame parser (0x10217c, fed by `b2p_uart`): longer
  minimum lengths, and an overflow now resets it instead of writing past
  the buffer;
- an Inferred RSSI sampler, HSM 0x1c5c30: one state fewer, its action 0
  one event earlier, a field added;
- 0x7ba60 (ids 0x67/0x68): no longer skipped when 0xc6184() is false.

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

Control (0x09) frames all go out through `0x6e804(session, id, value)`,
the only sender of opcode 9 (no other `0x6e55c` call passes it). It is
called directly with ids `0x06 0x0D 0x1D 0x1F 0x24 0x27 0x2E`, and through
`0x87354(id, value, …)`, which sends to every session, with ids `0x17 0x1B
0x23 0x25 0x26 0x28 0x29 0x2C 0x2D 0x2F 0x33 0x35 0x3E` (each id the
immediate loaded before the call; no pointer to either function in the
image, so no indirect calls). So the pods never send `0x1A`, the
press-and-hold cycle (`ListeningModeConfigs`), though they keep it: Known
for 9A348, and Observed (no `0x1A` received, also right after setting it,
and none in the `0x02` records).

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
around `0x1dd35e`–`0x1ddb7a`, `0x1c3410`). Not over UARP: the solicitation
handler [`0xbddcc`] serves `HSML`, `VERS` and `MTIC` (asset types 4, 7,
0x16) and refuses `LOGS` and `CRSH`, which it only considers from endpoint
type 0xe; a host *offering* an asset with one of those three tags reaches
`Spicy_LogAbort` [`0xbcd82`, `0xbcdca`]. `MTIC` is a RAM metrics queue
(provider 0x1e: per paired host usage, 300 bytes; provider 1: power and
usage, 704 bytes), no crash data.

Over AACP 0x17 (Known from code; the transfer Observed Nov 6: bud 2's
six 32 KB logs, one per core):
- The wrapper: `[class] [len u16 LE]` + body, the class a u32 when the
  AAP version is above 1.1 [`0x6cf1a`], else u16. Class 0x200 is a host's
  request to the log module (listener [`0xafa48`] → dispatcher
  [`0xafecc`], body `[type] [bud mask] [2 bytes] [sub] [1 byte]`); class 0x400 its
  answers; 0x100000 HID; 0x200000 and class-0x400 type 8 ULLA.
- The store: at boot the `crlg` object [`0x13dd4`] saves each core's RTKit
  crash log (signature `CLHE` or `CLGH`, ≥ 0x20 bytes [`0x194d0`]) to
  flash as `crlg.<n>` [`0x194a0`] and sets NVRAM `CrlgMap` bit per core
  [`0x1950c`]. A record (printer [`0x1837c8`]): 32-byte header, +8 size,
  +0xc exception kind (low byte: undefined, panic, hard fault, AP-requested,
  force reset, stack guard, mailbox error…; 0x100 incomplete, 0x200
  nested), sections `[tag] [4] [4] [size incl. 16]`: `Cstr` (the message),
  `Cver`, `Ctim`, `Cmbx`, `Crg8`, `Ccst`; then a copy of the header. Asahi
  Linux's RTKit crash log parser reads the same format.
- The announcement [`0xb36d0`, timer 0x69 in `0xb22e8`]: `06 00 00 [n]` +
  per core `[name 4] [size u32]` (cores AP AOP RTP BTM DSP1 DSP2; `fe`
  instead of `06` before an automatic push, per `crash_log.rs`), while
  `CrlgMap` ≠ 0 and no host has asked for an automatic push. Observed:
  ~10-14 s after a host connects (not only after a boot), to one host,
  so far always the one holding the audio (5 times); 7 times on Oct 27
  (the update day, repeated reboots), on the phone in the first session
  after the Nov 4 reboot, nowhere else since Oct 1; not in every session
  (the laptop's after Nov 4 didn't get it).
- The request: type 5 (send logs), mask `ff`, sub 1 (crash logs; 2: the AP
  log ring `aplg.NN`): `05 ff ff 00 01 02`, what the firmware builds itself
  for an automatic push [`0x1c4154`, `0xb24cc`]. No Apple Device ID or
  feature-flag gate on the path. Type 6 sets the push target in NVRAM;
  types 8/9 (log streaming, non-prod only) and 0x10-0x12 (`core_en`) change
  NVRAM: not for a host. Bud mask `00` clears the announcement without
  sending anything (AGENTS.md): don't.
- The answer [`0xb3048`]: `01 [n | 0x80 more] [bud 1/2 | 0x80 holding the
  link]` + up to 0x10c bytes, paced by the link's completions; `ff` ends
  [`0xb2d7a`], `fc 01` aborts [`0x156790`]. Afterwards `cr_user`/`cr_inear`
  are reset [`0xb2660`].
- Coredumps (`coredump`, NVRAM `core_en`) have no path to a host.

calvados asks once per run when announced (`aacp/crash_log.rs`).
Fetching doesn't clear them (Observed Nov 6: the same 192 KB again): the
only clear [`0xb25b8` → `0x19578`] zeroes NVRAM `CrlgMap`, while the
reader [`0x18854`] returns the RAM copy, so the same logs are announced
until the pods reboot.

Reading a record: `Ctim` is µs since the pods' boot (the 0x2B history's
clock); `Ccst` the current task's call stack; `Crtk` the RTKit task
table; `AMCL`/`HLCL` rings of active-object dispatches, `[u32 id] [from]
[to] [event] [u16] [LEB128 Δµs]`; `BTSK` the live ACLs; `CPvr` the build
(`90.3431000025000000.6807`). `CASE`, `SHUT`, `PMGR`, `CHLU`, `UPW3`,
`BPHY` undecoded.

### `stream_sync_a2dp.cpp:429` (the Nov 4 reboot)

Task `AudioMgr`; the assert stub [`0xe8f34`, `movw r2,#0x1ad`] is called
only from [`0xfb50a`], action 7 of the A2DP stream-sync state machine
(table `0x1cc6a4`, one object per stream, built in [`0xec938`]). On the
stream's start it enters S0x13/S0x14, starts the clock tracker (+0x88) and
a 3000 ms timer (+0xa4, event 0x08) [`0xfb0f4`], and waits for event 0x12,
the audio clock (`audio_clock.cpp`, +0x58) publishing 'AC' type 0,
anchored. First timeout: action 6, S0x15, the timer again at 2500 ms
[`0xfb034`]; action 6 reaches [`0xca5cc`] → event 0x10e to the per-host
machine (table `0x1c680c`), whose action 5 sends the source an **AVRCP
PAUSE** [`0x90494` → `0xca0a4` → `0x703e8`]. Second timeout in S0x15:
action 7, the unconditional assert. Other ways out: events 0x07 ('UT'
type 1), 0x0f, 0x01, 0x10 (a start time); which of them a SUSPEND or
CLOSE raises is not established. The record's AMCL ring shows the object
going S0x14 → S0x15 on event 8 at 1956.022 s, and the crash 2.5007 s
later.

### Resets that leave no crash log

Only the RTKit exception entry `0x182d28` writes the RAM log the next
boot saves: asserts (`0x1823c`), faults, and the RTKit watchdog's
pre-expiry (`0x182cfc`). The orderly reset `0x1c1f0` writes none. Its
main caller is the Shutdown Manager (`0x15f804(reason)`, the reason kept
in NVRAM `stn_rs`, names from the shell table `0x1d5954`): 1 uvlo,
2 case_uvp, 3 case_reset, 4 hard_reset, 5 soft_reset, 6 assert (the only
one with a log), 7 safety_fault. The case asks for most of them over B2P
[handler `0x158d6c`]: 0x60 → case_reset unless UARP is busy (`0xbe420`),
0x5c → soft_reset, 0x110 → case_uvp. Other callers: the main loop (4)
and a 1 s poll while charging (1).

The case resets docked buds (STFC): 4 s after the lid closes
[`0x0801f18a`] each bud whose radio is off is reset (state 9, B2P 0x60,
"reset attempt %d"; a CRABS hardware reset if that fails); a bud still
holding a host is left until its radio goes off, then reset 1 s later.
Observed: every put-away on Nov 12-13 was followed by a boot 1-3 s after
the session ended (the next session's 0x2B uptime), and the secondary
drops ~4 s after each close; Nov 6 and Nov 9 5-30 s after both buds went
in, the laptop's link timing out (0x08), no LMP detach
([paging.md](paging.md)). So a short uptime after the buds were in the
closed case is this, not a crash. A host can't read the reason: it isn't
in any AACP packet (the case gets it, record 0x188/6, with 3, 5 and 6
mapped to 0); a crash log's `SHUT` section has it (`0x15f7e0`, 0x48
bytes), and the pods' MTIC metrics asset might (untested).
Nov 13 19:14 (buds in ears, the phone's Bluetooth turned off mid-stream):
a boot ~3 s after the phone's session ended, no crash log, so one of the
log-less reasons or a reset below the AP (PMU watchdog, brown-out);
nothing found turns a vanished A2DP source into a reset.

## A new session

What the pods send a new AACP session, what holds part of it back, and the
feature flags: [session-start.md](session-start.md).

## Which host the pods connect to

When they page out, which host, and what the advertisement says about
it: [paging.md](paging.md).

## Ownership

The `OwnsConnection` arbiter, smart routing, `AudioSource` and the A2DP
switch: [ownership.md](ownership.md).

## A host-side updater

What closing the lid does, how several hosts interact, resuming, and the
case's version: [update-host.md](update-host.md).

## Find My pairing (9B47, the case)

Static only (Nov 15); C unless marked.

- NERF `pairing_control_point.c`, handler 0x3d808: 0x100 initiate, 0x102
  finalize, 0x104 complete, 0x10b a status of its own. Initiate's result
  (0x232ca): S1, S2 (≤ 0x500), `ref_key`, `sik_pub`. Finalize (0x23428):
  seed 32 at +0x59, S3 at +0x79 (length +0xdd, ≤ 100), 60 bytes at +0xde,
  the BAA leaf at +0x11a (length +0x606, < 0x2c3), the intermediate at
  +0x3dc (length +0x608, < 0x22b), 8 at +0x60a, 24 at +0x612. NERF passes
  them to R1 (0x20f40, 0x21620-0x2163c): certificates 0x21, S3 0x1c,
  check 0x22.
- R1FW `rkos` (file offset = address; `__DATA` file 0x4c000 = RAM
  0x20000000): crypto commands at 0x19b34. The S3 check (0x19d12-0x19e8a)
  hashes 0xaf bytes (the session nonce, a 20-byte field, 8 constant, the
  seed, SHA-256 of C2, 32 from the check command, `ATTEST_AUTH_TOKEN` or
  `_NOTKN`) and verifies ECDSA P-256 against `crypto_set_pub_key`'s key
  (command 0x16) or the built-in one at 0x42e29 (built at 0x1ab8c); the
  finalize carries no key, so the built-in (G). The chain (0x1b55a): leaf
  and intermediate plus the embedded Basic Attestation System Root CA
  (P-384, DER at 0x42bc4, 2017-2032), evaluated CoreTrust-style (0xa66 →
  0x854, policy at file 0x518f0, OIDs 0x51918); the leaf's key isn't
  compared with anything (0x3eae0). Errors 0x4017 S3, 0x4018 chain,
  0x4003 no S3, kept at +0xe8, blocking 0x1aaaa. Also: a P-256 key at
  0x42de8 (into ctx+0x74 at 0x1b53e, next to `ServerSharedSecret`; the
  server's encryption key, G), more P-384 anchors (keys 0x3fb24, 0x3fce4,
  0x3fd70; records 0x519c8-0x51bb0), KDF labels at 0x42e6a.
- The buds' code is signed: `FTAB` holds an IM4M (at 0x771e90, after
  `bver`; RSA/SHA-384, `T2016-SDOM1-TssLive-ManifestKey-RevE-DataCenter`
  under Apple Secure Boot Root CA - G3; BORD 6, CHIP 0x2016, no ECID or
  nonce) with SHA-384 digests of acib, aopf, bver, lpda, phyb, rkof, rkos,
  rtpf, sbd1, sbd2 (matching); `rkos` checks them at boot (0x4924,
  "Digest check failed(%s)"). Not in it: the ls* prompt tones (RIFF WAV),
  dstn, L*, apmu, btpw.
- 9A348 rkof: AACP 0x31 lands in `caseTone` (0x885c4), an NVRAM byte
  synced between the buds (0xc75aa, 0xc7114); it plays nothing.

## Open

- UARP: see the open list in [uarp.md](uarp.md).
- Crash logs: how the primary starts on its own logs (`0xb35dc` is
  reached only through bud message 0xc5, `0xc73e0`); whether the saver's
  `crlg.%u` [`0x194a0`] is the transfer's `crlg.%02d` [`0xb04fc`]; who
  calls the `CrlgMap` clear; who raises event 0x16d (the pods' own push,
  [`0x84438`]).
- What 0x62/0x63 carry, and the `0x02` record ids.
- Why the audio clock doesn't anchor after a take (the Nov 4 assert
  above), and whether an AVDTP SUSPEND leaves the wait.
- The takeover reboots (AGENTS.md "Pod reboots"): the aborts on the
  ownership paths ([ownership.md](ownership.md): a two-record arbiter pool
  that never checks for duplicates, a connection tracker that aborts on a
  third host) aren't tied to a takeover yet; the code that defers a second
  host's AVDTP START, and the caller of the SUSPEND sender, are not found.
