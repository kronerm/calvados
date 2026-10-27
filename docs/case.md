# How the case's firmware gets updated

AirPods Pro 2 (USB-C), build 9A348: the buds' `rkof`, the case MCU's
`STFC`/`STFW` and its BLE chip's `NERF` (hashes, tools: [firmware.md](firmware.md);
the buds and the case's sync: [sibling-and-case.md](sibling-and-case.md)).
Static, cited `[image 0xADDR]`; Known = read from code or data, Inferred =
marked. Checked strings: "mismatch fw ver", "Couple", "Deny %s(%u) offer"
(STFC), "Already paired. Reject request.", "Added App UARP Service" (NERF).

## Q1. Does a bud store case payloads for the case? (rkof)

Known:
- The stored-file table [rkof RAM 0x20002ca8, init file 0x1f2ca8: STFW->"ioe1", IOHM->"iohm",
  IOHS->"iohs", ESM1->"esm1", PDFW->"pdfw"] is referenced by exactly one literal [rkof 0xbfa4c],
  in FUN_000bf950, which only OPENS the file read-only (0x1b4272 -> 0x19af4), reads the 0x2c-byte
  SuperBinary header, caches its version and offers it as controller (0x77adc). No pointer to
  the file-name strings "ioe1".."pdfw" exists anywhere else (word scan of rkof for 0x1e2be5..,
  and for any RAM address 0x20002c80..0x20002d40: only 0xbfa4c).
- File "case" [str rkof 0x1e2b62] is referenced at: 0x7a380 (loop cmd 0x12: open read-only and
  offer as tag CASE), 0x151942 (open read-only, take its size into a status record), 0x15862
  (the CLI command group "case" / "Case Commands", not a file). No writer.
- Asset tags CASE, IOE1, IOHM, IOHS, ESM1, PDFW appear only in FUN_000bccd0 [rkof 0xbcec8-0xbcf0c],
  the asset-release handler: they are just recognised and go to the common release path
  (0xbcdda). The accessory-side payload handling (FUN_000bc978 [0xbc9be-0xbc9d2], FUN_000bd0ac
  [0xbd0b8-0xbd158]) knows only FTAB / LTAB / RTAB / HSML (+ outbound dynamic assets MTIC/LOGS/CRSH,
  VERS). STFC/STFW/NERF/R1FW never occur as 4cc literals or tag strings in rkof (word scan for
  'STFC','NERF','R1FW' = none; 'STFW' only in the table at 0x1f2ca8).
- The bver parser FUN_000a5770 understands fields "csv1:", "ioe1:", "csv2:", "ioe2:", "updv:"
  [strs rkof 0x1e2730..0x1e2748, lits 0xa5a50/0xa5a54/0xa5a58], but this build's FTAB bver is
  `90.3431000025000000.6807|date:2026 8 17|updv:348|chip_revision_b0` (no csv/ioe fields).
Inferred:
- The store-and-forward machinery (files "case", "ioe1", ... offered on transports 0x1a/0x1b,
  0x2d-0x30) is shared rkos platform code kept for other products (IOHM/IOHS/ESM1/PDFW are not
  parts of this set; "ioe" likely an older case/IO-expander scheme). In this firmware nothing
  writes those files, so the bud never holds case firmware to forward: the offers bail out at
  the file open (loop cmd 0x12 log 0x15c4ad03 [0x7a38a]; bf950 log DAT_000bfa40+4).
- Consistent with the live trace: the bud pulls only payload headers up to FTAB, FTAB's
  metadata and FTAB's data; nothing after FTAB.

## Who consumes which payload (Known: tag strings in the images)
- Bud (rkof): only FTAB (+HSML) is staged [rkof 0xbcfa8]; every other payload goes through
  0x76860 -> FUN_00076bd4, which just bumps the payload index and requests the next payload
  header (0x106ac0) or finishes the asset (0x103e32) [rkof 0x76bd4]: headers pulled, data never.
- Case MCU (STFC): tag list at [STFC 0x08071f5a..0x08071f9b]: R1FW, IM4M, NERF, STFC, AUFM, AULB,
  AUPS, AUPC, AUCH, AUER, AUUT, AUFR, AUUF (file offset 0x7075a). STFW (other case hardware
  variant) has the same list with STFW. Lower-case audio file names aufm..auuf at
  [STFC 0x080719ac] -> AU* are the case speaker's sounds, stored by the case.
