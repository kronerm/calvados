# UARP in the AirPods firmware

The firmware-update protocol as the pods implement it: AirPods Pro 2
(USB-C), build 9A348, image `rkof` (hashes and tools: [firmware.md](firmware.md)).
Addresses `[rkof 0x…]`. **Known** = read from code; **Inferred** = meaning
guessed (the binary has no message-type names; the names below follow
behaviour and request/ack pairing); **Observed** = on real pods, dated.

What the pods do with what they receive (A/B slots, staging, apply, the
manifest): [update.md](update.md). Lid close, several hosts, resuming, the
case's version: [update-host.md](update-host.md). The other bud and the
case: [sibling-and-case.md](sibling-and-case.md), [case.md](case.md).

## Transports

- **GATT** (Known; Observed on 7B21): characteristic
  `94110001-6d9b-4225-a4f1-6a4a7f01b0de` (write, indicate; firmware.md),
  which BlueZ serves over the classic link too; transport ids 0x10/0x11.
  Every write and indication starts with a **one-byte fragment header**,
  bit 0 set on a message's last fragment [rkof 0x69fba–0x6a03c, write
  callback at `0x1c1428[3]`]. Without it nothing is delivered (a first byte
  without bit 0 just starts buffering). Right after the case reopens, BlueZ
  can refuse the first write ("Not connected") for a moment.
- **AACP opcode 0x4F** (Known; Observed on 9A348), Apple hosts' route,
  transport id 0xd: `04 00 04 00 4F 00 [len u16 LE]` + one whole UARP
  message, ≤ 1007 bytes, no fragment header; the pods answer the same way
  [rkof 0x6d6d0, sender 0xbebc8]. **A 0x4F frame reaching a bud that isn't
  primary aborts it** [rkof 0xbea70]: nothing around a primary switch. The
  pods serve it to one host at a time ([update-host.md](update-host.md#one-host-at-a-time-over-aacp)).

## Observed on real pods

- **GATT probe (7B21)**, `tools/firmware/uarp_probe.py`: version discovery
  proposing 9 answered **4**. Tags: 1 `Apple`, 2 `AirPods Pro`, 3 a bud's
  serial, 4 `X2290/MPMP/MXXC1/*/*`, 5 running `7.1.21.0`, 6 staged
  `0.0.0.0` (none), 7 and 8 zero, 9 empty. Replies number their own
  message ids from 1. Discover Downstream (0x15) got 0x16 status 0 and no
  0x17, also fresh and charging (why: GATT hosts get no downstream,
  [sibling-and-case.md](sibling-and-case.md)).
- **GATT offers (7B21)** of 9A348, with a Python tool since removed (`uarp_offer.py`; asset
  tag 0, flags 1, version and length from the SuperBinary header, 20
  payloads): acked (0x0d, every field echoed), then nothing for 30 s worn
  and 2 min in the case without power: deferred. With the case charging,
  denied at once, flags `0x0202` (the default reason 0x200 [rkof 0x75af0];
  the specific reasons 1, 2, 5 would give `0x0003`, `0x0002`, `0x0007`),
  also after a rescind-all (`0x0c` asset `0xFFFF`). After closing and
  reopening the case (message ids restart at 1: UARP state reset) the same
  offer was **accepted**: so the denials came from state of earlier offers
  that rescinds didn't clear and closing the case did. The pods pulled the
  SuperBinary header (offset 0, 0x2c bytes), the payload headers (0x28
  each) up to `FTAB`, the 13th (`AUCH AUER AUFM AUFR AULB AUPC AUPS AUUF
  AUUT CMAP CMAP CTAB FTAB`), `FTAB`'s metadata (its SHA-256 `0xCCD28107`,
  matching, and `0xCCD28112` = 1), then `FTAB`'s data at once (475 bytes
  at `0x5e7781`): no personalization request, the buds need no ticket.
- **Full update over GATT (Oct 27)**, 7.1.21 → 9.0.348, with
  `uarp_offer.py … full`: case charging and freshly reopened, the phone's
  Bluetooth and the daemon off. The primary pulled exactly `FTAB`
  (7 811 966 bytes with headers and metadata, 475-byte requests,
  ~18 KB/s, ~7.5 min), then flags `0x0001` (fully staged): 7.1.21
  running, 9.0.348 staged. No apply sent: with the lid closed ~30 min the
  case copied the image to the other bud and applied both. Afterwards
  protocol version **6**, running 9.0.348 on both buds, nothing staged;
  0x1D's firmware slots `90.3431000025000000.6807`. The case's own
  firmware wasn't offered.
