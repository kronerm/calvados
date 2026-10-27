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