- CMAP and TMAP are NSKeyedArchiver bplists (keys "sections", "AppleModelNumber", "decoderId"):
  Inferred host-side decoder maps (logs/crash symbolication). CTAB = compressed FTAB (16 KiB
  chunk table 0xCCD28113, decompressed size 0xCCD28119 = FTAB size, same SHA-256 as FTAB).
  None of CMAP/TMAP/CTAB occurs (string or 4cc word) in rkof, rkos, STFC, STFW or NERF.
  (Inferred: CTAB is for a host/transport that sends compressed; this bud doesn't take it.)
- NERF version 90.264; STFC/STFW 90.390; R1FW and FTAB 9.0.348 (bver). Case payloads carry
  0xCCD28107 SHA-256 + 0xCCD28106 256-B signature + 0xCCD28105 = 2; STFC checks those
  (TLV ids as words at STFC file 0x13910-0x13918; "Attempting Dev Sig Check"/"Sig Check fail!"
  [STFC 0x08013784]).

## The case's BLE chip (NERF) exposes the SAME UARP GATT service as the buds (Known)
NERF is an nRF image based at 0 (MBR/SoftDevice vectors). Its vendor UUID table [NERF 0x51d1e]:
- 0x51d1e 4715650b-5e9d-4ac2-b898-a4fc0aa5df78 = UARP service (same as the buds, firmware.md)
- 0x51d2e 94110001-6d9b-4225-a4f1-6a4a7f01b0de = UARP characteristic (same as the buds)
- 0x51d3e 2ee26cd4-c04e-41ce-905a-b4054f5d1770, 0x51d4e 71062dc4-413a-41ea-af86-8cecfa21d057 (next
  service/char, literals at NERF 0x3bfa8/0x3bfb4; Inferred ACS) ...
Strings: "VS UUID UARP Service addition index", "Added App UARP Service", "Added App UARP Chars",
"UARP Service added", then ACS, DIS, TPS, FindMy (FMNS), POSH, AIS services; "Uarp cp write:
invalid Conn handle:%u", "uarp, rx:l=%u".

### NERF UARP characteristic: permissions and gatekeeping (Known)
- Registration FUN_0003bd54 [NERF 0x3bd54]: service 0x650b (UUID base at 0x51d1e), then the char
  through Nordic's characteristic_add (FUN_0002b4be, ends in svc 0xaa = sd_ble_gatts_characteristic_add)
  with ble_add_char_params_t: uuid 0x0001 (94110001-...), max_len 0xf4, var len, props 0x28 =
  write + indicate, deferred write = 1, read_access 0 (none), **write_access 2 and cccd_write 2
  = SEC_JUST_WORKS -> sm 1 lv 2 (encrypted link required)** [NERF 0x3be10-0x3be5c; the
  security_req -> (sm,lv) map FUN_0002b48c: 1->0x11, 2->0x21, 3->0x31, 4->0x12]. The next
  service (2ee26cd4-..., uuid16 0x6cd4) and its char 71062dc4-... are set up the same way.
- Writes arrive as write-authorize requests, FUN_0003b580 [NERF 0x3b580] ("Auth write handle"):
  the UARP char's value handle (+0x92) goes to FUN_0003dccc, which binds UARP to the first
  connection that writes ("Uarp cp write: invalid Conn handle" for any other, error 0x11) and
  queues the bytes (0x41384; Inferred: forwarded to the case MCU over the internal B2P link);
  the auth reply is "accept" (0x3b10c(1,..)). No Find My ownership check on this path; the
  encryption requirement is the gate.
- Pairing: the peer-manager handler FUN_0003bb88, event 5 (CONN_SEC_PARAMS_REQ) [NERF 0x3bc1c..]:
  if FUN_0003f2cc() (bit 0 of the pairing-state byte) is set, logs "Already paired. Reject
  request." [str 0x51a48] and replies with NULL params (pairing refused). The other control
  point (+0x16 handle; Inferred Find My pairing control point) likewise says "Already paired -
  refusing" [0x3b5e0, str 0x51792]. Advertising is Find My driven: "BTFinding - ADV Connectable",
  "... NON Connectable!", "advManagerNearOwnerAdvStart", stats case_only_adv / case_l_adv /
  case_r_adv.
Inferred: a host can only write the case's UARP characteristic over an encrypted link, i.e.
as a peer bonded with the case. A case set up with Find My (paired to the owner's iPhone) refuses
any new pairing, so a non-Apple host cannot reach the case's UARP over BLE; even an un-paired
case would have to be advertising connectably (Find My modes) and accept a fresh pairing (the
default security params were not traced).

## Q2. How the case receives its own firmware (STFC)

### The case's UARP accessory and its hosts (Known code, tid meaning partly Inferred)
- The case's UARP data sessions ("DS") are B2P data sessions keyed by **tid 0..3** (B2P = the
  case<->bud/peripheral protocol, "locust"; both rkof and STFC have it). Per-tid UARP buffers
  0x20023730 + tid*0xf4 (MTU 0xf4) [STFC 0x08011f32-0x08011f48], rx callback 0x08012129 ->
  event 0x28057 only for the current UARP host [0x200015d4+0x3c == tid] [STFC 0x0801214c].
- On event 0x2805e the case opens **sid 3** on tid [msg+0xc] (0x0802a2ac(tid, 3)) [STFC
  0x08011fc6-0x08011fd4] = its UARP session over that link. The B2B transfer opens sid 1
  ("Bud server") and sid 2 ("Bud client") on tid = the source/target bud [STFC 0x08037878 ->
  0x080379dc(dir,1|2), bounds tid <= 3, "Open tid:%d sid:%d"].
- Offer handler FUN_08015138 (delegate assetOffered, via 0x08058a5c) [STFC 0x08015138]:
  asset type [+0x3c] 2 (dynamic) -> deny unless case-OTA state; type 1 (SuperBinary): if apply
  state [0x20034fc8+0x35] & 0xfb (busy) -> "%s state:%s host:%s in assetOffered", deny.
  Then by host = tid [0x200015d4+0x40]:
  - tid 0/1: accepted only if [[0x20035244+0x70]+0xc] == 1 (and if +0xd == 1 only when
    [0x200350c8+0x46] > 5), else **"Deny tunnel(%u) offer"** [0x08015204-0x08015228];
  - tid 2: if [[..+0x70]+0xd] == 0 **"Deny BLE(2) offer"** [0x0801522e, 0x080152a8-0x080152b4];
  - tid 3: no version check, event 0x1c00e, returns the third result [0x08015294];
  - otherwise compares the offered version with the case's active and pending versions
    (FUN_08036e68, "A:%d.%d.%d.%d O:%d.%d.%d.%d"): accepted only if newer than active and not
    already pending [0x08015244-0x08015290].
  Inferred from the deny labels and the B2B use of tid: tid 0/1 = the two buds (the case's UARP
  tunnelled through a bud: "tunnel"), tid 2 = the case's BLE chip (NERF's GATT UARP forwarded over
  the internal B2P link), tid 3 = a wired path. (The RAM name table 0x20035a58 holding the tid
  names is filled at run time; names not recovered.)
