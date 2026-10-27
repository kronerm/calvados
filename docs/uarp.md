# UARP in the AirPods firmware

The firmware-update protocol as the pods' side implements it: AirPods Pro 2
(USB-C), build 9A348, image `rkof` (hashes and tools: [firmware.md](firmware.md)).
Static analysis, except where marked Observed: the handshake and the
information requests, sent with `tools/firmware/uarp_probe.py` to pods on
7.1.21 (7B21).

## Transport (Known; Observed on 7B21)

The GATT characteristic `94110001-6d9b-4225-a4f1-6a4a7f01b0de` (write,
indicate; firmware.md), which BlueZ serves over the classic link too. Every
write and every indication starts with a **one-byte fragment header**,
bit 0 set on a message's last fragment; the rest is appended to the
message [rkof 0x69fba–0x6a03c, the service's write callback at
`0x1c1428[3]`]. Without the header nothing is delivered: a first byte
without bit 0 just starts buffering.

Observed (7B21): version discovery proposing 9 answers **4** (9A348 would
answer 6). Tags: 1 `Apple`, 2 `AirPods Pro`, 3 a bud's serial, 4
`X2290/MPMP/MXXC1/*/*`, 5 running `7.1.21.0`, 6 staged `0.0.0.0` (none),
7 and 8 zero, 9 empty. Replies number their own message ids from 1.

Observed (7B21), offering 9A348's SuperBinary with
`tools/firmware/uarp_offer.py` (asset tag 0, flags 1, version and length
from the SuperBinary header, 20 payloads): the pods acknowledge the offer
(0x0d, every field echoed) and then request nothing, with no processing
notification, for 30 s while worn and 2 min in the case without power:
deferred. With the case charging they decide at once: processing
notification flags `0x0202`, denied with the handler's default reason
(`0x200`, set at [rkof 0x75af0]; the acceptance check's specific reasons
1, 2, 5 would give `0x0003`, `0x0002`, `0x0007`), also right after a
rescind-all (`0x0c` asset `0xFFFF`, acknowledged). Why is open: in 9A348
the default survives e.g. an offering endpoint of type 0xe
[rkof 0xbdae4–0xbdaea]. Nothing was staged in any of these.

