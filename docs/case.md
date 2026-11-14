# The case: its firmware, how it's updated, and what a host can reach

AirPods Pro 2 (USB-C). The case has two processors: the MCU (`STFC`; `STFW`
on the other hardware variant, same strings) and its BLE chip (`NERF`, an
nRF). Hashes, tools and the buds' images: [firmware.md](firmware.md); the
buds' sync and the case's bud-to-bud decision:
[sibling-and-case.md](sibling-and-case.md). Static reads are cited
`[image 0xADDR]`, always with the build: 9A348's and 7B21's STFC differ,
and so do 9B47's and 7B21's NERF. STFC is analysed at base 0x08001800,
NERF and `rkof` at 0. Known = read from code or data, Inferred = marked,
Observed = seen on our pods (dated, local time).

Strings checked across images: "mismatch fw ver", "Couple", "Deny %s(%u)
offer" (STFC), "Already paired. Reject request.", "Added App UARP Service"
(NERF).

## Short version

- Our case runs **7B21's case firmware** (STFC 71.8, NERF 70.59); the buds
  run 9A348, then 9B47. A buds-only update never touches the case.
- 7B21's case is updated only over its own BLE UARP service, which exists
  only once the case is Find My paired: only its owner's Apple device can
  update it. Its lid-close tunnel through a bud is switched off by a flash
  constant.
- 9A348's case (STFC 90.390) also opens a UARP tunnel through the primary
  bud ~2 s after the lid closes; a host holding an offer across the close
  would see it as downstream 1. Never seen on our 7B21 case, and it can't
  happen there.
- No host-to-case path exists through a bud's AACP, ATT or CLI, nor over
  USB without the case's CLI.

## Which firmware runs where

- Buds: shipped on 7B21 (7.1.21), updated to 9A348 over GATT (Oct 27) and
  to 9B47 over AACP (Nov 14) ([uarp.md](uarp.md)).
- Case: STFC 71.8.0.0 and NERF 70.59, 7B21's. Known: the case's USB
  descriptor (Observed Nov 13) has bcdDevice 0x4708, which 7B21's STFC
  fills from its version word [7B21 STFC 0x08032df0]. The package:
  `…/2024/patches/072-16842/4936FD7D-958B-4BA8-89F0-6891E72ECD89/com_apple_MobileAsset_UARP_A2968/609da25d4fc018fb870d598846665b6c96a95334.zip`
  (URL from AppleDB's `osFiles/Bluetooth Headset Firmware/7x/7B21.json`;
  STFC 511 168 bytes).
- 9A348's package carries NERF 90.264, STFC/STFW 90.390, R1FW and FTAB
  9.0.348 (`bver`).
- No host learns the case's version: the pods' 0x1D carries none; the case
  answers B2P 0x128 with its versions [7B21 STFC 0x0801c000], and nothing
  relays that to a host.
- No case-vs-bud version check exists in either STFC or in `rkof` (below),
  and our set runs mismatched.

## Payloads and who takes them (Known, tag strings; 9A348 package)

- Buds: only `FTAB` (+`HSML`) is staged; the filter (fPayloadReady
  [rkof 9A348 0xbcfa8]) takes FTAB, LTAB, RTAB, HSML. Every other payload
  goes through 0x76860 → FUN_00076bd4, which only moves to the next payload
  header (0x106ac0) or finishes the asset (0x103e32) [rkof 0x76bd4]:
  headers pulled, data never. Matches the live transfers.
- Case MCU: tag list [9A348 STFC 0x08071f5a..0x08071f9b, file 0x7075a]:
  R1FW, IM4M, NERF, STFC (STFW on the other variant), AUFM, AULB, AUPS,
  AUPC, AUCH, AUER, AUUT, AUFR, AUUF. The AU* are the case speaker's sounds
  (file names aufm..auuf at [STFC 0x080719ac]).
- CMAP and TMAP are NSKeyedArchiver bplists ("sections",
  "AppleModelNumber", "decoderId"); Inferred host-side decoder maps. CTAB is
  FTAB compressed (16 KiB chunk table 0xCCD28113, decompressed size
  0xCCD28119 = FTAB's size, same SHA-256). None of the three occurs in
  `rkof`, `rkos`, STFC, STFW or NERF.
- Case payloads carry 0xCCD28107 (SHA-256), 0xCCD28106 (256-byte
  signature) and 0xCCD28105 = 2; STFC checks the signature ("Attempting Dev
  Sig Check" FUN_08013784, "Sig Check fail!"; TLV ids at STFC file
  0x13910-0x13918). Signed, not personalized; only R1FW needs a TSS ticket
  ("Add IM4M to hdr").
- The case stages and applies them itself: "ST pend", "NRF pend", "R1
  pend", "Wr to partition %d", "Apply ST", "Apply BLE" [9A348 STFC
  0x0800349c] / "No pend BLE img to apply" / "nRF apply fail", "Apply R1
  image, pend bank %d" (UWB, A/B banks). The MCU flashes the nRF over SWD
  ("nrfSWD", "!! Upd nRF !! Req: %d.%d.%d", "Min BLE %s not set" [9A348
  STFC 0x08036c22-0x08036c3e], "BLE exp:%d.%d.%d rcvd" [0x08016d3a]) and
  requires a minimum NERF version.