- Accepted payload tags (case MCU): R1FW, IM4M, NERF, STFC (STFW on the other variant), AUFM,
  AULB, AUPS, AUPC, AUCH, AUER, AUUT, AUFR, AUUF [STFC file 0x7075a-0x70796]. The case stages
  them itself ("ST pend", "NRF pend", "R1 pend", "Wr to partition %d", "Attempting Dev Sig Check"
  [STFC 0x08013784] against the payloads' 0xCCD28106 signatures), and applies: "Apply ST",
  "Apply BLE" / "No pend BLE img to apply" / "nRF apply fail" (the MCU flashes the nRF: "nrfSWD",
  "!! Upd nRF !! Req: %d.%d.%d", "Min BLE %s not set" [STFC 0x08036c22-0x08036c3e]),
  "Apply R1 image, pend bank %d" (UWB, A/B banks; R1FW needs its TSS ticket, "Add IM4M to hdr").
- Case routing (CLI "routing", "Transport -> Type") [STFC 0x08048e8c]: transports UART, USB,
  TUNNEL [0x20035884+0x60/0x64/0x68], types Disabled/CLI/FWUP/B2P/CLI REMOTE/Internal_CLI
  [names 0x0806e24c]. Defaults [STFC 0x0803dd18]: **UART = CLI, USB = Disabled, TUNNEL =
  Disabled**. Code switches USB to FWUP for "Wired bud, tid:%s" [0x08037598-0x080375b2: routing
  (2,USB) then open sid 2 on that tid], to B2P on pass-through off [0x08035ede], to CLI REMOTE on
  pass-through on [0x08043018]. Inferred: a wired (USB) host path exists for factory/Apple
  tooling and wired bud updates; it's off by default.

### The bud side of the tunnel (Known code; meaning Inferred, consistent)
- rkof transport 0x03 up -> if this bud has an Apple-host UARP session (FUN_000bee88 returns
  only session types 0xd or 4 [rkof 0xbee88]) -> UARPL3Downstream_addDownstreamAccessory(1, ...)
  with the generic transport sender 0xbe929, MTU 0xf2 [rkof 0xbe4fa-0xbe526, 0xbe6de-0xbe70a].
  So the case appears to an AAP (transport 0xd) UARP host as **downstream accessory 1**, the
  sibling as downstream 2. With only a GATT (0x10) host session nothing is added (as observed:
  Discover Downstream empty).
- Transport ids on the bud are set from case messages (0x15bdd0 -> 0x846d4(id, state)); the case
  opens sid 1/2/3 => Inferred: bud transport 0x01/0x02/0x03 = B2P sid 1 (bud server)/2 (bud
  client)/3 (case UARP). This matches rkof's fixed roles (0x01 controller pushes FTAB, 0x02
  accessory, 0x03 downstream).