After closing and reopening the case (their message ids restart at 1, so
the UARP state was reset), the same offer with the case charging is
**accepted**: the pods pull the SuperBinary header (offset 0, 0x2c bytes),
then the payload headers one by one (0x28 each) up to `FTAB`, the 13th
(`AUCH AUER AUFM AUFR AULB AUPC AUPS AUUF AUUT CMAP CMAP CTAB FTAB`),
then 50 bytes at `0x253c`, past the headers (metadata, Inferred `FTAB`'s).
The tool rescinded there; acknowledged, nothing staged. So the denials
came from state left by earlier offers, which rescinds didn't clear;
closing the case did.

Serving the metadata too (`uarp_offer.py … metadata`, everything before
`0x2a5c`, the first payload's data): after `FTAB`'s metadata (its SHA-256,
`0xCCD28107`, which matches the payload, and `0xCCD28112` = 1) the pods ask
for `FTAB`'s data right away, 475 bytes at its start (`0x5e7781`), with no
personalization request in between: the buds stage `FTAB` first and need
no ticket. At 475 bytes a request, `FTAB` is ~16 500 requests. Rescinded
there; acknowledged, nothing staged.

The Linux daemon asks this at every AACP session (`crates/linux/src/uarp.rs`,
messages from `protocol::uarp`): Sync, version discovery proposing 4 and
the tag 5 request, read-only, to show the exact version (7.1.21 where
0x1D says only 7.1) and compare it with the catalog's release.

Discover Downstream (0x15, in `uarp_probe.py`) is answered with 0x16
status 0 and no 0x17 at all, also with a fresh state (case closed and
reopened) and charging: with nothing staged, the primary offers a host no
route to the other bud or the case. Right after the case reopens, BlueZ
can refuse the first GATT write ("Not connected") for a moment.

All addresses `[rkof 0x…]`. **Known** = read from code; **Inferred** = meaning guessed
(names are not in the binary: no msg-type name strings exist; names below follow
behaviour and request/ack pairing).

## 0. Framing and plumbing (Known)

- Header, big-endian: `[type u16][payloadLen u16][msgID u16]`, then payload at +6.
  Total length must be ≥ 6 and ≥ payloadLen+6, else status 8 [rkof 0x103f46–0x103f7a].
  msgID is stored in the endpoint (`ep+0x3c`) [rkof 0x103f74]. Dispatcher = `FUN_00103f10`,
  `tbh` jump table at 0x103fa4 (cases 0..0x26; >0x26 → default) [rkof 0x103f98].
- Tx builder `FUN_00107302(ctx, ep, type, totalLen, expectAck)` writes type, len-6, msgID=0
  [rkof 0x107302]; `FUN_001073ac` queues/sends [rkof 0x1073ac]. Outlined helpers:
  `0x1a931e` = BE16@msg+6, `0x1a98b8`/`0x1a9e50` = BE16@msg+8, `0x1a9356` = type of a pending
  tx entry, `0x1ac3ea` = mark pending entry answered. All multi-byte fields BE.
- Two UARP instances are created at boot [rkof 0x154600, 0x154684] by `FUN_00106554`:
  - **accessory role** (role 0), ctx `0x200b2728` — the one the iPhone/Mac talks to
    (rx path `FUN_00076e80` → `FUN_00103f10(0x200b2728, …)` [rkof 0x76e80]).
  - **controller role** (role 1), ctx `0x200b2ac8` — towards downstream devices; almost no
    message callbacks (struct re-zeroed [rkof 0x154660]).
- Config copied to ctx+0 (0x28 bytes) [rkof 0x106554]; **ctx+0xc = local max protocol
  version = 6** (`strh 6,[sp,#0x280]` → config+0xc [rkof 0x154550, 0x15455e]); default would
  be 9 if 0 [rkof 0x106554]. Callback struct (0x124 bytes) copied to ctx+0x2c.
- "Ack tracking" flag ctx+0x150 is cleared when callbacks ctx+0xb0/b4/b8 are all null
  [rkof 0x106586–0x10659a]; they are not set for the accessory instance, so responses are
  accepted without matching an outstanding request (Known from init; the matching loops
  below only run when the flag is set).
- Remote endpoint config (`FUN_0010661c` via `FUN_00076cb0` [rkof 0x76cb0]): ep[0] = max
  payload of data *responses* sent, ep[1] = max length per data *request*, ep[2] ≥ ep[1],
  ep+0xc version (0 → ctx's 6), ep+0x24/0x25 = 0. Values come from the transport at
  runtime (not static). ep+0x38 = "transfer active" (init 1) [rkof 0x10661c].
- Status codes seen (Known values, meanings Inferred): 0 ok, 8 bad length, 9 bad
  length/no matching request, 0xb no memory, 0xe no handler, 0x11 unknown asset, 0x16 bad
  offer flags, 0x1a/0x21 bad pause-resume flags, 0x1c unknown info tag, 0x27 buffer too
  small, 0x3a unknown downstream id, 0x41 data overflow.

### Accessory-instance callbacks wired [rkof 0x15456e–0x1545d6] (Known)
| ctx off | fn | used by |
|---|---|---|
| 0x48 / 0x50 | 0x75670 / 0x7569c | pause / resume (msg 8) — just return 0 |
| 0x58 / 0x5c | 0x756c8 / 0x756ce | SuperBinary / dynamic asset offered (msg 5) |
| 0x60 | 0x756d4 | apply staged assets (msg 0xa) |
| 0x68,0x70,0x78,0x80 | 0x7572c,0x75794,0x757fc,0x75870 | info tags 1–4 |
| 0x88,0x90 | 0x758e8,0x75930 | info tags 5,6 (versions) |
| 0x98 | 0x75978 | info tag 8 (last error) |
| 0xa4 | 0x75988 | msg 0x11 solicitation |
| 0xbc | 0x75aa4 | version negotiated (msg 1) |
| 0xc0 | 0x759f4 | info tag 9 |
| 0xd8 | 0x75a68 | (returns 0x33) |
| 0x108 | 0x78134 | msg 0x15 "Discover downstream accessories" |
| 0x118 | 0x78280 | msg 0x1b relay to downstream |
| 0x11c | [sp+0x14c] if non-null | msg 0x1d |
| 0x14c | 0x781c0 | downstream removed |
| 0x174 | 0x105ec4 | vendor msg 0xFFFF |
Not wired (handler acks or returns 0xe): 0x4c/0x54 (msg 0xe), 0x64 (0xb), 0xa8/0xac,
0x10c/0x144 (0x17), 0x110/0x114 (0x19/0x1a), 0xc8 (0x20), 0xcc (0x22), 0xd0 (0x23),
0xd4 (0x25), 0x120 (vendor).

## 1. Message table

Dir: C→A = controller (iPhone/Mac) to accessory, A→C = accessory to controller. Offsets are
from message start (payload starts at +6).

| Type | Dir | Name (Inferred) | Payload | Reply / notes |
|---|---|---|---|---|
| 0x00 | both | Sync | empty | Accessory role: flush tx queue, reply type 0 empty (`FUN_00105e20`) [rkof 0x1053aa→0x105e20]. Controller role: reset ep, send type 1 with version=min(ep+0xc,9) [rkof 0x103ff6–0x104088]. |
| 0x01 | C→A | Version Discovery Request | +6 u16 controller protocol version; len≥2 | Reply **0x02**, len 0xc: +6 u16 status 0, +8 u16 negotiated = max(1, min(req, 6)), +10,+11 bytes ep+0x24/0x25 (=0) **only if negotiated > 5**, else payload 4 bytes [rkof 0x104a74–0x104b22]. Stores ep+0x2c; calls ctx+0xbc. |
| 0x02 | A→C (rx in ctl role) | Version Discovery Response | +6 u16 status, +8 u16 version, opt +10,+11 bytes → ep+0x2e/0x30; len≥4 | status≠0 → error 2; version clamped 1..9 [rkof 0x1047fe–0x104860, 0x10563a] |
| 0x03 | C→A | Accessory Information Request | +6 u32 info tag; len≥4 | Reply **0x04**: +6 u16 status, +8 u32 tag, +12 u32 value length, +16 value. Error → payload 10 (status only, 0x1c unknown tag, 0x27 too small) [rkof 0x1048aa–0x104982, 0x105534–0x105b8e]. Tags §3. |
| 0x04 | A→C (rx ctl) | Accessory Information Response | +6 u16 status, +8 u32 tag, +12 u32 len, +16 value; len≥10, len+16 ≤ total | dispatches to ctx+0x6c..0xc4 (tag 5/6 versions stored at ep+0x62/+0x72) [rkof 0x10458c–0x104634] |
| 0x05 | both | Asset Available Notification (offer) | +6 4cc asset tag, +10 u16 flags (bit0 SuperBinary, bit1 dynamic), +12 u16 assetID, +14 16-byte version (4×u32), +0x1e u32 asset length, +0x22 u16 numPayloads; len≥0x1e | Reply **0x0d** len 0x24 echoing all fields [rkof 0x104cb8–0x104e30]. Flags bit0 → ctx+0x58, bit1 → ctx+0x5c, neither → 0x16. Dynamic offer of same tag/version reuses the asset slot. Sent by A as `FUN_00106c38` [rkof 0x106c38] (e.g. personalization request §5). |
| 0x06 | A→C (C→A when A is asset source) | Asset Data Request | +6 u16 assetID, +8 u32 offset, +12 u16 length (len≥8 on rx) | Sent by `FUN_00102eb4` [rkof 0x102eb4]: length = min(remaining, window, ep[1], 0xffff); not sent while paused (ep+0x38==0 → 0x1f) or asset+0x49 set. On rx: reply **0x07** [rkof 0x104f1e–0x105000]. |
| 0x07 | C→A (also A→C) | Asset Data Response | +6 u16 status, +8 u16 assetID, +10 u32 offset, +14 u16 requested len, +16 u16 actual len, +18 data; len≥0xc, actual+0x12 ≤ total | Built on rx of 6: status 0x11 if asset unknown or offset > size; actual = min(req, ep[0], size-offset) [rkof 0x104f1e]. On rx: status≠0 → abandon asset with processing flags 4; checks offset==asset+0xfc, req==asset+0x100, actual≤req, buffer fits (else 0x41); copies, then next request or section-complete handler [rkof 0x104986–0x104a70, 0x10587a–0x105e10]. |
| 0x08 | C→A | Asset Data Transfer Notification (pause/resume) | +6 u16 flags: 1 = pause, 2 = resume; len≥2 | Reply **0x0e** len 8 echoing flags. Pause: ep+0x38 1→0, ctx+0x48. Resume: 0→1, re-issue data requests for all assets of ep, ctx+0x50. Other/invalid state → 0x21 [rkof 0x105022–0x1050b8, 0x1052ee]. A sends 8 as controller ("Sending pause/resume for asset" strings 0x1e10b0/0x1e106f). |
| 0x09 | A→C | Asset Processing Notification | +6 u16 assetID, +8 u16 flags | Sent by `FUN_00103e32` [rkof 0x103e32]. Flags (low values Known, meaning Inferred): 1 = fully staged [rkof 0x106b58], 2\|reason = denied [rkof 0x1069d0], 3\|reason = abandoned (reasons 0x700/0x800 when superseded) [rkof 0x106a12, 0xbdbb8], 4 = corrupt (bad SuperBinary header / data error) [rkof 0x103114, 0x104a64]. On rx (ctl role) reply **0x0f** echo; asset+0x94/0xb8 callback [rkof 0x1046a8]. |
| 0x0a | C→A | Apply Staged Assets Request | none checked | Reply **0x0b** len 10: +6 u16 status (cb result, 0xe if none), +8 u16 flags [rkof 0x105004, 0x1053c6]. See §4. |
| 0x0b | A→C | Apply Staged Assets Response | +6 status, +8 flags; len≥4 | rx: ctx+0x64 (unset) [rkof 0x104542] |
| 0x0c | C→A | Asset Rescinded Notification | +6 u16 assetID (0xFFFF = all); len≥2 | Reply **0x10** len 8 echo. Marks asset(s) rescinded (+0x48), asset+0x80 cb; ctx+0xa8 if all & none [rkof 0x10463c–0x1046a6, 0x1055e8]. |
| 0x0d | A→C | Asset Available Notification Ack | echo of 5; len≥0x1e | rx: clears pending 5 with same assetID [rkof 0x104ed4] |
| 0x0e | A→C | Data Transfer Notification Ack | +6 u16 flags; len≥2 | rx: 1 → ctx+0x4c, 2 → ctx+0x54, else 0x1a [rkof 0x1044ba] |
| 0x0f | C→A | Processing Notification Ack | +6 assetID, +8 flags; len≥4 | rx: asset+0x98 cb, release [rkof 0x104864] |
| 0x10 | A→C | Rescinded Ack | +6 assetID; len≥2 | rx: ctx+0xac (all) / asset+0x84 [rkof 0x104474] |
| 0x11 | C→A | Dynamic Asset Solicitation | +6 4cc tag; len≥4 | Reply **0x12** len 0xe: +6 u32 status 0, +10 tag [rkof 0x104b9a–0x104c04]; ctx+0xa4 → 0x75988: supported tags HSML, VERS, MTIC (platform `FUN_000bddcc` [rkof 0xbddcc], tag strings 0x1e2b12/0x1e2b3f/0x1e2b30; LOGS/CRSH listed but rejected on that path); on failure sends **0x13**(tag, 0x1a) [rkof 0x75988]; on success the platform later offers the asset with msg 5 (Inferred). |
| 0x12 | A→C | Solicitation Ack | +6 u32 status, +10 tag; len≥8 | rx: clears pending 0x11 by tag [rkof 0x104fc6] |
| 0x13 | A→C | Solicitation Result (Inferred) | +6 4cc tag, +10 u16 status | rx: reply **0x14** (+6 tag, +10 status echo), asset by tag gets +0x9c cb; no asset → 3 [rkof 0x1050e8–0x105186] |
| 0x14 | C→A | Solicitation Result Ack | +6 tag, +10 status; len≥6 | rx: asset+0xa0 cb [rkof 0x104c08] |
| 0x15 | C→A | Discover Downstream Accessories (cb logs "Discover downstream accesories" 0x1e1ceb) | none | Reply **0x16** status 0; ctx+0x108 → announces each attached downstream with **0x17** [rkof 0x104e9c, 0x78134] |
| 0x16 | A→C | Discover Ack | +6 status; len≥2 | [rkof 0x1050bc] |
| 0x17 | A→C | Downstream Endpoint Added | +6 u16 downstream id; (ver≥9: TLV list `[u32 type][u32 len][val]` follows) | Sent by `FUN_00106f40` [rkof 0x106f40]. rx: reply **0x18** (+6 status 0, +8 id) [rkof 0x105188] |
| 0x18 | C→A | Added Ack | +6 status, +8 id; len≥4 | [rkof 0x1047d0] |
| 0x19 | A→C | Downstream Endpoint Removed | +6 u16 id | Sent by `FUN_00106fb6` [rkof 0x106fb6]; rx: reply **0x1a** (+6 status 0/0x3a, +8 id) [rkof 0x10472e] |
| 0x1a | C→A | Removed Ack | +6 status, +8 id; len≥4 | [rkof 0x1052d8] |
| 0x1b | both | Downstream Relay | +6 u16 downstream id, +8 encapsulated UARP message; len≥8 | C→A: forwarded to downstream `sendMessageFxn` (0x78280); A→C: `FUN_00105e66` wraps downstream traffic. Ack **0x1c** (+6 status, +8 id) only if negotiated version < 5 [rkof 0x10437c–0x10440a, 0x105e66]. |
| 0x1c | — | Relay Ack | +6 status, +8 id | [rkof 0x10520a] |
| 0x1d | C→A | unmapped (downstream family) | none | Reply **0x1e** +6 status 0; ctx+0x11c [rkof 0x105242] |
| 0x1e | — | ack of 0x1d | | [rkof 0x105032] |
| 0x1f | C→A | Query id list (unmapped meaning) | none | Reply **0x20**: +6 status 0, +8 u16 count, +10 u16[count] ids from ctx+0x17c table (stride 8) [rkof 0x104b26]; no populate of the table found (only cleared [rkof 0x1067ba]) → likely count 0 |
| 0x20 | — | reply of 0x1f | +8 count, +10 u16[] | rx: ctx+0xc8 (unset) [rkof 0x10505a] |
| 0x21 | C→A | Query entry by id (unmapped meaning) | +6 u16 id; len≥2 | Reply **0x22**: +6 status 0, +8 id, +10 u16 count, +12 count×4 bytes [rkof 0x104502, 0x10540e] |
| 0x22 | — | reply of 0x21 | +8 id, +10 count, +12 data | rx: ctx+0xcc (unset) [rkof 0x10440e] |
| 0x23 | C→A | Bulk Info Query | +6 u16 id, +8 4cc context, +12 u32 count (<2^30), +16 u32[count] TLV types; len≥10 | Reply **0x24**: +6 status 0, +8 id, +10 4cc, +14 count, +18 types copy [rkof 0x1041fa–0x104326]; ctx+0xd0 not wired. |
| 0x24 | — | Bulk Info Query Ack | len≥0xc | [rkof 0x10432c] |
| 0x25 | A→C | Bulk Info Response | +6 u16 id, +8 4cc context, +12 TLV data | Sent by `FUN_00107188` from `FUN_000771cc` (strings "Bulk Info Response failed", "Uknwn TLV:%u", pool 0x773a0) [rkof 0x107188, 0x77366]. rx: reply **0x26** (+6 status, +8 id, +10 4cc) [rkof 0x104136] |
| 0x26 | C→A | Bulk Info Response Ack | len≥8 | [rkof 0x10527e] |
| 0xFFFF | both | Vendor Specific | +6 3-byte OUI `BC 09 63`, +9 u16 vendor type, +11… | ctx+0x174 = `FUN_00105ec4` [rkof 0x105ec4]. Vendor 0x20 = property query: +0xb u32 property id 1..0x21 (len≥4); reply vendor 0x21: +0xb u32 status (0x1a unsupported), +0xf u32 id, +0x13 u32 len, +0x17 value. Accessory property table at ctx+0x190 wires only ids 1 and 2 (strings via `FUN_000bc8b0` sel 0xb/0xc) [rkof 0x1545de–0x1545e4, 0x7832c, 0x78342]. |

Other types (0x27–0xFFFE): default branch, only 0xFFFF handled [rkof 0x10408c].

## 2. Handshake / version negotiation (Known)

1. Controller sends **Sync (0)** (or accessory replies Sync to Sync: role 0 flushes its queue
   and answers type 0) [rkof 0x105e20].
2. Controller sends **0x01** with its version; accessory answers **0x02** with
   `max(1, min(controllerVersion, 6))`. This firmware's max = **6** [rkof 0x15455e]
   (library default 9, controller role caps the proposal at 9 [rkof 0x104060]).
3. Version gates: >5 adds 2 bytes to 0x02; <5 requires 0x1c acks for relay; ≥9 TLVs in 0x17.

## 3. Accessory information tags (msg 3/4) (Known source, Inferred names)

| Tag | Source | Format |
|---|---|---|
| 1 Manufacturer | string at RAM 0x20006634+0x14 [rkof 0x7572c] | ASCII, no NUL (len = strlen) |
| 2 Model name | 0x20006634+0x54 [rkof 0x75794] | ASCII |
| 3 Serial number | ctx+0x2ac [rkof 0x757fc] | ASCII |
| 4 Hardware version | 0x20006634+0x94 [rkof 0x75870] | ASCII |
| 5 Active FW version | ctx+0x370, 16 bytes [rkof 0x758e8] | 4×u32 BE major.minor.release.build |
| 6 Staged FW version | ctx+0x380 [rkof 0x75930]; 0xFF×16 if no source [rkof 0x105aae] | same |
| 7 Statistics | ep+0x3e..0x4c, 4×u32 [rkof 0x10490a] | BE u32×4 (Inferred: bytes/assets counters) |
| 8 Last error | ctx+0x2a0, ctx+0x2a8 [rkof 0x75978] | 2×u32 BE (default 0xFFFFFFFF×2) |
| 9 (unnamed string) | ctx+0x32c [rkof 0x759f4] | ASCII |
Tags 1–4, 9 fall back to "UNKNOWN" (7 bytes, 0x1e8370) when no callback [rkof 0x105e14].
Other tags → status 0x1c.

## 4. Firmware flow (accessory as target) (Known unless marked)

1. **0x05** offer → 0x0d ack → `FUN_000756c8` logs "%s Offered <%u> <Version …> Payloads<%d>",
   merges with an earlier asset of the same contents ("Merging assets"), asks platform
   `FUN_000bd938` → 1 accept / 2 deny / 3 defer [rkof 0x756c8, 0xbd938]. Deny → msg 9
   flags `reason|2` (reason from bd938: 1..5, default 0x200); accept failure → 0x900|2.
   bd938 compares versions (`FUN_001082e4`) and checks HSML tag / endpoint type; a new offer
   abandons an older one (msg 9, 3|0x700 or 3|0x800) [rkof 0xbdbae–0xbdbb8].
2. Accept (`FUN_00106810`) → pulls sections with **0x06**/**0x07**: SuperBinary header
   (0x2c bytes, format ≤ 4, header length must be 0x2c, offsets/lengths within size, else
   msg 9 flags 4) [rkof 0x103000], payload headers (0x28 each) [rkof 0x1031e8], metadata,
   then payload data per payload [rkof 0x1034b8]. Max per request = ep[1] (transport).
3. Pause/resume via **0x08** (§1). Rescind via **0x0c** (0xFFFF = all).
4. Fully staged → **0x09** flags 1 [rkof 0x106b50]; "Asset <%u> fully staged".
5. **0x0a** apply: `FUN_000756d4` sets flags 1, calls `FUN_000bc390` [rkof 0xbc390]: if the
   controller endpoint's type is 0x10/0x11 (`bic #1; cmp #0x10` [rkof 0xbc3a0]), it compares
   staged vs running version (`FUN_000bfd5c`, `FUN_001082e4`) and only if newer calls
   `FUN_00086b1c` (tears down audio/BT subsystems, Inferred pre-reset) [rkof 0xbc3be]; then,
   newer or not, reply flags **3** and `FUN_001ad03c(0x62)` = timer 0x62 (1000 ms, Inferred:
   reboot into the staged firmware) [rkof 0xbc3c2–0xbc3ce]. Other endpoint types: flags
   **6** (Inferred "not applied"). Reply 0x0b.

## 5. Personalization (Known structure, Inferred semantics)

- Personalization goes over the same asset messages, with the accessory as *source*:
  `FUN_00076860` (UARPL3Personalization) builds an in-RAM SuperBinary dynamic asset with
  one payload tagged **"IM4M"** (bytes written at [rkof 0x102aa8]) and offers it upstream
  with **0x05** (flags = dynamic) [rkof 0x76b74 → 0x106c38]. The controller pulls it with
  0x06 and the accessory serves 0x07 [rkof 0x104f1e].
- Metadata TLVs added (type, size, value) [rkof 0x76968–0x76b58, 0x102b08–0x102d3a]:

| TLV | Size | Value |
|---|---|---|
| 0x88B29102 | 2 | SuperBinary assetID (BE) (cf. host 0x88B30002) |
| 0x88B29101 | 4 | payload 4cc to personalize (cf. 0x88B30001) |
| 0x88B29103 | strlen | string at ctx+0x2ec (Inferred: manifest prefix, cf. 0x88B30003) |
| 0x88B29104, 0x88B29105 | 4 | literal 0 (r2=0 via `0x1b40bc`) |
| 0x88B29106 | 8 | literal 0 (u64) |
| 0x88B29107 | 8 | stack buffer (not filled in this function) |
| 0x88B29108 | 32 | stack buffer (not filled) |
| 0x88B29109–0x88B2910C, 0x88B2910F | 4 | register left from a log call — no meaningful value |
| 0x88B29118 | 1 | 0 |
| 0x88B29110 (×n) | var | items from list ctx+0x390 (measurements, "pItem->pMeasurement"); sub-TLVs 0x88B29115/0x88B29116 get u16 fields rewritten [rkof 0x102c94] |
| 0x88B2910E | 4 | SuperBinary asset+0x120 |
  Inferred: these TLVs act as a request template (which identity fields the host must
  ask TSS for); no ECID/nonce/chip-id value is read in this builder. Identity values may
  come through vendor property queries (only props 1, 2 wired) — unmapped.
- Delivery back: the host offers a SuperBinary whose payload (tags **FTAB/LTAB/RTAB** or
  **HSML**) carries metadata TLV **0x88B29130**; platform `FUN_000bd0ac` (event 0xc of
  `FUN_00078824` [rkof 0x78914]) writes it to flash file "mani.00"-style storage
  (`0x1e2afb`) and logs "Personalized Manifest" [rkof 0xbd0ac–0xbd304]. TLV **0xCCD28107** on
  the same payloads is hashed into "sha_0"/"sha_1" slots (Inferred payload digest).
  "Releasing Personalization Asset" [rkof 0x785ea].

## 6. Dangerous / irreversible steps

- **0x0a Apply** from endpoint type 0x10/0x11: timer 0x62 → (Inferred) reset; with a newer
  staged version also `FUN_00086b1c` first [rkof 0xbc3a8–0xbc3ce].
- Personalized manifest TLV 0x88B29130 writes flash (`FUN_00195774`) [rkof 0xbd1c0–0xbd22a].
- Payload data writes into staging storage happen during 0x07 handling via asset callbacks
  (asset+0xec/0xf0) — staging region not traced (unmapped).
- An accepted new offer abandons an older staged/staging asset [rkof 0xbdbb8].

## Unmapped / open

- Semantics of 0x1d/0x1e, 0x1f–0x22 tables, 0x88B29104–0x88B2911x names, vendor
  properties 1/2 content, ep[0]/ep[1] numeric limits (runtime), statistics tag 7 field
  meanings, what sets ctx+0x11c (sp+0x14c).