- The case's own apply: refused while the lid is open or the case is at
  ≤ 10 %, which logs "No Apply %u %u" and sets `[0x20034fc8+0x37]` [9A348
  STFC 0x08012e58]; retried by FUN_08012f34 15 s after the next close,
  cleared when an apply starts. What starts the first apply after a staging
  without a host's 0x0A is open.

## The bud never holds case firmware (Known, rkof 9A348)

`rkof` has store-and-forward files for other products' SuperBinaries
("case", "ioe1", …; [sibling-and-case.md](sibling-and-case.md)) but nothing
writes them: every file write site is firm.00/01, mani.00, 0x19b90's file
or remotefs. The offers end at the file open. Tags CASE, IOE1, IOHM, IOHS,
ESM1, PDFW appear only in the asset-release handler FUN_000bccd0 [rkof
0xbcec8-0xbcf0c]; STFC, NERF and R1FW never occur in `rkof`. The `bver`
parser FUN_000a5770 knows compatibility fields "csv1:", "ioe1:", "csv2:",
"ioe2:", "updv:" [strs 0x1e2730..0x1e2748], but 9A348's `bver`
(`90.3431000025000000.6807|date:2026 8 17|updv:348|chip_revision_b0`)
has none. Inferred: shared RTKit platform code kept for other products.

## Update routes by case build

| Route | 9A348 STFC (90.390) | 7B21 STFC (71.8, ours) |
|---|---|---|
| Tunnel through the primary bud on lid close (host's downstream 1) | on: config +0xc = 1 | off: config +8 = 0 |
| BLE (NERF's UARP service, tid 2) | not denied (config +0xd = 1); needs Find My | not denied (config +8 = 0); needs Find My |
| USB (tid 3) | routing Disabled by default | routing Disabled by default |

### The case's UARP accessory (9A348 STFC; Known code, tid meanings Inferred)

- UARP data sessions are B2P data sessions ("DS"; B2P = the case-bud
  protocol, "locust", in both `rkof` and STFC) keyed by **tid 0..3**. tid =
  bud index for the buds (0 "R Bud", 1 "L Bud" [0x0806e0ec]); tid 2 the
  BLE chip, tid 3 a wired path (Inferred from the deny labels and the
  priorities). Per-tid buffers 0x20023730 + tid·0xf4 (MTU 0xf4)
  [0x08011f32-0x08011f48].
- sid 3 = the case's UARP over a bud link (opened on event 0x2805e,
  `0x0802a2ac(tid, 3)` [0x08011fc6-0x08011fd4]); sid 1/2 = the B2B "Bud
  server"/"Bud client" [0x08037878 → 0x080379dc].
- Rx 0x08012129 posts 0x28057 only for the current UARP host
  (`[0x200015d4+0x3c] == tid` [0x0801214c]). One host at a time
  (`[0x200015d4+0x40]`, 4 = none), picked by FUN_08016acc: tid 0/1
  priority 5, tid 2 priority 1, tid 3 priority 9. "Init Ctrl" 0x080158bc
  takes a tid only when none is current.
- Protocol version: reply min(proposed, cap), at least 1, cap = init
  option +0xc (9 when 0) [STFC 0x0804b520-0x0804b52c, 0x0804c3a0; the
  same library in rkof 0x104a88-0x104a96, 0x106554]; the case sets 6
  (0x080158bc, passed at 0x08015b1a). The tunnel gate below wants > 5, so
  **the case accepts an offer only at protocol 6**.
- Offer handler FUN_08015138 (assetOffered, via 0x08058a5c):
  - dynamic asset (type 2): denied unless in case-OTA state;
  - SuperBinary while applying (`[0x20034fc8+0x35] & 0xfb`): "%s state:%s
    host:%s in assetOffered", denied;
  - tid 0/1: needs config +0xc == 1, and with +0xd == 1 also
    `[0x200350c8+0x46] > 5` (set by FUN_08015378, "Prot %d %s"; Inferred
    the negotiated protocol version), else "Deny tunnel(%u) offer"
    [0x08015204-0x08015228];
  - tid 2: "Deny BLE(2) offer" when config +0xd == 0 [0x0801522e,
    0x080152a8-0x080152b4];
  - tid 3: no version check (event 0x1c00e) [0x08015294];
  - otherwise only newer than active and not already pending
    (FUN_08036e68, "A:%d.%d.%d.%d O:%d.%d.%d.%d") [0x08015244-0x08015290].
  No Find My or host-identity check on this path (searched, not
  exhaustively).
- Tag 5 (accessory info, Init Ctrl [0x08015904-0x08015a3e]): "AirPods Pro
  Case " + suffix, and 16-byte active and pending versions (FUN_08036e68,
  "A: %d.%d.%d.%d P: …"), combined from component versions (0x08004aac,
  0x0803688e); Inferred 4 × u32 BE, exact value not established.