## Q3. Reaching the case as a host
- Over BLE: the case's nRF exposes the same UARP service/char UUIDs as the buds (above), but
  writes need an encrypted link (SEC_JUST_WORKS) and the case refuses new pairing once it's
  "paired" (Find My) [NERF 0x3bb88 ev 5]. Known code; Inferred: only the owner's Apple device
  (already bonded via Find My) can use it. Not established: whether/when it advertises
  connectably for a bonded owner outside Find My modes, the default SMP params of an unpaired case.
- Through a bud: only as downstream accessory 1, announced only to an AAP (0xd/4) UARP host
  session [rkof 0xbee88, 0xbe6de-0xbe70a]; then relayed with UARP 0x1b (Downstream Relay) ->
  bud transport 0x03 -> case tid 0/1 sid 3. A GATT (0x10) host gets no downstream (Observed).
  Inferred: an AAP-UARP session is what an Apple host opens; whether a non-Apple host can open
  one (it rides on AAP, which Calvados already speaks) was not traced (creator 0xbea24 is called
  from 0x9ca4c in FUN_0009c6dc, reached from 0xcd420 via a pointer at 0xcb43c; not decoded).
- Wired: case USB routing defaults to Disabled; FWUP on USB is switched on only by the
  "Wired bud" path or CLI. Not a general host path as far as seen.

## Q4. Version coupling between case and buds
Known:
- Bud vs bud: the case's "Couple" check FUN_0801d6cc [STFC 0x0801d6cc] requires both buds
  present, a 3-byte field equal in both records (0x2002658c+0x12..0x14 vs +0x52..0x54,
  FUN_0801e19c; meaning not established) and equal ACTIVE versions; otherwise logs
  "Couple mismatch fw ver" (or "no comms") and posts 0xc001, which runs the B2B decision
  FUN_0803542c (R2L/L2R/APPLY_*). Cross/pending matches set force-apply (+0x5d).
- ERR_MANI (FUN_0803542c result 10) is about the BUDS' personalized manifest in B2B
  ("B2B: Mani invalid:%d, check_side_ok:%d, check_pending:%d" [STFC 0x080372c8]; gate flag
  [0x20001748+0x57]), not about the case.
- Case vs nRF: the MCU requires a minimum BLE (nRF) version and flashes the nRF from its staged
  NERF ("!! Upd nRF !! Req", "Min BLE %s not set", "BLE exp:%d.%d.%d rcvd" [0x08016d3a]).
- Case vs buds: no check found in STFC that compares the case's own version with the buds',
  and none in rkof (its bver parser knows "csv1:/ioe1:" compatibility fields, but this build's
  bver has none). The case's own offer check compares only against its own active/pending
  [STFC 0x08015244-0x08015290]; "Opportunistic Apply" [0x08003728] compares its own versions.
- Bud updates via the case are blocked while a case OTA is in progress when tunnelled
  ("bud fwup cond ... tunnel:%d, caseOTA:%d", 0x08035838).