- **AACP probe (Oct 27, 9A348)**, `tools/firmware/uarp_aacp_probe.py` (its
  own AACP channel, daemon stopped): version 6, running 9.0.348; tags 3/4/9
  differ from GATT's (the other bud's serial, hardware `3.36.32`, tag 9
  `AirPods Pro`); unsolicited Downstream Added (0x17) for id 2 (the other
  bud); the pods offered a 300-byte dynamic asset `MTIC` (flags 0x000a;
  not accepted). No case.
- **Full update over AACP (Nov 14)**, calvados: `calvados firmware update`
  on the laptop, both buds in the charging case, lid open, the phone's
  Bluetooth off. 9A348 → 9B47 offered 11:34:00, acked (deferred) at once;
  the primary pulled 8604 requests of 971 bytes (~26 KB/s) and reported
  fully staged at 11:38:57, no drop or stall. With the lid closed the
  primary held the laptop's link for 30 s, then let go (0x13). Lid opened
  at 12:10: the next session's query read 9.1.47 (0x1D
  `91.3431046004000000.29`), `Done`.

## Calvados (controller side)

`protocol::{uarp, superbinary, update}` and the engine (AGENTS.md has the
details). Only over AACP 0x4F, and only once the session has settled and
5 s after a connect or a primary change (the abort above).
- Sends Sync, version discovery proposing **6** (the case takes an offer
  only at 6), information request tag 5 (running version) while it's
  unknown, the offer (asset id 1, tag 0, flags 1; version, length and
  payload count from the SuperBinary header), data responses (exactly what
  is asked, cut to 989 bytes; unknown asset or offset past the end: status
  0x11), rescind (asset 1), acks 0x0F and 0x18, and the relay 0x1B to the
  case (downstream 1). **Never 0x0A (apply), never a solicitation.**