- Config: `[0x20035244+0x70]` = 0x20001790 (= 0x20001748+0x48), init data
  copied from flash 0x0807c8b4 [0x08049132-0x0804914c], written only at
  0x080121c6: +0 a function 0x08004c01, **+0xc = 1** (tunnel; also the
  "tunnel:%d" of "bud fwup cond"), +0xd = 1, +0xf = 0 (the B2B
  manifest-required flag `0x20001748+0x57`), +0x11 = 1. Same on every case.
- Routing (CLI "routing", [0x08048e8c]): transports UART, USB, TUNNEL
  [0x20035884+0x60/0x64/0x68], types Disabled/CLI/FWUP/B2P/CLI
  REMOTE/Internal_CLI [names 0x0806e24c]. Defaults [0x0803dd18]: **UART =
  CLI, USB = Disabled, TUNNEL = Disabled**. USB becomes FWUP only for
  "Wired bud, tid:%s" [0x08037598-0x080375b2], B2P on pass-through off
  [0x08035ede], CLI REMOTE on pass-through on [0x08043018]. Inferred:
  factory paths.
- A case OTA in progress blocks bud updates when tunnelled ("bud fwup cond
  … tunnel:%d, caseOTA:%d" [0x08035838]).

### 9A348 STFC: the lid-close tunnel (Known code)

The case opens the tunnel by itself; no bud or host message asks for it.

- The only publisher of 0x2805e ("Tunnel avail") is [0x08012cf2-0x08012cf8];
  its only subscriber [0x08011f0a] runs 0x08011fc4: `0x0802a2ac(tid, 3)`
  ("Open tid:%s ds:%d") → 0x0802a22c → B2P message 0x10 {ds 3, state 1} to
  that bud via 0x0802a328.
- It runs at the expiry of a 2 s timer (0x1e8480 µs; timer 0x20036188,
  event 0x1c006, case 0x080126da of FUN_080123b0's table 0x0801243c). The
  timer starts on lid close (0x28030 arg 0, from the lid handler 0x0803c804
  [0x0803c828, 0x0803c870] → [0x08012adc]) or at boot with the lid closed
  (0x28013 [0x080129ac-0x080129cc], posted from [0x08007322]), both only
  with config +0xc == 1 [0x08012b9c-0x08012bbc] and the No-Apply flag
  `[0x20034fc8+0x37]` clear (else the case's own apply runs). Lid open
  stops it [0x080125b2-0x080125ba].
- At expiry [0x080126da-0x08012718]:
  1. lid closed (`[0x200350c8+5]` == 0; set by 0x0803c7c8, "Lid
     Opened/Closed");
  2. coupled, FUN_0801d8e0: both buds present and valid (valid = 1 for pid
     0x2024 [0x08026c84]), coup_st (S1 bits 1-2) 0 on both, coupling state
     `[0x2003533c+0x4c]` not 1..5 (names 0x0806cd0c; started only by a
     missing peer, a new case, bud connection events, a B2P command or the
     CLI, never the lid), each bud's record holding the other's address
     (+0/+6). The records are polled (B2P 0x6a, ≤ 1 s; reply 0x6b at
     0x08026924) and cleared only when a bud is removed (FUN_0801fa30);
  3. lid-event counter `[0x200350c8+0x44]` ≥ 2 (incremented on every lid
     event [0x0801258c]; Inferred: one open and one close since boot);
  4. a role claimant, FUN_0801e074: per tid, present and cached S0 bit 1
     (the stored primary); one claimant → it, both → 2, mapped to tid 0
     [0x08012708], none → nothing;
  5. that bud's S1 bit 0 set (`[0x2002658c + tid·0x40 + 0x16]`).
  On failure: no log, one retry 1 s later while `[0x20036188+0x1c]` is set
  (cleared by the retry, re-armed on lid open [0x080125b8]); then nothing
  until the next close. No attempt counter. The same retry also runs after
  a successful open on tid 1 [0x08012d06-0x08012d1a].
- Not inputs: charging, battery, a pending case OTA (apart from +0x37), the
  host type, any bud request, bt_prog, bud versions, the offer.
- The send helper 0x0802a328 passes 0x10 only to a bud whose case-side
  state (`[0x200356a0+bud·4+0x50]`, names 0x0806cd2c) is first connect,
  charging, not charging or disabled; in other states it drops it silently.
  At close +2..3 s the primary is charging or not charging (a close cancels
  a running presence check and re-arms it for 15 s [0x0801f190-0x0801f1ac];
  prepare-ulp takes ~30 s), so this doesn't block it. State table 12×12 at
  0x0806cd80 (entries 0x0806ce40, exits 0x0806ce10; 30 s presence limit at
  0x20026710). Queued sends (0x0801c4ec) have a per-bud mode
  `[0x20026580+bud·4]` (0 send now for charging/not charging, 1 fail only
  in idle, search, presence; levels FUN_080209c0 [0x0801c6fc]).
- The DS descriptor (FUN_0802a380: 0x20026e18 + tid·0x2c0 + idx·0x58, +8
  open) is RAM only, zeroed at boot [0x080490c8-0x08049118]. An open writes
  a timestamp; +8 = 1 only on the bud's connect response with state 1
  [0x0802664a]; any other state marks it closed [0x08026616-0x08026644,
  0x08026bf8]. Also closed by "Close tid", data errors, bud removal
  (0x08020944) and a 14 s idle timeout (+0x20 = 14000, "tid:%s ds:%d idle
  TO" [0x08028b5e]). It can't stay stuck open.
- With the tunnel up: host 0x1b to id 1 → rkof 0x78280 → the slot's sender
  0xbe929 on transport 0x03 (MTU 0xf2) → case DS 3. bt_prog (S0 bit 6, set
  at "transport 3 up" [rkof 0x15f6e4 via be420]) keeps the case from putting
  that bud into ULP [STFC 0x08022482].

### 7B21 STFC (71.8): the tunnel is switched off (Known)

- "Tunnel Available" (9A348: "Tunnel avail") is logged by FUN_08011d00 on
  event 0x170006 [0x08011eec], checking lid closed (`[0x200322a4+0xe]`),
  coupled (FUN_08023908) and a role claimant (FUN_08023f70); no lid
  counter, no S1 bit, no retry. It posts 0x1c0052 → 0x08011ad4 → B2P 0x10
  {3,1}, the same message (DS table 0x08069b44).
- 0x170006 comes only from timer 0x20033120, started on lid close
  (0x0801223a) or 0x1c0011 (0x08011de6), both only if
  `[[0x20032424+0x1c]+8]` != 0. That pointer is written once (0x08011b24)
  with the flash constant 0x080635fc, whose +8 is **0**. A close starts only
  the 15 s timer of the case's own apply.
- The same byte gates "Deny offer from BLE" (offer handler FUN_080141f4,
  0x080142c0): 0, so BLE (tid 2) offers pass; tid 3 has no version check.
  "bud fwup cond … tunnel:%d" passes a literal 0 [0x08033492].
- The case still accepts a bud-initiated DS connect (B2P 0x10 handler
  0x0801bf78 → DS-3 callback 0x08011989); 9A348 and 9B47 buds never send
  one (below). "ft tunnel" is a factory CLI test session (0x0804698c).
- USB routing: UART = CLI, USB = Disabled (0x0803c570); CDC only after
  `crabapple` (+0xf, 0x08043fb0).

## The bud's side of the tunnel (rkof 9A348; Known unless marked)

9B47 is identical here (addresses shifted, mostly −0x20).

- Transport 0x03 = the case's DS 3; 0x01/0x02 = DS 1/2 (Inferred from the
  case's sids and `rkof`'s fixed roles: 0x01 controller pushing FTAB, 0x02
  accessory, 0x03 downstream). Transport state comes only from the case's
  B2P 0x10 [0x15bdd0] → 846d4(id, state); transports 0x1a/0x1b (registered
  0x1541c6) likewise or from a byte-stream adapter, never from a host.
- The open: B2P 0x10 → 846d4(3,1) → be4ec case 3 → bee88 (a host UARP
  session of type 0xd (AACP 0x4F) or 4 (GATT-AAP) [rkof 0xbee88]) →
  addDownstreamAccessory 0x77ef0 (id 1, sender 0xbe929, MTU 0xf2
  [0xbe4fa-0xbe526, 0xbe6de-0xbe70a]) → 76e0c (host endpoint) → 106f40
  (UARP 0x17 to the host). The reply to the case is B2P 0x11 with the
  resulting state [0x15be28]; 0 if the add failed (846d4 resets it
  [0x84794-0x84798]); status 5 if the transport isn't enabled
  [0x15c6ba-0x15c6c2]. No offer state is needed.
- Refusals: transport 3 not enabled (always enabled at boot [0x1541fa]),
  already up (846d4 returns at once, no add, reply 1), no 0xd/4 session (log,
  0x80000000 [0xbe7d6]), 2 downstreams already, slot 3 in use, no free slot,
  no 0xd endpoint, allocation. None reads offer state, role, feature flags
  or protocol version, and none is reported to a host (only the 0x11 reply
  and the bud's log ring). Downstream 2 (the other bud) goes through the
  same bee88 → 77ef0 → 76e0c → 106f40 path.
- 0x77ef0 marks the slot in use (+0x24), bound (+0x28) and announced (+0x2e)
  before any ack [0x77fcc-0x78010]; Discover Downstream (0x15, 0x78134)
  re-announces only unannounced slots [0x78150-0x7817e], so it never
  repeats a 0x17. The release callback 0x781c0 (pointer 0x154768) clears
  +0x28/+0x2e; the slot goes when transport 3 goes down [0xbe7b0-0xbe7d0].
- bea24 (creating the 0xd session) never adds downstream 1: a failed open
  stays failed until the next close.
- The bud drops transport 3 by itself after ~25 s idle (0x8395c) or on link
  loss (0xd644c) without telling the case; the case's 14 s idle timeout
  covers that.
- **The hold.** On lid close, bf2b0(0) [0xbf2d8-0xbf35c] keeps exactly one
  host and drops the others (0x9a364, reason 4), takes holds and pages it
  back (reason 5), only if bee30 (a SuperBinary record of that session in
  state 2 [0xbee60-0xbee6a]) or bef74 (a dynamic record, list +0x398, or a
  SuperBinary record in state 1 or 6 [0xbefdc-0xbf024]) finds a 0xd/4
  session. Otherwise every host drops (Observed ~0.18 s after the close,
  Oct 27). So a host survives a close only with an accepted (2), deferred
  (1) or state-6 offer, and a 0xd host's SuperBinary is accepted only if
  newer than running (bd938, else deny 1). Which host dynamic assets reach
  state 1/6 is not established. The release (bf2b0(1), cmd 0xf, or 0xd ≠ 6)
  has no poster found.

## Status bytes the bud reports

To the case (built fresh per request by FUN_001586cc via 0x158d6c [rkof
9A348]; the case reads its cached copy `[0x2002658c + tid·0x40 +
0x15/0x16]`):
- **S0 bit 1**: the stored pair role, 0xc61a4 = 0xc6449 on object
  0x200b8778: cfg+0x40 in a CRC'd retained struct (1 primary, 2 secondary;
  0xa5b44), not the link holder. Writers: the role machine's commit (HSM
  0x1c90f8 action 8 0xe08fe, toggling action 4's saved value), the case
  over B2P (0x15a284), boot (0x152f48 → secondary when a peer is stored and
  r10, unresolved; 0xc2470/0xc2486 → primary; kept unless out of box,
  0x8dc18, then by the factory SIDE cfg+0x41), 0xd69ee. The case's role
  command (B2P 0x5a 'P'/'S' [9A348 STFC 0x08021ddc] → bud 0x15a278,
  0xc61d4) changes nothing while exactly one bud claims; none → Right
  primary; both → the one with fewer host ACLs (tie: Right).
- **S0 bit 6**: bt_prog (above). S0 bit 3 is read by 'AP' (below).
- **S1 bit 0**: `AP.isInState(4)` (S4 and its children S5/S6) [0x1589ba,
  vtable +0x18 0xb63f6], built at 0x1589ac-0x158a00. 'AP' (vtable 0x1c4334
  at 0x161cbc, HSM 0x1c8124, actions 0xde930): S1 holds paging and the 'BS'
  sleep arbiter, S2 drops every host (reason 2), S3 releases, S4 arms
  paging; S4 → S5 → S1 when the radio is no longer wanted (in the case:
  0x15f60c, timer 3 via 8cd04(3,10,…), when the bud's location changes,
  unless S0 bit 3 and host ACLs). Back to S4 only through S2, so **a held
  host link means the bit is set**. The case's docked reset reads the same
  bit [9A348 STFC 0x0801ec5c-0x0801ed16].
- **S1 bits 1-2**: coup_st, from object 0x200bb6e0 (HSM 0x1c8cbc, the bud's
  side of the case's Couple procedure); 0 in steady state.

To hosts:
- AACP `PrimaryBud` is `[side] 00 [role machine state in 0x14] [active]`
  [rkof 9A348 0x873b0-0x873ce]; the third byte read 0 with the secondary
  gone, 1 with it back (Observed).
- The proximity advertisement's status byte [rkof 9A348 FUN_0014fe68]:
  0x40 this bud is stored secondary (0xc61a4 false), 0x20 this bud is Left
  (SIDE 2), 0x80 out of box, 0x01 = 0xc6214, 0x02/0x04 and 0x08/0x10 the
  stored primary's and secondary's locations. Observed: 0x35 (Left primary)
  + 0x55 on the Nov 13 22:18 and 23:05 runs, link on Left; 0x15 (Right
  primary) + 0x75 on the Nov 14 00:33-00:37 run, link on Right.

## The case's BLE chip (NERF)

- nRF image (MBR/SoftDevice; 7B21's identical to 9B47's). It has the buds'
  UARP service and characteristic: 4715650b-5e9d-4ac2-b898-a4fc0aa5df78
  (service), 94110001-6d9b-4225-a4f1-6a4a7f01b0de (characteristic), then
  2ee26cd4-c04e-41ce-905a-b4054f5d1770 / 71062dc4-413a-41ea-af86-8cecfa21d057
  (Inferred ACS), then DIS, TPS, Find My (FMNS), POSH, AIS.
- **The UARP and 2ee26cd4 services exist only while the case is Find My
  paired** (Known): added by FUN_0003bda8 [9B47 NERF] / FUN_00044f04 [7B21
  NERF] only when paired, at boot (9B47 0x3c050, 7B21 0x4551a) and at
  disconnect (9B47 0x3a56c, 7B21 0x42e14). An unpaired case has no UARP
  characteristic.
- Characteristic: write and CCCD access Just Works, i.e. an encrypted link
  (9B47 0x3bda8: `0x0202`); write + indicate, max 0xf4 (the Oct 27 read
  below). A paired case refuses every new pairing
  ("Already paired. Reject request."; 9B47 FUN_0003bbdc event 5). Security
  params {bond, no MITM, no LESC, NoInputNoOutput}: legacy Just Works (7B21
  `31 07 10 03 03` at 0x44e16). An unowned case accepts such a bond but
  deletes it and resets at disconnect (9B47 0x380f4, 7B21 0x3ffec). Sends
  are gated on paired and encrypted [9B47 0x3cfdc]. UARP binds to the first
  connection that writes ("Uarp cp write: invalid Conn handle:%u", error
  0x11) and is forwarded to the MCU over B2P (Inferred) (Oct 27 read).
- Pairing state byte (9B47 RAM 0x20008ba0, getter FUN_0003f320; 7B21 RAM
  0x20008a08, FUN_000486fc): bit 0 "BA paired", bit 1 FMN enabled, bit 2
  loaded. Set only by the loader FUN_0003f2b4 (a stored 0xd0-byte record)
  and FUN_0003f4e8 at Find My pairing completion; the record is written
  only by PAIRING_COMPLETE FUN_000381f4 after the server-backed
  INITIATE_PAIRING steps (S1-S4 and BAA certificates), so it needs Apple's
  servers. A plain LE bond doesn't set it (PAIR → PAIR_BONDED only). The MCU
  can only query it (B2P 0x4de); the buds' sync only clears it. (9B47
  addresses.)
- Cleared by findMySmUnpair FUN_00038044, then a reset: B2P 0x154
  FACTORY_RESET from the MCU (factory reset, below), bud reconciliation
  (0x38678), a failed Find My pairing, a failed boot storage check, or the
  owner's "RX Unpair" (0x3cc54, encrypted owner link only). (9B47.)
- Advertising: unpaired, each lid opening (7B21 LID_OPENED 0x3f504) starts
  a connectable Find My pairing advertisement (service data 0xFD44, 21
  bytes, public address, 30 ms) for 10 minutes (9B47 FUN_0003990c, 7B21
  FUN_00041fc0). Paired: rotating static random address, Apple type 0x12
  (near owner, length 2, FUN_00039af0; separated FUN_00039a24, Inferred long
  form) (9B47). Observed Nov 13: lid open, nothing near the laptop
  advertised 0xFD44, so our case is paired.
- Unresolved: an Oct 27 read (labelled 9A348, NERF 90.264) gave other
  addresses: UUID table 0x51d1e, registration FUN_0003bd54 (permissions at
  0x3be10-0x3be5c, map FUN_0002b48c), write-authorize FUN_0003b580 →
  FUN_0003dccc (binding), peer manager FUN_0003bb88 event 5, pairing bit
  FUN_0003f2cc. They don't match 9B47's, though firmware.md calls 9B47's
  non-`rkof` changes version stamps; which build they belong to is open.

## USB (Observed Nov 13; code 9A348 STFC unless marked)

- With a data cable the case enumerates at once (lid and buds irrelevant;
  any Rp level at CC, 0x0803b8c4): `05ac:139e` "MagSafe Charging Case
  (USB-C)", bcdDevice 47.08 (= 71.8, above), full speed, 500 mA, one HID
  interface, interrupt endpoints 0x81 in, 0x01 out. Report descriptor
  `06 00 ff 09 01 a1 01 09 01 85 01 15 00 26 ff 00 75 08 95 40 82 22 01 c0`
  (vendor page 0xFF00, report 1, 64 bytes), then a Consumer Control
  collection (ID 3: next, previous, play/pause). Linux: hidraw, root only
  without a udev rule. Nothing was sent to it.
- The HID interface is inert: OUT reports go to a `return 0` handler
  (0x080352f7 via 0x0800cd88), SET_REPORT data is ignored, nothing goes IN.
- A CDC-ACM port (CLI or B2P/FWUP pipe, "usbTxService") is added only after
  the CLI's `crabapple` unlock (0x08042e34, string 0x0806da3c). The CLI's
  `usb side:… eusb:… durant:<app|dfu> reset:…` (0x08041b28) routes the data
  lines to a bud and can put it in DFU; "Wired bud" (0x08037580) opens a
  FWUP pipe to a bud. Inferred: factory paths. So over USB a host can't
  update the case or reach the buds without the case's CLI.

## The CLIs aren't reachable from a host (Known unless marked)

- Bud (rkof 9A348): the RTKit shell (read loop 0x1d628) reads only its
  console, two UARTs added at boot (0x147a6, 0x147b6; only by 0x19c7e4); on
  a production unit (0x18f2c: `is_prod_override` or chipbase+0x607c)
  0x7f62c drops one. Other users only register commands.
- Case (9A348 STFC): CLI routing (0x0803dde4: 0 UART, 1 USB, 2 TUNNEL) goes
  to TUNNEL only through the CLI's own `routing` command (0x08048efc); the
  six other callers pick USB. B2P sessions 0x1d/0x1e are "b2b auth"
  (Couple), not a CLI.
- Inferred: no AACP, UARP or GATT path reaches either CLI.

## No host-to-case relay through the bud (rkof 9A348, 7B21 STFC; Known)

- The bud's B2P sends to the case are replies (0x11, 0x13, 0x23), a DS
  close (0xbe4d2, 0x10 {id,0}), DS data on an open DS (0x12, the UARP
  downstream relay, 0xbe9ae and 0x84968), fragmented internal messages
  (0x184/0xe184 from fixed callers, 0x83a38) and fixed internal sends
  (0x46c, 0x1d0, 0x1d4, 0x0, 0x16, 0x1c). The receive handler forwards node
  3/5 messages between B2P links (0x158e9a).
- No AACP opcode (callback table filled at 0xcb170) or ATT write ends in a
  B2P send with host-chosen bytes. Only the case's 0x10 brings transport 3
  up (846d4's other callers 839c4, be4da, d647a, 15c81c pass state 0;
  15be08 is the case's).
- LibrePods' case-sound command `12 3A …` is an ATT write to the Find My
  service (handle 0x3A, `4F860005`, handler 0x68fa4), a fixed setting.
- So no non-Apple host makes a 7B21 case tunnel: it's updated only by its
  Find My owner's Apple device over BLE.

## Resets and coupling (9A348 STFC; Known unless marked)

- Reboots: IWDG (5 s kicks [0x080353dc]); SYSRESETREQ 0x0802f770 (callers
  not reachable from a host); none on USB attach or wireless charging.
  Running flat: Inferred a power-on reset (coupling and Find My, in flash,
  survive).
- Button, lid open: Press, Hold 0.48 s, Pairing Hold 1.98 s, "Case Hw Reset"
  ~10 s, factory reset ~13.5 s (stored, finished at the next boot,
  FUN_08019698).
- Factory reset: "Clr coupling" (event 0x900e) wipes coupling and sends B2P
  0x154 to NERF (Find My unpair) [0x0801b5ac, 0x0801bcaa]; re-coupling is
  automatic (B2P 0x4ae). A foreign bud or invalid NV data triggers "auto
  fact rst in 2s" [0x08022b00].
- Coupling compares bud with bud only (FUN_0801d6cc, "Couple mismatch fw
  ver"; [sibling-and-case.md](sibling-and-case.md)): a mismatch starts the
  bud-to-bud copy. "Opportunistic Apply" [0x08003728] compares the case's
  own versions. No case-vs-bud check.

## The probes (Nov 13-14) and what they ruled out

`calvados firmware try-probe-case` (AGENTS.md) holds the pods across a lid
close with an offer (at most 10 % of `FTAB` served, never staged, withdrawn
after 3 minutes) and asks the case's version through downstream 1 if it
appears. All runs: buds 9A348, laptop, AACP 0x4F. The case never appeared,
explained by its 7B21 firmware (tunnel off). Observed:

- The hold works: every close dropped the phone and kept the laptop
  (advertisement byte 10: 2 hosts, then 1).
- 16:49-16:52 (phone connected, case on a wireless charger, lid moved
  often): four closes (40 s, 77 s, brief, 26 s); only downstream 2 announced,
  ~0.05 s to a few s after each close. The secondary reported
  `Disconnected` for a few seconds around each close, then came back as
  downstream 2.
- 17:29-17:32 (phone off): four closes, one during the transfer (2-4 %; it
  went on with the lid closed), three held at 10 %. `PrimaryBud` and the
  secondary `Disconnected` 4.1 s after each close for as long as it stayed
  closed, back 2-3 s after opening.
- 22:18-22:20 (0x15 every 3 s): every 0x15 got 0x16 status 0 within ~0.1 s,
  no 0x17 for id 1. The held link survived the first close (26 s); 31 s
  into the second the pods ended it (0x13) and rebooted (2m45s up at
  22:23:30), announcing no crash log.
- 23:05 (no 0x15 polling, case charging): held from 23:06:24, closed
  ~23:06:27 for 2m20s, link held, secondary gone at +4 s, no downstream 1.
  Rescinded at the 3-minute limit (23:08:52). Second close ~23:08:55: the
  primary ended the link (0x13) 31 s later and booted (27 s up at 23:09:54).
  So the 31 s release on a second close isn't the polling's doing.
- 23:44 (Right primary, Left docked first): two closes, the second within
  ~5 s of opening; link held, no 31 s release, no 0x17 for id 1 in the raw
  frames.
- Nov 14 update (9A348 → 9B47, [uarp.md](uarp.md)): with the image staged
  the primary held the laptop's link 30 s after the close, then let go
  (0x13).

Ruled out (don't retry):
- The phone's drop or the secondary in flux at the case's 2-3 s checks:
  same with the phone off; the secondary leaves only at +4 s.
- An active transfer instead of a held one: no difference.
- S1 bit 0 ("radio"): a held link means it's set.
- The role pick: the advertised status byte shows the stored role on the
  link holder in every run; Right as primary changed nothing.
- A stale "open" DS descriptor: RAM only, closed by any non-1 response and
  a 14 s idle timeout.
- The case's per-bud send state: the primary is charging or not charging
  at +2..3 s, both send.
- The bud refusing silently: downstream 2 takes the same path on every
  close and the held session proves bee88's session exists.
- "No slot, since 0x15 got no 0x17": void, 0x15 never repeats a 0x17.
- UARP protocol version: matters only for an offer to the case (needs 6),
  not for the tunnel opening.

## What a host can and can't do

- Can't: update or query a 7B21 case (ours). Its UARP is BLE-only and
  exists only for its Find My owner; no bud, AACP, ATT, CLI or USB path
  reaches it.
- On a 9A348 case (Inferred, never seen): hold an accepted or deferred offer
  (newer than the buds run) over AACP 0x4F with both buds in the case,
  coupled, lid opened once since the case booted; close the lid; ~2 s later
  expect 0x17 for id 1; then 0x1b to id 1 with Sync, Version Discovery at
  protocol 6 and Info tag 5, lid kept closed. Risks: the hold is a real
  staging the case may copy to the other bud and apply; a 0x4F to a
  non-primary aborts [rkof 0xbea70]; the hold drops every other host and
  keeps the buds out of ULP. Not established: a hold without a newer image,
  the exact tag-5 value, whether the bud accepts a page in the closed case
  before 2 s.
- A complete set update like Apple's (Inferred): FTAB to the primary (the
  case copies and applies on the buds, [sibling-and-case.md](sibling-and-case.md)),
  plus a separate offer of the same SuperBinary to the case (downstream 1
  or its own BLE UARP), R1FW with a live TSS ticket.
- A buds-only update leaves the case as it is; no check requires matching
  versions (and ours works mismatched). What new buds expect of an old
  case's B2P is not visible.

## Case B2P messages never to send

The fwup factory reset / Clr coupling (0x900e → 0x154), 0x184 op 0x82
property sets (class 3 writes NV), 0x352 memory write, 0x32a state reset,
0x4b6 (NV key 0xd), 0x4b4 tones, and any 0x10/0x12 DS traffic outside a
tunnel.