## Q5. Bottom line (Known/Inferred as marked)
- One SuperBinary, three consumers: buds take FTAB (+HSML); the case MCU takes STFC|STFW, NERF,
  R1FW (+IM4M), AU*; CMAP/TMAP are host-side decoder maps; CTAB (compressed FTAB) is consumed
  by none of these images. (Known from tag tables.)
- The buds never store or forward case payloads in this build: their offer filter skips every
  payload but FTAB/HSML (header only), and the store-and-forward files (case, ioe1, ...) have
  no writer. (Known; a generic factory/CLI file write was not excluded.)
- The case gets its firmware by being a UARP accessory itself: over its BLE chip (encrypted,
  bonded owner only) or tunnelled through a bud as downstream accessory 1 of an AAP UARP host
  (Inferred: the iPhone/Mac path), or wired. A GATT-only host like ours reaches neither.
- Complete set update from a host: FTAB to a bud (the case then syncs the other bud via B2B
  and applies, with both buds in the case under its gates), PLUS a separate offer of the same
  SuperBinary to the case (as downstream 1 over AAP, or over the case's own BLE UARP when
  bonded), including R1FW with a live TSS ticket ("Rap"). (Inferred)
- Buds-only update (FTAB via the primary over GATT): both buds end on the new build once the
  case runs B2B + apply; the case stays on its old STFC/NERF/R1FW/audio (90.390 / 90.264 /
  older). No code requiring case and bud versions to match was found, so the set should work
  mismatched; what new bud firmware expects from an old case (B2P protocol) is not visible.


## The case's tunnel to a host (how it becomes downstream 1)

Static (STFC, rkof 9A348), cited as above. Summary: the case opens its UARP
tunnel to the primary bud by itself about 2 s after the lid closes, never
with the lid open, and only if both buds are present and coupled, the lid
has moved at least twice since the case booted and the primary's radio is
active; nothing a host sends triggers it. The bud announces the case as
downstream 1 only if a host's AACP 0x4F (or GATT) UARP session exists at
that moment, and the pods drop every host within ~0.2 s of the lid closing
unless that host's session holds an accepted or deferred offer. So reading
the case's version, or updating it, needs an offer in progress when the lid
closes: in practice, during an update of the buds.

Images: STFC (case MCU, base 0x08001800) and rkof (bud). Known = read from code;
Inferred = meaning guessed. Nothing was run against devices.

## Answer in one paragraph

The case opens its UARP tunnel on its own, **about 2 s after the lid closes**, never
while the lid is open, and never because a bud or host asks. The bud has no message that
asks for it: it only answers the case's open request. When the request comes, the bud adds
the case as downstream 1 only if an AACP-UARP (0xd) or GATT-AAP (4) session exists on
that bud at that moment. A closed lid normally drops every host within ~0.2 s (observed
Oct 27), so the session is gone 2 s later. The bud keeps exactly one host across a lid
close: the one whose 0xd/4 session has an offer record in state 1, 2 or 6 (rkof bf2b0).
Today's probe ran with the lid open, so no tunnel existed. That fits.

## Q1. STFC: what raises 0x2805e ("Tunnel avail") (Known)

- The only publisher of 0x2805e is [STFC 0x08012cf2-0x08012cf8]: 0x28052+0xc, arg =
  tid, then the log "Tunnel avail". Its only subscriber, [STFC 0x08011f0a] (in the UARP DS
  setup 0x08011ee8), is handler 0x08011fc4: `0x0802a2ac(tid, 3)` = "Open tid:%s ds:%d"
  -> 0x0802a22c sends B2P message 0x10 {ds 3, state 1} to that bud (0x0802a328).
  A constant-tracking scan of every publish/subscribe call site found no other 0x2805e.
