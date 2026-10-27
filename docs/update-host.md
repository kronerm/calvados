# What a host-side updater needs to know

For calvados' own updater: closing the lid, the case's version, several
hosts, resuming. AirPods Pro 2 (USB-C), build 9A348; static analysis of
`rkof`, `STFC` and `NERF`, cited `[image 0xADDR]`, Known vs Inferred
marked, with what was observed on real pods where noted. Background:
[uarp.md](uarp.md), [update.md](update.md), [sibling-and-case.md](sibling-and-case.md),
[case.md](case.md).


(work in progress; written incrementally)

## Q1 notes (raw, being written)

### Case MCU (STFC) ULP gate for buds in a closed case (Known code, names from the log format)
STFC FUN_08022314 [STFC 0x08022314-0x08022680] decides per bud whether it is "ULP Ready"
(logs "%s ULP %s: Bud: oob,obc_rdy,data_valid,chg_st_rdy,bt,ba,my_bud,sbp; Case: pwr,lid,ggSoC,
uiSoC,fwup,ttr" [str 0x080752e8, lit 0x080226b0] and "%s ULP %sReady. 30s Met: lid,fwup,bt,pwr,
dbnc_ok; in_case_2_hr,t_rem(s)" [str 0x08075395, lit 0x080226c0]):
- lid open ([0x200350c8+5] != 0) -> Not Ready [0x08022444]; needs the "30 s met" lid flag
  [0x20034fc8+0x6a]==1, debounce [+0x6b]==0, data_valid, mode != 9 [0x08022460-0x08022480].
- **bt or fwup set -> not ready** [0x08022482-0x0802248e]: bt = bit 6 of the bud records'
  status byte (0x2002658c+0x15 | +0x55) [0x08022418-0x08022426]; fwup = 0x0803774c() (bud fwup
  active) OR a case OTA aimed at this bud (0x08035838/0x08035864) [0x080223e2-0x08022404].
- Inferred: ULP = the deep sleep the case puts buds into after the lid has been closed 30 s;
  a bud that reports a BT connection or an update is kept out of it.

### Bud side: lid and dock state (Known)
- State block 0x2004f6d0: +4 dock (1 undocked, 2 docked, 3 docked_dead), +0x4c lid (0 unknown,
  1 closed, 2 open) — names from the CLI table [rkof 0x1c28bc/0x1c28cc], "case" CLI group
  {batt, incase, dock, lid} [0x1c28a8]. +0 = CLI override (set by `case lid ...`, 0x95028).
- Setter FUN_00094f88(lid) [rkof 0x94f88-0x94fe6]: stores +0x4c, on open bumps an open counter
  (+0x50) and calls 0xc1c7c(0x6d, 10000); posts notification id **0x38** (0x7f6f0). Fed from the
  case message parser [rkof 0x15887e-0x1588ac]: lid := closed(1) if a byte flag is 0... (0x1b3fb4).
  Dock setter FUN_00094f14 [0x94f14]: on change resets lid to unknown, notification **0x35**.
- Main-task notification loop [rkof 0x155386-0x1554ec, TBH 0x155520 on id-0xa]: id 0x38 ->
  [0x15686c] lid closed -> event **0x1e**, open -> **0x1d**; id 0x35 -> [0x1566a0] docked 0x1b,
  undocked 0x1c, docked_dead 0x1f; all through FUN_000af978 -> object [0x200c1e1c+0x40]
  vtable+0xc = FUN_000af79c.
- Event chain [Known]: FUN_000af79c maps events 0x1b..0x23 through table 0x1c40dc to HSM events
  (lid closed 0x1e -> 0xb, open 0x1d -> 0xc, docked 0x1b -> 1, undocked 0x1c -> 2, dead 0x1f -> 6)
  for the "case location" HSM, descriptor 0x1c8068 (object 0x200b9d58, built [rkof 0x160a06]):
  state 6 docked --0xb--> 8 (docked, lid closed) / --0xc--> 7 (lid open); actions 0xde52c post a
  location 0..7 (8 = closed lid -> loc 6, 7 = open -> loc 5, docked -> 4, dead -> 7) to two
  per-bud location HSMs (0x1c7f54) and to a listener FUN_000d4730, which only turns location into
  SIF state bits through FUN_000b7b98 (events 0xf docked, 0x10 lid open, 0x11, 0x12) ->
  0xb861c -> SIF set/clear (0xd53d4/0xd5404): bits DOCKED(10), CASE_LID_OPEN(11) of the SIF
  state list [names 0x1c4420: A2DP HFP ANC TRANSPARENCY PSE HS LIVE_LISTEN SENSOR_PKT_SERVICE
  TIPI SPLITTER DOCKED CASE_LID_OPEN IN_EAR ON_HEAD TABLE_TOP SPARE_5 COUPLED BUD_SWAP_IN_PROGRESS
  CONNECTION_IN_PROGRESS FWUP NEW_FW_BOOT ...] (Inferred: a power/performance context, logged by
  "SIF dump" [0xd4f20]). The host-disconnect decision on lid close was NOT found on this chain.

### Bud -> case status byte S0 (Known both sides)
rkof builds it at [rkof 0x158940-0x1589aa] (+0x15 of the status message): bit7 always 1,
bits4-5 = number of host ACLs ([0x2005ede8+0x130]), bit1 0xc61a4 (role), bit2 0x8dc18 (oob),
bit3 0xc6214 (utp), **bit6 = 0x15f6e4() != 0**. STFC unpacks the same byte as "S0: pair, role,
oob_mode, utp_conn, conn_cnt, bt_prog, msmat_supp" [STFC 0x0801dca6-0x0801dcd6]; the ULP gate's
"bt" is bit 6 = **bt_prog**, not the connection count. So host connections alone do not keep
the case from putting a bud into ULP; bt_prog or fwup does.

### The UARP glue reacts to lid closed — only for an Apple-host (AAP) UARP session (Known)
- 'LO' (0x4c4f) broker messages {location, bud} are published by the case-location HSM
  [rkof 0xde4b8 -> 0xaf28c]. The UARP platform subscribes [rkof 0xbfac0-0xbfae4]: for its own
  bud it posts UARP loop command 0xd with the location; the loop calls
  **FUN_000bf2b0(location != 6)** [rkof 0x7a3f4-0x7a3fe] (location 6 = docked, lid closed).
- bf2b0(0) (lid just closed) [rkof 0xbf2d8-0xbf35c]: unless already held ([0x200c16d0+0x46]),
  finds the current host UARP session with bee30/bef74 — these search only the AAP session
  tables (transport 4: 0x200c3174, 0xd: 0x200c4f38, 0xe: 0x200c1f90+0x4c) [0xbf2fe-0xbf32c] —
  and, if one exists, resolves its connection (0x7878c) and takes holds: 0x9a364(conn,0,4),
  0xabfc4(4), 0x992f8-path(5,0), 0x920b4(0x44); sets +0x46 = 1. bf2b0(1) (any other location,
  or loop cmd 0xf [0x7a2d4]) releases them (0xabff8(4), 0x992f8(5), 0x920e4(0x44)).
- A GATT (transport 0x10/0x11) UARP session is not in those tables, so closing the lid takes
  no hold for it.

### Disconnect-all-except and the reconnect HSM (Known code, meanings Inferred)
- FUN_0009a364(keep_mac, keep_mac2, reason) [rkof 0x9a364-0x9a3e6]: for every host ACL in the
  connection table 0x2005ede8 (2 x 0x98) whose MAC is neither keep_mac nor keep_mac2, calls
  FUN_00099d88(mac, reason) [rkof 0x99d88]: writes history kind 0x0b with the reason
  (0x704a0) and disconnects (reason 1..0x16 mapped to HCI 0x13/0x14/0x15, 250/1000 ms). Only 3
  callers: the lid-closed UARP hold above (reason 4, keep = the UARP host) [0xbf344], 0xc0706
  (reason 0xd), 0xcd1ae (reason 0xc).
- 0x2004f7ec is an HSM (descriptor 0x1c6d38, built [rkof 0x15299a]) with two target MACs (+0, +6),
  a reason bitmask (+0x24; set 0x9925c -> event 1, clear 0x992f8 -> event 2 when empty), timer 0x16
  (period +0x1c, 500 ms set at 0xdb04e) and actions that look up a paired record and page it
  (0x94004, 0x3b7ac, 0x9d1cc(mac,1)) [rkof 0xdafdc-0xdb204]. Inferred: the auto-reconnect/paging
  machine (matches the PagedHost history events: 3 tries 6 s apart after power-on). The lid-closed
  AAP-UARP hold sets its reason 5 (0x1b1e2c -> 0x9925c(5,0)) and clears it on reopen.
- => Inferred, strong: with an Apple host's AAP-UARP session up, closing the lid makes the bud
  drop every OTHER host, keep the updating host, vote 4/0x44 resources and (re)page that host.
  None of this exists for GATT (0x10/0x11).

### Observed (journal, Oct 27, our two non-Apple hosts, no transfer)
14:46:31: lid closed with laptop (64:4a:..) and phone (08:8b:..) connected: the pods sent one
ListeningMode packet, then dropped the laptop 0.18 s later, HCI reason 0x13 (pods-initiated). The
next 0x2B history shows both hosts "HostLeft 722" at the same second and no kind-0x0b event, so not
the 0x99d88 path. History values line up with 700 + HCI code (704 page timeout 0x04, 708 0x08
timeout, 721 0x15, 722 0x16 local host terminated, 734 0x22) — Inferred (BlueSDK OI_HCIERR base).
The generic lid-close disconnect itself was not located.

## Answers (summary)

Q1 closed lid.
- Known: lid closed reaches the bud as case message -> 0x94f88 -> notification 0x38 -> case-location
  HSM (loc 6) -> 'LO' broker message; subscribers include the UARP platform (0xbfac0) and a SIF
  power-state bit (CASE_LID_OPEN).
- Known: the only update-related exception is FUN_000bf2b0: at loc 6 with an AAP (transport
  4/0xd/0xe) UARP session it disconnects all other hosts, takes holds and turns on the reconnect
  HSM reason 5; released at any other location. GATT sessions are excluded (bee30/bef74 look only
  at AAP tables). No AACP control command or UARP message found that a host can use to set it.
- Known (STFC): after the lid is closed, the case puts a bud in ULP once the "30 s met" lid flag
  holds, unless the bud reports bt_prog (S0 bit 6) or a bud/case fwup is active. bt_prog
  [rkof 0x15f6e4] = be420 (AAP-UARP busy on 4/0xd, transport 3 up, sibling relay, platform state 5)
  | other platform flags | 0xb9dfc; GATT transfers don't appear in it. Host connections alone
  (conn_cnt) don't block ULP.
- Observed: disconnect within ~0.2 s, all hosts. Not established: where the generic disconnect
  happens, and whether a GATT host could page the pods back in a closed case before ULP.
- Conclusion (Inferred): a non-Apple GATT host cannot run a transfer with the lid closed; stage
  with the lid open (charging), then close the lid and let the case copy/apply (as observed).

Q2 case version: see ui-q2.md. No bud path (0x1D slot 3 is the case SERIAL; VERS asset has the
case serial). NERF DIS has only PnP ID; Find My AIS char 7 = case MCU version (8-bit minor),
SEC_OPEN, if AIS (not POSH) is registered; connectable only in timed windows.

Q3/Q4: see ui-q34.md (two endpoint slots keyed by transport id; Sync only flushes own queue;
identical offer from B merges/takes over A's transfer at A's offset, A gets 0x0003; newer offer
abandons (0x0803), older denied; no in-progress indication; no data timeout but GATT transports
idle >~25 s are torn down until FTAB metadata arrives; resume within one boot by re-offering the
identical package; a failed release can drop an earlier staged image; case reopen resets).


## Detail: the case's version


## (a) Case version through the buds (rkof)

- AACP 0x1D builder FUN_00088734 [rkof 0x88734-0x88900], slot table RAM 0x20002c14 (Known):
  0 name (0x14fe04), 1 model (0x15efb0), 3 serial = NVRAM `case_sn` via 0x7f5b4 [lit 0x7f5ec -> 'case_sn' 0x1e2076]
  (i.e. the set/case serial), 4 and 5 own firmware "%llu.%llu.%llu" (a5fec/a5ffc/a600e), 6 "1.0.0",
  7 updater bundle id, 8/9 left/right bud serials (7f698/7f6c4 -> own a6098 / peer 7f550 by side),
  10 bver number "%d" [0x2004fd60+0x38], 11/12 16-byte NVRAM my_uuid / cpl_uuid (b119c [lits 0xb1248/0xb124c]),
  13/14 "%u" of NVRAM awd_ftu / cpl_ftu (b13a0 [lits 0xb13c8/0xb13cc]). No case firmware version. Known.
- UARP info tags 1-9, vendor properties 1/2, Bulk Info TLVs: sources already mapped in docs/uarp.md / update.md:
  all the bud's own data; none reads case data. Known.
- What the bud keeps of the case: NVRAM `caseCoup` (0x32 B, RAM 0x200c57d4) written from a case message in the
  case dispatcher [rkof 0x15acde-0x15ad2c]: +0 6 bytes (logged as an address), +6 20-byte string (the case SN:
  STFC's coupling compares 20-byte "BudSN"/"CaseSN" [STFC 0x08026a78-0x08026af4]), +0x1a 24 bytes (getters:
  u32 at +0x1c [rkof 0x94d28], 13 bytes at +0x20 [rkof 0x94d4c]; meaning not established). NVRAM caseInfo (6 B)
  and caseDtls (13 B) in the bootInfo block [rkof 0xa5528-0xa55b2, 0xa5bdc]. Nothing found that stores a case
  firmware version on the bud.
- UARP dynamic asset VERS (solicit 0x11 tag 'VERS' -> type 7 [rkof 0xbde0e]; loop cmd 7 [rkof 0x7a200, 0x7a698-0x7a72a]):
  tag set 'VERS' (0x106454), one payload 'FTAB' with TLV 0x44e66e00 (0x20 B: the bud's two 16-B versions, bfdb8),
  TLV 0x44e66e01 = caseCoup+6 (20 B, case SN) if caseCoup is set (0x94e0c), and TLV 0x44e66e01? from a6098 (own
  serial, 0x14 B). So the VERS asset reports bud versions + serials, not the case version. Known (TLV values
  Inferred from the getters).
- STFC keeps per-bud records with "active", "pend", "long" and a "case: %s" string and "hw_ver" [STFC 0x0801db08]
  (what the bud reported to the case; the "case" field Inferred = a case-version field in the bud's report, cf. the
  bud bver parser's csv1:/ioe1: fields, absent in 9A348's bver). The direction case->bud of the case's own version
  was not found.

## (b) The case's BLE chip (NERF, image based at 0)

- DIS: FUN_00030698 is Nordic's ble_dis_init (service 0x180A, adds 2A29/2A24/2A25/2A27/2A26/2A28/2A23/2A2A/2A50
  each only if set) [NERF 0x30698-0x30800]. The caller [NERF 0x3c03a-0x3c074] memsets the init struct and sets
  ONLY p_pnp_id (vendor id source 1, vendor 0x004C, product id = st_info+0x48 via 0x3f748, version 1) and
  dis_char_rd_sec = 1 (SEC_OPEN). So the case's DIS has just PnP ID, readable without encryption, and **no
  firmware revision (2A26)**. Known. TPS (0x305bc) also open.
- AIS (Find My Accessory Information Service, 87290102-3c51-43b1-a1a9-11b9dc38478b, chars 6aa5000N-6352-4d57-
  a7b4-003a416fbb0b) [NERF 0x3c448-0x3c7dc]: every char read_access = 1 (SEC_OPEN) ([sp+0x59]=1 in
  ble_add_char_params_t). Char 7 (Find My "firmware version", u32) = FUN_0003f768 [NERF 0x3f768]:
  (st_info+0x42 u16 << 16) | (st_info+0x44 u16 << 8, truncated to 16 bits) | st_info+0x46 u8, st_info =
  RAM 0x20008c00 ("st_info.c"): the case MCU's (ST) version as reported to the nRF. Others: 1 product data,
  2 "Apple", 3 model, 5 category 0x95, 6 capabilities 0x3b, 8 Find My version, 9 battery type 2, 10 level. Known.
  BUT AIS is registered only when config byte [[cfg]+0x44] == 0; otherwise POSH (an Apple-private service)
  is registered instead [NERF 0x3c2e4-0x3c2ec]; which branch a retail case takes is not established.
  Encoding caveat (Inferred): the minor field is packed in 8 bits, so 90.390 would not round-trip.
- UARP service: registered only when the case is already paired (0x3f2cc bit 0) [NERF 0x3c3ee-0x3c3f4 ->
  0x3bd54]; writes need an encrypted link (case.md).
- Connectable advertising [NERF 0x39c20-0x39dda]: connectable (adv type 1) only when flag 0x2000803f
  ("persistent" window, cleared by a timeout "BTFinding - Persistent Timed Out" [0x37028]) or 0x2000803e
  (a timed BT-finding window with a disconnect timer, set by 0x37ba4) is set AND fewer than 2 connections
  (0x3a7a8); otherwise non-connectable (type 4) "BTFinding - NON Connectable!". What sets those windows
  (MCU command, owner nearby, lid) not traced. No owner check on connect itself; pairing is refused once
  paired ("Already paired. Reject request." [0x3bb88 ev 5]), but AIS/DIS/TPS reads need no pairing.
- Find My characteristics (FMNS) write/cccd access 2 (just works) [NERF 0x3c14a-0x3c150].

## Not established
- Whether a retail case registers AIS or POSH; when its advertising is connectable for a non-owner
  (window triggers); whether the BLE address is resolvable/rotating for a non-owner to find it.
- Meaning of caseCoup +0x1c (u32) and +0x20 (13 bytes); whether the case ever sends its own version to a bud.
- BLE proximity advertisement (rkof): no version field checked in code (LibrePods' layout has none).


## Detail: several hosts and robustness


## Accessory endpoints (Known)
- FUN_00076cb0 (add accessory endpoint, called from be4ec on transport up for 0x02/0x09/0x10/0x11/side-dependent 0x18/0x19):
  a fixed table of **2** slots x 0xb0 [DAT_00076dec], counter [DAT_00076de8]; if the counter is 2 it logs and fails;
  if a slot with the same transport id (+0x10) is active it logs and fails (no second endpoint for the same id);
  slot+0x10 = transport id, slot+0x28 = the library endpoint (FUN_0010661c). [rkof 0x76cb0-0x76de0]
- So the accessory instance serves at most two controller endpoints at once, keyed by transport id.
- Removal (transport down, be4ec(0,id)) for 0x10/0x11/0x02/0x09: post loop cmd 0xa (be2b4: re-read running/staged
  versions into ctx+0x370/+0x380, i.e. info tags 5/6) then FUN_00076ed4 [rkof 0xbe5fc-0xbe606]:
  - library FUN_00106740: frees the ep's tx queues, FUN_00103bae(ctx, ep, 1): every asset of that ep in ctx+0x164:
    SuperBinary assets (flags&1) are only DETACHED (+0x1d4 ep = 0, +0x1d8 = 0) and kept; dynamic assets marked
    rescinded and released [rkof 0x103bae].
  - platform: dynamic-asset records of that ep (lists +0x398/+0x39c) abandoned (0x1b2796 -> 106a12) [0x76f1c..];
    if it was the LAST endpoint (counter -> 0): SuperBinary records in state 7 freed, others kept in ctx+0x394,
    then FUN_001065d8 drops all library assets (ctx+0x164) [rkof 0x76f50-0x76fa0, 0x1065d8].

## Sync (msg 0) on the accessory side (Known)
FUN_00105e20: if ctx+0x158 == 0, FUN_0010744e flushes THAT endpoint's pending tx list (ep+0x5c) only, then
replies Sync [rkof 0x105e20-0x105e64, 0x10744e]. No asset, staging or other-endpoint state is touched.
Side effect for the same host: an outstanding data request (msg 6) of its own asset is dropped from the queue
if not yet sent; nothing re-issues it until a new offer/resume (Inferred).
Ack tracking (ctx+0x150) is off for this instance, so tx is sent immediately (FUN_001073d4), no retransmits.

## Offer from host B while host A has an asset (Known code; FUN_000756c8 + FUN_000bd938)
Platform record per offered asset (alloc 0x1b0d12, list ctx+0x394): [0]=UARP asset, [1]=endpoint record
(+0x10 = transport id), [4]=state (1 deferred, 2 accepted, 7 done), current SuperBinary record = [0x200c1f90+8].
1. Merge first [rkof 0x75740-0x757e0]: any other record (state not 4/5) whose asset has identical contents
   (FUN_00108374: flags +0x2e, tag +0x30, length +0x44, payload count +0x11c, version +0x34 equal) is merged:
   FUN_00106b62(old, new) copies the old asset's progress (first 0x2c B, +0xc0..+0x11c request state incl.
   offset +0xfc, +0x120.., +0x12c..) into the new one and marks old rescinded; old record's buffers freed,
   bdf24 clears "current" if it was the old one, then FUN_00106a12(ctx, old, 0) = processing notification
   flags 0x0003 (abandoned, reason 0) to the OLD asset's endpoint. => the same SuperBinary offered by B
   takes over A's transfer and B continues from A's offset; A is told "abandoned".
2. bd938 (SuperBinary, offering transport t, current record C with transport c) [rkof 0xbd938-0xbdd8e]:
   - not newer than running and t not 0x10/0x11 -> deny reason 1 (flags 0x0003).
   - no C -> C = new (accept).  C in state 7 -> abandon C (flags 0x0803), C = new.
   - C's version newer than offered -> deny 1 (0x0003); offered newer -> abandon C with 0x800 (0x0803), C = new.
   - equal version (different contents, else it would have merged): c in {0x10,0x11} -> **deny reason 2
     (0x0002)**; c in {01,02,10,11,18,19} and t in {4,0xd} -> deny 2; t not 0x10/0x11 and (c not 4/0xd or t not
     in that set): t != c -> deny reason 0 (0x0002? flags = 0|2), t == c -> abandon C 0x700, C = new;
     else (t is 0x10/0x11, or c is 4/0xd and t in set) -> abandon C (0x0703), C = new, bbf5c(0,9).
   - after accept: t in {4,0xd} and bf0f0()==0 -> **defer (3)**; only AAP-type transports are ever deferred here.
   - deny reason 4 if the 8 KiB data buffer can't be attached (787c4: two alternating buffers at 0x200586f0+0x2000/0x4000).
   - flags sent = reason | 2 (FUN_001069d0 `orr r3,r6,#2`) [rkof 0x1069f2]; default reason 0x200 -> 0x0202
     when bd938 returns 2 without setting one (offering endpoint type 0xe, HSML dynamic path).
3. Rescind (0x0c) from B: the library rescind is per assetID of the receiving context; B can rescind-all only
   assets it can name; (Inferred) a rescind-all from B hits assets regardless of owner since the library's asset
   list is shared (handler not re-read here).
- No arbitration message: processing notifications go to the asset's own endpoint (103e32 -> asset+0x1d4)
  only. Another host sees an update only indirectly: info tag 6 (staged version) stays the old/none value
  during a transfer (the FTAB header is withheld until verified) and flips only after full staging; tag 7
  statistics are per endpoint. Nothing found that exposes "transfer in progress" to a second host.

## Transport idle teardown (Known code; time unit Inferred)
FUN_0008395c [rkof 0x8395c-0x839cc], registered at [0x160678]: for each up transport (30 ids, table 0x1c2230),
if now - last-activity (table 0x2004eb80, refreshed by FUN_00084840 on transport up and on every send through
the generic sender be929 [rkof 0xbe9c6], also 0x15bf4a) >= 25 000 001 (Inferred µs: ~25 s) AND transport
flag +0x41 == 0 -> FUN_000846d4(id, 0) = transport DOWN -> be4ec down path -> endpoint removed.
- 0x10/0x11 come up with +0x41 = 0 (FUN_00084a76(id,0) [rkof 0xbe5aa]): an idle GATT UARP session is torn down
  after ~25 s without traffic.
- FUN_000bd3ac (SuperBinary metadata for FTAB/LTAB/RTAB) sets +0x41 = 1 for 0x10 AND 0x11 [rkof 0xbd63a-0xbd646]
  (exempt during the transfer); FUN_000bf378(x,1) (loop cmds 0xb/0xc/0x1c/0x1d) clears it [rkof 0xbf39a-0xbf3aa].
  bf378(x,0) retries the a7108 resource request up to 3 times, then a fatal abort (Spicy_LogAbort) [0xbf3b8-0xbf3d4].
- Transport up also arms something for 10000 (Inferred 10 s) via [0x200c3488+0x10] vt+0x10 [rkof 0x84740-0x84752].

## Resume / timeouts / end of staged state
- No accessory-side data-response timeout in the UARP library (no ack tracking, no retransmit timer found);
  a stalled request waits until the transport goes down (idle 25 s, unless exempt), a new offer, or reset.
- Resume within one boot (Inferred, strong): after the endpoint is removed the SuperBinary record survives
  (unless state 7); re-offering the identical SuperBinary merges (106b62 copies the offset/section state) and
  FUN_00106810 continues from the copied state (checks version/length equal, then resumes header / payload
  headers / metadata / data where the copied counters say) [rkof 0x106810-0x10693e]; bf414 reopens the same
  staging file while [0x200c16d0+0x49] is set. If it was the last endpoint, 1065d8 also drops the library
  assets (buffers freed by 103c56) - whether the record's asset pointer is still valid then is not established.
- Across a reboot or UARP reset (case close/open observed): RAM state is gone; the inactive slot's header is
  0xFF, so the next offer stages from offset 0 (bf414 reformats when +0x49 is clear).
- SuperBinary release (FUN_000bccd0, result 0 = not completed): if the digest buffers are missing or differ,
  and the inactive slot has a valid header (a5674 != 0xff), **ac3ec drops the previously staged image** and
  be2b4 refreshes the versions [rkof 0xbce78-0xbce90]. Clears current [0x200c1f90+8] [0xbcf1e].
- Staged-but-not-applied ends by: host 0x0A (bc390), case B2B/APPLY (observed), a new FTAB staging (bf414
  reformats the inactive slot), an unsuccessful SuperBinary release (above), failed boot tries (ac3ec).
  Other ac350 callers (0xc2ad2 in FUN_000c29d4, 0xd69fa, 0x15a1c6) not decoded here.
- Defer (bd938 = 3): only when the offering transport is 4/0xd (AAP) and FUN_000bf0f0() (= !vcall via
  0x91c68, a resource manager [0x200c1c2c+0x14]) says not ready [rkof 0xbdbc4-0xbdbd6]. GATT offers are never
  deferred by bd938, so the observed silence without charging is not explained by this function (open).