- The package is checked first as the pods check it (header, ranges,
  `FTAB`'s SHA-256; §4).
- Offer unanswered for 10 s: `NoAnswer` (another host holds the
  endpoint). No request for 60 s: rescind (`NotStarted`, `Stalled`; the
  pods have no timeout of their own). A drop: re-offer the identical
  package (resumes, update-host.md); more than 3 drops without a request
  or 5 min down: `Lost`. Done once the running version reaches the staged.
- `Goal::ProbeCase`: an offer only to hold the pods across a lid close so
  the case tunnels through as downstream 1 and is asked its version; at
  most 10 % of `FTAB` served, never staged, withdrawn once the case answers
  or after 3 min ([case.md](case.md)).

## 0. Framing and plumbing (Known)

- Header, big-endian: `[type u16][payloadLen u16][msgID u16]`, payload at +6.
  Total length ≥ 6 and ≥ payloadLen+6, else status 8 [rkof 0x103f46–0x103f7a].
  msgID stored at `ep+0x3c` [rkof 0x103f74]. Dispatcher `FUN_00103f10`,
  `tbh` table at 0x103fa4 (cases 0..0x26; above → default) [rkof 0x103f98].
- Tx builder `FUN_00107302(ctx, ep, type, totalLen, expectAck)` writes type,
  len-6, msgID=0 [rkof 0x107302]; `FUN_001073ac` queues/sends. Outlined
  helpers: `0x1a931e` = BE16@msg+6, `0x1a98b8`/`0x1a9e50` = BE16@msg+8,
  `0x1a9356` = type of a pending tx entry, `0x1ac3ea` = mark it answered.
- Two UARP instances, created at boot [rkof 0x154600, 0x154684] by `FUN_00106554`:
  - **accessory role** (role 0), ctx `0x200b2728`: the one hosts talk to
    (rx `FUN_00076e80` → `FUN_00103f10(0x200b2728, …)` [rkof 0x76e80]);
  - **controller role** (role 1), ctx `0x200b2ac8`: towards downstream
    devices; almost no callbacks (struct re-zeroed [rkof 0x154660]).
- Config copied to ctx+0 (0x28 bytes); **ctx+0xc = max protocol version =
  6** [rkof 0x154550, 0x15455e] (library default 9 if 0 [rkof 0x106554]).
  Callback struct (0x124 bytes) copied to ctx+0x2c.
- Ack tracking (ctx+0x150) is off: cleared when callbacks ctx+0xb0/b4/b8 are
  all null [rkof 0x106586–0x10659a], as for the accessory instance. So
  responses aren't matched against requests and there are no retransmits.
- Remote endpoint config (`FUN_0010661c` via `FUN_00076cb0`): ep[0] = max
  data *response* payload sent, ep[1] = max length per data *request*,
  ep[2] ≥ ep[1], ep+0xc version (0 → 6), ep+0x24/0x25 = 0; values from the
  transport at runtime. ep+0x38 = "transfer active" (init 1) [rkof 0x10661c].
- Status codes (values Known, meanings Inferred): 0 ok, 8 bad length, 9 bad
  length/no matching request, 0xb no memory, 0xe no handler, 0x11 unknown
  asset, 0x16 bad offer flags, 0x1a/0x21 bad pause-resume flags, 0x1c unknown
  info tag, 0x27 buffer too small, 0x3a unknown downstream id, 0x41 data overflow.

### Accessory-instance callbacks [rkof 0x15456e–0x1545d6] (Known)

| ctx off | fn | used by |
|---|---|---|
| 0x48 / 0x50 | 0x75670 / 0x7569c | pause / resume (msg 8): return 0 |
| 0x58 / 0x5c | 0x756c8 / 0x756ce | SuperBinary / dynamic asset offered (msg 5) |
| 0x60 | 0x756d4 | apply staged assets (msg 0xa) |
| 0x68,0x70,0x78,0x80 | 0x7572c,0x75794,0x757fc,0x75870 | info tags 1–4 |
| 0x88,0x90 | 0x758e8,0x75930 | info tags 5,6 (versions) |
| 0x98 | 0x75978 | info tag 8 (last error) |
| 0xa4 | 0x75988 | msg 0x11 solicitation |
| 0xbc | 0x75aa4 | version negotiated (msg 1) |
| 0xc0 | 0x759f4 | info tag 9 |
| 0xd8 | 0x75a68 | (returns 0x33) |
| 0x108 | 0x78134 | msg 0x15 Discover Downstream |
| 0x118 | 0x78280 | msg 0x1b relay to downstream |
| 0x11c | [sp+0x14c] if non-null | msg 0x1d |
| 0x14c | 0x781c0 | downstream removed |
| 0x174 | 0x105ec4 | vendor msg 0xFFFF |

Not wired (the handler acks or returns 0xe): 0x4c/0x54 (msg 0xe), 0x64 (0xb),
0xa8/0xac, 0x10c/0x144 (0x17), 0x110/0x114 (0x19/0x1a), 0xc8 (0x20), 0xcc
(0x22), 0xd0 (0x23), 0xd4 (0x25), 0x120 (vendor).

## 1. Message table

C→A = controller (host) to accessory, A→C the reverse. Offsets from the
message start (payload at +6).

| Type | Dir | Name (Inferred) | Payload | Reply / notes |
|---|---|---|---|---|
| 0x00 | both | Sync | empty | Accessory: flush this endpoint's tx queue, reply type 0 (`FUN_00105e20`) [rkof 0x1053aa→0x105e20]. Controller: reset ep, send type 1 with version=min(ep+0xc,9) [rkof 0x103ff6–0x104088]. |
| 0x01 | C→A | Version Discovery Request | +6 u16 controller version; len≥2 | Reply **0x02**, len 0xc: +6 u16 status 0, +8 u16 max(1, min(req, 6)), +10,+11 bytes ep+0x24/0x25 (=0) **only if negotiated > 5**, else payload 4 bytes [rkof 0x104a74–0x104b22]. Stores ep+0x2c; calls ctx+0xbc. |
| 0x02 | A→C | Version Discovery Response | +6 u16 status, +8 u16 version, opt. +10,+11 → ep+0x2e/0x30; len≥4 | status≠0 → error 2; version clamped 1..9 [rkof 0x1047fe–0x104860, 0x10563a] |
| 0x03 | C→A | Accessory Information Request | +6 u32 tag; len≥4 | Reply **0x04**: +6 u16 status, +8 u32 tag, +12 u32 length, +16 value. Error → status only (0x1c unknown tag, 0x27 too small) [rkof 0x1048aa–0x104982, 0x105534–0x105b8e]. Tags §3. |
| 0x04 | A→C | Accessory Information Response | +6 u16 status, +8 u32 tag, +12 u32 len, +16 value; len≥10, len+16 ≤ total | rx: ctx+0x6c..0xc4 (tag 5/6 versions at ep+0x62/+0x72) [rkof 0x10458c–0x104634] |
| 0x05 | both | Asset Available (offer) | +6 4cc asset tag, +10 u16 flags (bit0 SuperBinary, bit1 dynamic), +12 u16 assetID, +14 version (4×u32), +0x1e u32 length, +0x22 u16 numPayloads; len≥0x1e | Reply **0x0d** len 0x24 echoing all fields [rkof 0x104cb8–0x104e30]. bit0 → ctx+0x58, bit1 → ctx+0x5c, neither → 0x16. A dynamic offer of the same tag/version reuses the slot. Sent by A as `FUN_00106c38` (e.g. §5). |
| 0x06 | A→C (C→A when A is the source) | Asset Data Request | +6 u16 assetID, +8 u32 offset, +12 u16 length (len≥8 on rx) | Sent by `FUN_00102eb4`: length = min(remaining, window, ep[1], 0xffff); not while paused (ep+0x38==0 → 0x1f) or asset+0x49 set. rx: reply **0x07** [rkof 0x104f1e–0x105000]. |
| 0x07 | C→A (also A→C) | Asset Data Response | +6 u16 status, +8 u16 assetID, +10 u32 offset, +14 u16 requested, +16 u16 actual, +18 data; len≥0xc, actual+0x12 ≤ total | Built on rx of 6: status 0x11 if asset unknown or offset > size; actual = min(req, ep[0], size-offset). rx: status≠0 → abandon the asset (processing flags 4); checks offset==asset+0xfc, req==asset+0x100, actual≤req, buffer fits (else 0x41); copies, then the next request or section-complete handler [rkof 0x104986–0x104a70, 0x10587a–0x105e10]. |
| 0x08 | C→A | Data Transfer Notification (pause/resume) | +6 u16 flags: 1 pause, 2 resume; len≥2 | Reply **0x0e** echoing flags. Pause: ep+0x38 1→0, ctx+0x48. Resume: 0→1, re-issue data requests for the ep's assets, ctx+0x50. Otherwise 0x21 [rkof 0x105022–0x1050b8, 0x1052ee]. A sends 8 as controller (strings 0x1e10b0/0x1e106f). |
| 0x09 | A→C | Asset Processing Notification | +6 u16 assetID, +8 u16 flags | Sent by `FUN_00103e32`. Low byte (values Known, meaning Inferred): 1 fully staged [rkof 0x106b58], 2 denied [rkof 0x1069d0], 3 abandoned (0x700/0x800 when superseded) [rkof 0x106a12, 0xbdbb8], 4 corrupt (bad SuperBinary header / data error) [rkof 0x103114, 0x104a64]; the reason in the high byte. rx (controller): reply **0x0f** [rkof 0x1046a8]. |
| 0x0a | C→A | Apply Staged Assets | none checked | Reply **0x0b**: +6 status (cb result, 0xe if none), +8 flags [rkof 0x105004, 0x1053c6]. §4. |
| 0x0b | A→C | Apply Response | +6 status, +8 flags; len≥4 | rx: ctx+0x64 (unset) [rkof 0x104542] |
| 0x0c | C→A | Asset Rescinded | +6 u16 assetID (0xFFFF = all); len≥2 | Reply **0x10** echo. Marks asset(s) rescinded (+0x48), asset+0x80 cb; ctx+0xa8 if all & none [rkof 0x10463c–0x1046a6, 0x1055e8]. |
| 0x0d | A→C | Asset Available Ack | echo of 5; len≥0x1e | rx: clears pending 5 with the same assetID [rkof 0x104ed4] |
| 0x0e | A→C | Data Transfer Notification Ack | +6 u16 flags; len≥2 | rx: 1 → ctx+0x4c, 2 → ctx+0x54, else 0x1a [rkof 0x1044ba] |
| 0x0f | C→A | Processing Notification Ack | +6 assetID, +8 flags; len≥4 | rx: asset+0x98 cb, release [rkof 0x104864] |
| 0x10 | A→C | Rescinded Ack | +6 assetID; len≥2 | rx: ctx+0xac (all) / asset+0x84 [rkof 0x104474] |
| 0x11 | C→A | Dynamic Asset Solicitation | +6 4cc tag; len≥4 | Reply **0x12** (+6 u32 status 0, +10 tag) [rkof 0x104b9a–0x104c04]; ctx+0xa4 → 0x75988: tags HSML, VERS, MTIC (platform `FUN_000bddcc`; LOGS/CRSH listed but rejected on that path); failure → **0x13**(tag, 0x1a); success: the platform later offers the asset with 5 (Inferred). |
| 0x12 | A→C | Solicitation Ack | +6 u32 status, +10 tag; len≥8 | rx: clears pending 0x11 by tag [rkof 0x104fc6] |
| 0x13 | A→C | Solicitation Result (Inferred) | +6 4cc tag, +10 u16 status | rx: reply **0x14** echo; asset by tag gets +0x9c cb; no asset → 3 [rkof 0x1050e8–0x105186] |
| 0x14 | C→A | Solicitation Result Ack | +6 tag, +10 status; len≥6 | rx: asset+0xa0 cb [rkof 0x104c08] |
| 0x15 | C→A | Discover Downstream (log "Discover downstream accesories" 0x1e1ceb) | none | Reply **0x16** status 0; ctx+0x108 announces each attached downstream with **0x17** [rkof 0x104e9c, 0x78134] |
| 0x16 | A→C | Discover Ack | +6 status; len≥2 | [rkof 0x1050bc] |
| 0x17 | A→C | Downstream Added | +6 u16 downstream id; (version ≥ 9: TLVs `[u32 type][u32 len][val]`) | Sent by `FUN_00106f40`. rx: reply **0x18** (+6 status 0, +8 id) [rkof 0x105188] |
| 0x18 | C→A | Added Ack | +6 status, +8 id; len≥4 | [rkof 0x1047d0] |
| 0x19 | A→C | Downstream Removed | +6 u16 id | Sent by `FUN_00106fb6`; rx: reply **0x1a** (status 0/0x3a, id) [rkof 0x10472e] |
| 0x1a | C→A | Removed Ack | +6 status, +8 id; len≥4 | [rkof 0x1052d8] |
| 0x1b | both | Downstream Relay | +6 u16 downstream id, +8 a whole UARP message; len≥8 | C→A: to the downstream's send function (0x78280); A→C: `FUN_00105e66` wraps downstream traffic. Ack **0x1c** only if negotiated version < 5 [rkof 0x10437c–0x10440a, 0x105e66]. |
| 0x1c | — | Relay Ack | +6 status, +8 id | [rkof 0x10520a] |
| 0x1d | C→A | unmapped (downstream family) | none | Reply **0x1e** status 0; ctx+0x11c [rkof 0x105242] |
| 0x1e | — | ack of 0x1d | | [rkof 0x105032] |
| 0x1f | C→A | id list query (unmapped) | none | Reply **0x20**: status 0, u16 count, u16[count] from ctx+0x17c (stride 8) [rkof 0x104b26]; the table is only ever cleared [rkof 0x1067ba] → likely count 0 |
| 0x20 | — | reply of 0x1f | +8 count, +10 u16[] | rx: ctx+0xc8 (unset) [rkof 0x10505a] |
| 0x21 | C→A | entry query by id (unmapped) | +6 u16 id; len≥2 | Reply **0x22**: status 0, id, u16 count, count×4 bytes [rkof 0x104502, 0x10540e] |
| 0x22 | — | reply of 0x21 | +8 id, +10 count, +12 data | rx: ctx+0xcc (unset) [rkof 0x10440e] |
| 0x23 | C→A | Bulk Info Query | +6 u16 id, +8 4cc context, +12 u32 count (<2^30), +16 u32[count] TLV types; len≥10 | Reply **0x24** (status, id, 4cc, count, types) [rkof 0x1041fa–0x104326]; values follow in 0x25 (update.md §5). |
| 0x24 | — | Bulk Info Query Ack | len≥0xc | [rkof 0x10432c] |
| 0x25 | A→C | Bulk Info Response | +6 u16 id, +8 4cc context, +12 TLVs | Sent by `FUN_00107188` from `FUN_000771cc` [rkof 0x107188, 0x77366]. rx: reply **0x26** [rkof 0x104136] |
| 0x26 | C→A | Bulk Info Response Ack | len≥8 | [rkof 0x10527e] |
| 0xFFFF | both | Vendor Specific | +6 OUI `BC 09 63`, +9 u16 vendor type, +11… | `FUN_00105ec4`. Vendor 0x20 = property query (+0xb u32 id 1..0x21); reply 0x21: +0xb u32 status (0x1a unsupported), +0xf id, +0x13 len, +0x17 value. Only ids 1 and 2 wired (update.md §5) [rkof 0x1545de–0x1545e4, 0x7832c, 0x78342]. |

Types 0x27–0xFFFE: default branch [rkof 0x10408c].

## 2. Handshake (Known)

1. Controller sends **Sync (0)**; the accessory flushes its queue and answers Sync [rkof 0x105e20].
2. Controller sends **0x01** with its version; the accessory answers **0x02**
   with `max(1, min(proposed, 6))` [rkof 0x15455e] (the controller role caps
   its own proposal at 9 [rkof 0x104060]). 7B21 answered at most 4.
3. Version gates: > 5 adds 2 bytes to 0x02; < 5 requires 0x1c acks for
   relays; ≥ 9 adds TLVs to 0x17.

## 3. Accessory information tags (msg 3/4) (Known source, Inferred names)

| Tag | Source | Format |
|---|---|---|
| 1 Manufacturer | RAM 0x20006634+0x14 [rkof 0x7572c] | ASCII, no NUL |
| 2 Model name | 0x20006634+0x54 [rkof 0x75794] | ASCII |
| 3 Serial number | ctx+0x2ac [rkof 0x757fc] | ASCII |
| 4 Hardware version | 0x20006634+0x94 [rkof 0x75870] | ASCII |
| 5 Active FW version | ctx+0x370, 16 bytes [rkof 0x758e8] | 4×u32 BE major.minor.release.build |
| 6 Staged FW version | ctx+0x380 [rkof 0x75930]; 0xFF×16 if no source [rkof 0x105aae] | same |
| 7 Statistics | ep+0x3e..0x4c [rkof 0x10490a] | 4×u32 BE (Inferred: counters) |
| 8 Last error | ctx+0x2a0, ctx+0x2a8 [rkof 0x75978] | 2×u32 BE (default 0xFFFFFFFF×2) |
| 9 (unnamed) | ctx+0x32c [rkof 0x759f4] | ASCII |

Tags 1–4 and 9 fall back to "UNKNOWN" (0x1e8370) without a callback
[rkof 0x105e14]. Other tags → status 0x1c.

## 4. Firmware flow, accessory as target (Known unless marked)

1. **0x05** offer → 0x0d ack → `FUN_000756c8` logs "%s Offered <%u> <Version …>
   Payloads<%d>", merges with an earlier asset of the same contents ("Merging
   assets"), asks the platform `FUN_000bd938`: 1 accept / 2 deny / 3 defer
   [rkof 0x756c8, 0xbd938]. Deny → msg 9 flags `reason|2` (reason 1..5,
   default 0x200); accept failure → 0x900|2. A newer offer abandons an older
   one (3|0x700 or 3|0x800) [rkof 0xbdbae–0xbdbb8]. The cases, by transport:
   [update-host.md](update-host.md#several-hosts).
2. Accept (`FUN_00106810`) → pulls with **0x06**/**0x07**: the SuperBinary
   header (0x2c bytes; format ≤ 4, header length 0x2c, offsets/lengths
   within the size, else flags 4) [rkof 0x103000], the payload headers (0x28
   each) [rkof 0x1031e8], metadata, then each payload's data [rkof 0x1034b8].
   The buds take only `FTAB` (update.md §3). Max per request = ep[1].
3. Pause/resume **0x08**; rescind **0x0c**.
4. Fully staged → **0x09** flags 1 [rkof 0x106b50], "Asset <%u> fully staged".
5. **0x0a** apply (calvados never sends it): `FUN_000756d4` → `FUN_000bc390`
   [rkof 0xbc390]: only from transport 0x10/0x11 (`bic #1; cmp #0x10`
   [rkof 0xbc3a0]); if staged is newer than running (`FUN_000bfd5c`,
   `FUN_001082e4`) `FUN_00086b1c` (NVRAM state save, Inferred pre-reset)
   [rkof 0xbc3be]; then, newer or not, flags **3** and timer 0x62 (1000 ms:
   boot into the staged slot, update.md §6) [rkof 0xbc3c2–0xbc3ce]. Other
   transports: flags **6** (Inferred "not applied"). Reply 0x0b.

## 5. Personalization (Known structure, Inferred semantics)

- Same asset messages, the accessory as *source*: `FUN_00076860`
  (UARPL3Personalization) builds an in-RAM dynamic asset with one payload
  tagged **"IM4M"** [rkof 0x102aa8] and offers it with **0x05** (flags =
  dynamic) [rkof 0x76b74 → 0x106c38]; the controller pulls it (0x06/0x07).
  Requested only for payloads whose metadata has `0x88B29100` ≠ 0; in the
  stock package only `R1FW` (update.md §4–5).
- Metadata TLVs [rkof 0x76968–0x76b58, 0x102b08–0x102d3a]:

| TLV | Size | Value |
|---|---|---|
| 0x88B29102 | 2 | SuperBinary assetID (BE) (cf. host 0x88B30002) |
| 0x88B29101 | 4 | payload 4cc to personalize (cf. 0x88B30001) |
| 0x88B29103 | strlen | string at ctx+0x2ec (Inferred: manifest prefix, cf. 0x88B30003) |
| 0x88B29104, 0x88B29105 | 4 | literal 0 |
| 0x88B29106 | 8 | literal 0 |
| 0x88B29107 | 8 | stack buffer, not filled here |
| 0x88B29108 | 32 | stack buffer, not filled here |
| 0x88B29109–0x88B2910C, 0x88B2910F | 4 | a register left from a log call |
| 0x88B29118 | 1 | 0 |
| 0x88B29110 (×n) | var | measurements from list ctx+0x390; sub-TLVs 0x88B29115/0x88B29116 rewritten [rkof 0x102c94] |
| 0x88B2910E | 4 | SuperBinary asset+0x120 |

  So the asset is a template naming the identity fields; the values come
  from Bulk Info Query 0x23/0x25 (update.md §5).
- Delivery back: a SuperBinary whose payload (**FTAB/LTAB/RTAB** or
  **HSML**) carries metadata TLV **0x88B29130**; `FUN_000bd0ac` (event 0xc
  of `FUN_00078824` [rkof 0x78914]) writes it to `mani.00` and logs
  "Personalized Manifest" [rkof 0xbd0ac–0xbd304]. TLV **0xCCD28107** on the
  same payloads goes to NVRAM `sha_0`/`sha_1` (update.md §2).
  "Releasing Personalization Asset" [rkof 0x785ea].

## 6. Irreversible steps

- **0x0a Apply** from transport 0x10/0x11: timer 0x62 → (Inferred) reset;
  with a newer staged image also `FUN_00086b1c` first [rkof 0xbc3a8–0xbc3ce].
- Manifest TLV 0x88B29130 writes flash (`FUN_00195774`) [rkof 0xbd1c0–0xbd22a].
- Payload data is written into the inactive A/B slot during 0x07 handling
  (update.md §3).
- An accepted newer offer abandons an older staged/staging asset [rkof 0xbdbb8].
- Never offer an asset tagged `MTIC`, `LOGS` or `CRSH`: that path aborts the
  pods [rkof 0xbcd82, 0xbcdca] (AGENTS.md).

Risk per step: [update.md](update.md) §8.

## Unmapped

- 0x1d/0x1e, the 0x1f–0x22 tables, what sets ctx+0x11c (sp+0x14c).
- ep[0]/ep[1] numeric limits (runtime; observed requests: 475 bytes over
  GATT, 971 over AACP), tag 7's fields.