- The code is case 0x1c006 of the fwup handler FUN_080123b0 (tbh table 0x0801243c:
  0x1c006 -> 0x080126da). 0x1c006 is the expiry of timer 0x20036188 (callback 0x0801237c,
  installed [0x0801223e]). Conditions at expiry [0x080126da-0x08012718]:
  1. lid closed: [0x200350c8+5] == 0 (+5 = lid GPIO, 1 = open; set by 0x0803c7c8, which
     also logs "Lid Opened/Closed"),
  2. FUN_0801d8e0: both buds present, coup_st (S1 bits 1-2) == 0 on both, coupling
     state [0x2003533c+0x4c] not 1..5, and both bud records hold each other's address
     (Inferred: a coupled pair, no coupling running),
  3. lid-event counter [0x200350c8+0x44] >= 2 (it is incremented on every lid event
     0x28030/0x28031 [0x0801258c]; Inferred: at least one open and one close since the
     case MCU booted),
  4. FUN_0801e074: tid = the bud whose S0 bit 1 (role, rkof 0xc61a4, Inferred primary)
     is set,
  5. that bud's S1 bit 0 "radio" is set ([0x2002658c + tid*0x40 + 0x16] bit 0). The
     bud side is rkof 0x1589ba: S1 bit 0 = 0xb64b4(), meaning object 0x200ba890 (HSM
     0x1c8124) is in state 4. Inferred: the bud's radio is active.
  If 2-5 fail there is one retry 1 s later (0xf4240) while [0x20036188+0x1c] is set. The
  flag is cleared after the retry and set again on lid open [0x080125b8]. The same retry
  path also runs after a successful open on tid 1 [0x08012d06-0x08012d1a].
- What starts the 2 s timer (0x1e8480; Inferred µs):
  - **0x28030 with arg 0 (lid closed)**, posted by the lid handler 0x0803c804
    [0x0803c828, 0x0803c870] -> [0x08012adc]. This needs the "No Apply" flag
    [0x20034fc8+0x37] != 1 (otherwise the case's own apply path runs, 0x08012f34 + 15 s),
    and it needs the tunnel config [[0x20035244+0x70]+0xc] == 1 [0x08012b9c-0x08012bbc].
  - 0x28013 with the lid closed and the same config [0x080129ac-0x080129cc]. It is posted
    once at boot from the boot lid state [0x08007322]. Inferred: the case booted with the
    lid closed.
  - Lid open (0x28030 arg 1) stops it [0x080125b2-0x080125ba].
- Tunnel config: the pointer at 0x20035244+0x70 = 0x20001790 (FUN_080121a8 called with
  0x20001748+0x48 [0x08008816-0x0800881c]). It lives in initialized data (copied from flash
  0x0807c8b4 to 0x20000000 by the C runtime [0x08049132-0x0804914c]). Its initial bytes
  are +0xc = 1 (tunnel, also "tunnel:%d" in "bud fwup cond" via 0x20001748+0x54),
  +0xd = 1, +0xf = 0 (the manifest-required flag 0x20001748+0x57 is **0**, which corrects
  the earlier open question), and +0x11 = 1. No code store to +0xc/+0xd was found. So
  tunnels are enabled in this build.
- Not inputs: charging, battery, case OTA pending (apart from +0x37), host type, any bud
  request, bt_prog. The B2B "Bud server/client" tunnels (ds 1/2) are a separate path
  (0x08037878, case event 0xc001; docs/sibling-and-case.md).

## Q2. rkof: does the bud ask for it? (Known: no)

- rkof has no tunnel or DS strings. Its only B2P sends of a DS type are two 0x11 replies
  [rkof 0x15be28, 0x15c6c2]. The first answers the case's 0x10: `846d4(id, state)` brings
  transport 0x03 up, then the reply carries the resulting state (0 if the up callback
  failed: 846d4 resets state to 0 [rkof 0x84794-0x84798]). The second replies status 5
  when the transport isn't enabled ([desc+0x42] == 0) [rkof 0x15c6ba].
- What the bud reports and the case reads here is only S0 role and S1 radio. No host,
  AACP, 0x4D, Discover Downstream (0x15) or Solicitation event reaches the case. 0x15
  only re-announces existing downstream slots (below).

## Q3. Ordering and re-triggering

- At tunnel up: 0xbe6de-0xbe70a calls bee88 (any 0xd/4 session, **no offer state
  needed**). With none, it logs and returns 0x80000000 [rkof 0xbe7d6], 846d4 drops 0x03
  back to down, and the bud replies state 0 (Known). The case's connect-rsp
  [STFC 0x08026616-0x08026644, 0x08026bf8] then just marks ds 3 closed. It has no
  retry of its own (Known), beyond the one 1 s timer retry above.
- bea24 (creating the 0xd session) never adds downstream 1 (its calls: role, endpoint
  add/remove, bb4e0, 9e038, sibling cmd 0x14) (Known). So a tunnel that failed stays
  failed until the next lid close + 2 s.
- If the tunnel came up while a session existed: addDownstreamAccessory 0x77ef0 needs the
  host endpoint (0x76e0c), sends 0x17 and marks the slot in use (+0x24), announced
  (+0x2e) and bound (+0x28) [rkof 0x77fcc-0x78010]. Release callback 0x781c0 (pointer
  at 0x154768) clears only +0x28/+0x2e. Discover Downstream 0x78134 re-announces every
  slot that is in use but not announced to the asking host (0x17) [rkof 0x78150-0x7817e].
  The slot is removed only when transport 0x03 goes down [0xbe7b0-0xbe7d0]. Inferred
  (when 0x781c0 runs was not traced): once downstream 1 exists, a later 0xd session can
  get it back with 0x15, as long as the case keeps ds 3 open.
- Re-trigger: close the lid again. A cycle needs the lid open, then closed for at least
  2 s. Each close is another attempt.
- The catch is the host link. On lid close, bf2b0(0) [rkof 0xbf2d8-0xbf35c] keeps exactly
  one host and drops all others (0x9a364, reason 4), takes holds and pages it back
  (reason 5). It does this only if bee30 (a SuperBinary record of that session in state
  2 [0xbee60-0xbee6a]) or bef74 (a dynamic record, list +0x398, or a SuperBinary record
  in state 1 or 6 [0xbefdc-0xbf024]) finds the 0xd/4 session. Otherwise all hosts drop
  (observed ~0.18 s after close). So a 0xd session survives a lid close only with an
  accepted (2), deferred (1) or state-6 offer from that host. For a 0xd host, a
  SuperBinary is accepted or deferred only if it is newer than running (bd938: not newer
  -> deny 1). With 9A348 running there is none. Which host-offered dynamic assets the bud
  records in state 1/6 was not established.
- With the tunnel up, bt_prog (S0 bit 6, rkof 0x15f6e4 via be420 "transport 3 up") keeps
  the case from putting that bud into ULP (STFC 0x08022482).

## Q4. After downstream 1 exists

- Host 0x1b to id 1 -> rkof 0x78280 -> the slot's send function 0xbe929 on transport 0x03
  (MTU 0xf2) -> case ds 3 on tid = primary. On the case, rx 0x08012129 posts 0x28057
  only if the case's current UARP host [0x200015d4+0x3c] == tid [0x0801214c] (Known).
- The case serves one UARP host at a time ([0x200015d4+0x40]; 4 = none), chosen by
  priority FUN_08016acc: tid 0/1 base 5, BLE tid 2 base 1, tid 3 base 9 (Known values;
  Inferred: a tunnel outranks the case's own BLE). "Init Ctrl" 0x080158bc takes a tid
  only when none is current [0x080158d8-0x080158ec].
- Tag 5: Init Ctrl builds the accessory info. It contains "AirPods Pro Case " (+ suffix)
  and 16-byte active and pending versions from FUN_08036e68 (log "A: %d.%d.%d.%d P: ...")
  [0x08015904-0x08015a3e], the same ctx+0x370/+0x380 layout the bud answers tags 5/6
  from. Inferred: tag 5 = 4 x u32 BE. The values are combined from component versions
  (0x08004aac(2/1/4) + 0x0803688e), so the exact number (90.390.x.y?) is not established.
- Gates: "Deny tunnel(%u)" / "Deny BLE" in 0x08015138 sit in the offer path only. For
  tid 0/1 they need cfg+0xc == 1 (set) and, because +0xd == 1, [0x200350c8+0x46] > 5
  (unknown field). No host-identity check was found on the info path (not searched
  exhaustively).

## Q5. Steps (Inferred) and risks

1. Both buds in the case, lid opened at least once since the case booted, buds coupled.
   Settle on the primary.
2. Open AACP, send 0x4F Sync and Version Discovery (creates 0xd), then Discover Downstream.
3. Get a hold: offer the primary a SuperBinary newer than it runs, over 0x4F, and let it be
   accepted or deferred. Without that the lid close drops us.
4. Close the lid. After ~2 s, expect 0x17 for id 1 (or send 0x15 again).
5. Send 0x1b to id 1 with Sync, Version Discovery and Info tag 5. Keep the lid closed.

Risks: step 3 means a real staging that the case may B2B-copy and apply. A 0x4F to a
non-primary aborts (rkof 0xbea70). The hold drops every other host and keeps the buds out
of ULP. Not established: a hold without a newer image, whether the bud accepts a page
in the closed case before 2 s, the case's exact tag-5 value, and ds 3's idle timeout.
