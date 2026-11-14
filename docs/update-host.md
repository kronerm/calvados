# What a host-side updater needs to know

For calvados' updater (`crates/protocol/src/update.rs`): closing the lid,
several hosts, resuming, the case's version. AirPods Pro 2 (USB-C), build
9A348; static analysis of `rkof`, `STFC` and `NERF`, cited `[image 0xADDR]`,
Known vs Inferred marked, Observed dated. The protocol: [uarp.md](uarp.md);
what a bud does with the image: [update.md](update.md); the other bud and the
case: [sibling-and-case.md](sibling-and-case.md), [case.md](case.md).

## Answers

- **Closing the lid** normally drops every host within ~0.2 s. A host on
  AACP 0x4F (transport 0xd) with an offer in progress is kept instead,
  every other host dropped (Known code, Observed Nov 13/14); a GATT host
  (0x10/0x11) never is. So stage with the lid open (case charging), then
  close it and let the case copy and apply. Details below.
- **Several hosts**: over AACP one host at a time has the UARP endpoint; a
  second gets no answer at all (calvados: `NoAnswer` after 10 s). Across
  transports an identical offer takes over the other's transfer at its
  offset. Nothing tells a host that another is transferring.
- **Resuming**: within one boot, re-offering the identical package continues
  at the pods' offset. A reboot or the case closing and reopening starts
  over from 0. The pods have no data timeout (calvados' watchdog rescinds).
- **The case's version**: no bud stores or reports it. The case's lid-close
  tunnel (downstream 1, [case.md](case.md)) is the only route through a bud
  and has never come through; its BLE chip exposes it only in Find My's AIS.

## Closing the lid

### The case's ULP gate (STFC, Known code, names from the log format)

STFC `FUN_08022314` [STFC 0x08022314-0x08022680] decides per bud whether it
is "ULP Ready" (logs "%s ULP %s: Bud: oob,obc_rdy,data_valid,chg_st_rdy,bt,
ba,my_bud,sbp; Case: pwr,lid,ggSoC,uiSoC,fwup,ttr" [str 0x080752e8] and
"%s ULP %sReady. 30s Met: lid,fwup,bt,pwr,dbnc_ok; in_case_2_hr,t_rem(s)"
[str 0x08075395]):
- lid open ([0x200350c8+5] != 0) → not ready [0x08022444]; needs the "30 s
  met" lid flag [0x20034fc8+0x6a]==1, debounce [+0x6b]==0, data_valid,
  mode != 9 [0x08022460-0x08022480].
- **bt or fwup set → not ready** [0x08022482-0x0802248e]: bt = bit 6 of the
  bud records' status byte S0 (0x2002658c+0x15 | +0x55); fwup = 0x0803774c()
  (bud fwup active) or a case OTA aimed at this bud (0x08035838/0x08035864)
  [0x080223e2-0x08022404].
- S0 (Known both sides): rkof builds it [rkof 0x158940-0x1589aa]: bit 7
  always 1, bits 4-5 number of host ACLs, bit 1 role (0xc61a4), bit 2 oob
  (0x8dc18), bit 3 utp (0xc6214), **bit 6 = 0x15f6e4() != 0**. STFC unpacks
  it as "S0: pair, role, oob_mode, utp_conn, conn_cnt, bt_prog, msmat_supp"
  [STFC 0x0801dca6-0x0801dcd6]: bt = **bt_prog**. bt_prog [rkof 0x15f6e4] =
  be420 (AAP-UARP busy on 4/0xd, transport 3 up, sibling relay, platform
  state 5) | other platform flags | 0xb9dfc. Host connections alone
  (conn_cnt) and GATT transfers don't block ULP.
- Inferred: ULP is the deep sleep the case puts buds into 30 s after the
  lid closes; a bud with an update or AAP-UARP busy is kept out of it.

### The bud's lid and dock state (Known)

- State block 0x2004f6d0: +4 dock (1 undocked, 2 docked, 3 docked_dead),
  +0x4c lid (0 unknown, 1 closed, 2 open), +0 CLI override; names from the
  CLI table [rkof 0x1c28bc/0x1c28cc] ("case" group {batt, incase, dock, lid}).
- Setter `FUN_00094f88(lid)` [rkof 0x94f88-0x94fe6], fed from the case
  message parser [rkof 0x15887e-0x1588ac]: on open bumps a counter (+0x50)
  and calls 0xc1c7c(0x6d, 10000); posts notification **0x38**. Dock setter
  `FUN_00094f14`: on change resets lid to unknown, notification **0x35**.
- Main-task loop [rkof 0x155386-0x1554ec]: 0x38 → lid closed event
  **0x1e**, open **0x1d**; 0x35 → docked 0x1b, undocked 0x1c, docked_dead
  0x1f; through `FUN_000af978` → `FUN_000af79c`, which maps them (table
  0x1c40dc) to the "case location" HSM (descriptor 0x1c8068, object
  0x200b9d58): docked + lid closed = state 8 → **location 6**, lid open →
  5, docked 4, dead 7. Locations go to two per-bud location HSMs, to
  `FUN_000d4730` (SIF state bits DOCKED, CASE_LID_OPEN [names 0x1c4420])
  and as 'LO' (0x4c4f) broker messages {location, bud} [rkof 0xde4b8 → 0xaf28c].

### The UARP hold (Known)

- The UARP platform subscribes to 'LO' [rkof 0xbfac0-0xbfae4]: for its own
  bud it posts loop command 0xd, which calls **`FUN_000bf2b0(location != 6)`**
  [rkof 0x7a3f4-0x7a3fe].
- `bf2b0(0)` (lid just closed) [rkof 0xbf2d8-0xbf35c], unless already held
  ([0x200c16d0+0x46]): finds the host's UARP session with bee30/bef74, which
  search only the AAP tables (transport 4: 0x200c3174, 0xd: 0x200c4f38,
  0xe: 0x200c1f90+0x4c) and match only a session holding an accepted (2),
  deferred (1) or state-6 offer ([case.md](case.md) Q3). If found: holds
  0x9a364(conn,0,4), 0xabfc4(4), 0x992f8-path(5,0), 0x920b4(0x44); +0x46 = 1.
  `bf2b0(1)` (any other location, or loop cmd 0xf [0x7a2d4]) releases them.
- `FUN_0009a364(keep_mac, keep_mac2, reason)` [rkof 0x9a364-0x9a3e6]
  disconnects every host ACL but the kept ones through `FUN_00099d88`
  (history kind 0x0b with the reason, HCI 0x13/0x14/0x15 after 250/1000
  ms). Callers: this hold (reason 4, keeping the UARP host) [0xbf344],
  0xc0706 (reason 0xd), 0xcd1ae (reason 0xc).
- Reason 5 goes to the reconnect HSM 0x2004f7ec (descriptor 0x1c6d38; two
  target MACs, a reason bitmask, 500 ms timer; actions page a paired record
  [rkof 0xdafdc-0xdb204]; [paging.md](paging.md)): set on close, cleared on
  reopen. Inferred: the held host is paged back if it drops.
- So with an offer in progress on AACP, closing the lid drops every other
  host and keeps the updating one; no AACP or UARP message sets the hold
  otherwise. The generic lid-close drop of all hosts wasn't located.

### Observed

- Oct 27 (no UARP session): lid closed with laptop and phone connected:
  one ListeningMode packet, then the laptop dropped 0.18 s later, HCI 0x13.
  The next 0x2B history: both hosts "HostLeft 722" in the same second, no
  kind-0x0b event, so not the 0x99d88 path. History values are 700 + HCI
  code (704 page timeout, 708 timeout, 721 0x15, 722 0x16, 734 0x22;
  Inferred: BlueSDK OI_HCIERR base).
- Nov 13 (`try-probe-case`, offer held at 10 %): every close dropped the
  phone and kept the laptop; the link held through closes of up to 2m20s,
  but twice a second close ended it after ~31 s (0x13) and the pods
  rebooted ([case.md](case.md)).
- Nov 14 (fully staged over AACP): the laptop's link held 30 s after the
  close, then 0x13 ([uarp.md](uarp.md)).

## The case's version

### Through the buds (rkof): none (Known)

- AACP 0x1D, `FUN_00088734` [rkof 0x88734-0x88900], slots (RAM 0x20002c14):
  0 name, 1 model, 3 NVRAM `case_sn` (the set's serial) [lit 0x7f5ec], 4 and
  5 own firmware "%llu.%llu.%llu", 6 "1.0.0", 7 updater bundle id, 8/9
  left/right bud serials, 10 bver number [0x2004fd60+0x38], 11/12 NVRAM
  my_uuid / cpl_uuid, 13/14 awd_ftu / cpl_ftu. No case version.
- UARP info tags, vendor properties and Bulk Info TLVs
  ([uarp.md](uarp.md) §3, [update.md](update.md) §5) are all the bud's own.
- Dynamic asset VERS (solicit 0x11 'VERS' → type 7 [rkof 0xbde0e]; loop cmd
  7 [rkof 0x7a698-0x7a72a]): one payload 'FTAB' with TLV 0x44e66e00 (the
  bud's two 16-byte versions, bfdb8), TLV 0x44e66e01 = the case serial
  (caseCoup+6) if set, and the bud's own serial (TLV values Inferred from
  the getters). Bud versions and serials only.
- Kept of the case: NVRAM `caseCoup` (0x32 B, RAM 0x200c57d4), from a case
  message [rkof 0x15acde-0x15ad2c]: +0 6 bytes (logged as an address), +6
  the 20-byte case SN (cf. STFC's "BudSN"/"CaseSN" [STFC 0x08026a78]), +0x1a
  24 bytes (getters: u32 at +0x1c [rkof 0x94d28], 13 bytes at +0x20
  [rkof 0x94d4c]; meaning unknown). NVRAM caseInfo (6 B) and caseDtls
  (13 B) [rkof 0xa5528-0xa55b2, 0xa5bdc]. No case firmware version.
- STFC keeps per-bud records with "active", "pend", "long", "case: %s" and
  "hw_ver" [STFC 0x0801db08] (Inferred: the bud's report to the case). The
  case's own version going case → bud was not found.
- The case as UARP downstream 1 through the primary (tunnel at lid close,
  asked with relay 0x1B): [case.md](case.md); never seen, and 7B21's case
  MCU has the tunnel switched off.

### The case's BLE chip (NERF, image at 0)

- DIS: Nordic's `ble_dis_init` (`FUN_00030698`); the caller [NERF
  0x3c03a-0x3c074] sets only the PnP ID (vendor 0x004C, product st_info+0x48,
  version 1), SEC_OPEN: **no firmware revision (2A26)**. TPS also open. Known.
- AIS (Find My Accessory Information, 87290102-3c51-43b1-a1a9-11b9dc38478b,
  chars 6aa5000N-6352-4d57-a7b4-003a416fbb0b) [NERF 0x3c448-0x3c7dc], every
  char SEC_OPEN. Char 7 (firmware version, u32) = `FUN_0003f768`: (st_info+0x42
  u16 << 16) | (st_info+0x44 u16 << 8, truncated to 16 bits) | st_info+0x46,
  st_info = RAM 0x20008c00: the case MCU's version as reported to the nRF.
  Others: 1 product data, 2 "Apple", 3 model, 5 category 0x95, 6
  capabilities 0x3b, 8 Find My version, 9 battery type 2, 10 level. Known.
  AIS is registered only when [[cfg]+0x44] == 0, else the Apple-private
  POSH [NERF 0x3c2e4-0x3c2ec]. Inferred: the 8-bit minor wouldn't
  round-trip 90.390.
- UARP service: only when the case is already paired [NERF 0x3c3ee-0x3c3f4];
  writes need an encrypted link ([case.md](case.md)).
- Connectable advertising [NERF 0x39c20-0x39dda] only in a "persistent"
  window (0x2000803f, cleared by "BTFinding - Persistent Timed Out") or a
  timed BT-finding window (0x2000803e, set by 0x37ba4), with fewer than 2
  connections; else non-connectable. Pairing is refused once paired
  ("Already paired. Reject request."), but AIS/DIS/TPS reads need none.
  FMNS write/CCCD access 2 (just works) [NERF 0x3c14a-0x3c150].

### Not established

- AIS or POSH on a retail case; what opens the connectable windows; whether
  its address can be found by a non-owner.
- caseCoup +0x1c and +0x20; whether the case ever sends its version to a bud.
- No version field in the BLE proximity advertisement's code path.

## Several hosts

### One host at a time over AACP

Known: the bud's endpoint table keyed by transport [rkof 0x76e0c] (4
GATT-AAP, 0xd AACP, 0xe) holds one AACP UARP endpoint for all hosts. 0xbea24
creates it for a host only when none of type 0xd exists (and drops a GATT
one); 0x9b2d4, from the host-link drop path (0x98534), removes a host's.
Observed Nov 13: the phone's version query (15:30:33 UTC) was answered; the
laptop's 6 s later, and its offer, got nothing at all; with the phone away
the laptop was answered. Inferred: the first host to send UARP keeps the
endpoint until its link drops (whether creation waits for the first 0x4F or
the AACP connect wasn't traced). Calvados fails such an offer after 10 s as
`NoAnswer`.

### Accessory endpoints (Known)

- `FUN_00076cb0` (add an accessory endpoint, from be4ec on transport up for
  0x02/0x09/0x10/0x11 and side-dependent 0x18/0x19): **2** slots × 0xb0
  [DAT_00076dec], counter [DAT_00076de8]; fails when full or when a slot
  with the same transport id (+0x10) is active [rkof 0x76cb0-0x76de0].
- Removal (be4ec(0,id)) for 0x10/0x11/0x02/0x09: loop cmd 0xa (be2b4
  re-reads running/staged versions, info tags 5/6), then `FUN_00076ed4`
  [rkof 0xbe5fc-0xbe606]:
  - library `FUN_00106740`: frees the ep's tx queues; `FUN_00103bae`:
    SuperBinary assets of the ep are only **detached** (+0x1d4, +0x1d8 = 0)
    and kept; dynamic assets rescinded and released [rkof 0x103bae].
  - platform: the ep's dynamic-asset records abandoned [0x76f1c..]; if it
    was the last endpoint, SuperBinary records in state 7 freed, others
    kept in ctx+0x394, then `FUN_001065d8` drops all library assets
    [rkof 0x76f50-0x76fa0].

### Sync (Known)

`FUN_00105e20` flushes only that endpoint's pending tx list, then replies
[rkof 0x105e20-0x105e64, 0x10744e]; no asset, staging or other endpoint is
touched. Inferred: a host's own unsent data request is dropped with it and
not re-issued until a new offer.

### An offer from host B while host A has an asset (Known; `FUN_000756c8`, `FUN_000bd938`)

Platform record per offered asset (list ctx+0x394): [0] UARP asset, [1]
endpoint record (+0x10 transport id), [4] state (1 deferred, 2 accepted,
7 done); current SuperBinary record [0x200c1f90+8].
1. Merge first [rkof 0x75740-0x757e0]: a record (state not 4/5) whose asset
   has identical contents (`FUN_00108374`: flags, tag, length, payload
   count, version) is merged: `FUN_00106b62(old, new)` copies the progress
   (incl. offset +0xfc) into the new asset, the old is rescinded, and A's
   endpoint gets flags 0x0003 (abandoned). B continues at A's offset.
2. `bd938` (offering transport t, current record C on transport c) [rkof 0xbd938-0xbdd8e]:
   - not newer than running and t not 0x10/0x11 → deny 1 (0x0003);
   - no C → accept; C in state 7 → abandon C (0x0803), accept;
   - C newer than offered → deny 1; offered newer → abandon C (0x0803), accept;
   - equal version, different contents: c in {0x10,0x11} → deny 2 (0x0002);
     c in {01,02,10,11,18,19} and t in {4,0xd} → deny 2; t not 0x10/0x11
     and (c not 4/0xd or t not in that set): t != c → deny 0, t == c →
     abandon C (0x700), accept; else abandon C (0x0703), accept, bbf5c(0,9);
   - after accept: t in {4,0xd} and `bf0f0()` == 0 (a resource manager,
     !vcall via 0x91c68) → **defer** [rkof 0xbdbc4-0xbdbd6]; only these
     transports are deferred here;
   - deny 4 if the 8 KiB data buffer can't be attached (787c4);
   - flags = reason | 2 [rkof 0x1069f2]; default 0x200 → 0x0202 when bd938
     returns 2 without a reason (e.g. offering endpoint 0xe, HSML path).
3. Rescind (0x0c): per assetID of the receiving context. Inferred: a
   rescind-all from B hits assets of every owner (the asset list is shared).
- Over AACP B never gets this far (one endpoint, above): the merge matters
  across transports (Inferred).
- Processing notifications go only to the asset's endpoint. Another host
  sees an update only indirectly: info tag 6 (staged) changes only after
  full staging (the `FTAB` header is withheld until verified); tag 7 is per
  endpoint. Nothing exposes "transfer in progress".

### Idle teardown (Known code; time unit Inferred)

`FUN_0008395c` [rkof 0x8395c-0x839cc]: a transport idle ≥ 25 000 001
(Inferred µs, ~25 s; activity = transport up and every send through be929)
with flag +0x41 == 0 is taken down (`FUN_000846d4(id, 0)`), removing its
endpoint.
- 0x10/0x11 come up with +0x41 = 0 [rkof 0xbe5aa]: idle GATT UARP sessions
  drop after ~25 s. FTAB/LTAB/RTAB metadata (`FUN_000bd3ac`) sets +0x41 for
  0x10 and 0x11 (exempt during the transfer) [rkof 0xbd63a-0xbd646];
  `bf378(x,1)` (loop cmds 0xb/0xc/0x1c/0x1d) clears it; `bf378(x,0)` retries
  the a7108 resource request 3 times, then aborts (Spicy_LogAbort) [0xbf3b8-0xbf3d4].
- Transport up also arms something for 10000 (Inferred 10 s) [rkof 0x84740-0x84752].

## Resuming, timeouts, the end of a staged image

- The pods have no data-response timeout (no ack tracking, no retransmit
  timer): a stalled request waits for the transport to drop (idle 25 s
  unless exempt), a new offer or a reset. Calvados' watchdog rescinds after
  60 s without a request.
- Resume within one boot (Inferred, strong): the SuperBinary record
  survives the endpoint's removal (unless state 7); the identical offer
  merges (106b62 copies offset and section state) and `FUN_00106810`
  continues where the counters say [rkof 0x106810-0x10693e]; bf414 reopens
  the same staging file while [0x200c16d0+0x49] is set. If it was the last
  endpoint, 1065d8 also frees the library assets: whether the record's
  asset pointer stays valid is not established. Calvados re-offers after a
  drop; untested (Nov 14 had no drop).
- Across a reboot or UARP reset (case close and reopen): RAM state is gone,
  the inactive slot's header is 0xFF, the next offer starts at 0.
- An unsuccessful SuperBinary release (`FUN_000bccd0`, digests missing or
  different) with a valid staged header **drops the previously staged
  image** (ac3ec) [rkof 0xbce78-0xbce90].
- A staged image ends by: the case's copy and apply (observed), 0x0A from
  0x10/0x11, a new `FTAB` staging (bf414 reformats the slot), such a
  release, or failed boot tries ([update.md](update.md) §1). Other ac350
  callers (0xc2ad2, 0xd69fa, 0x15a1c6) not decoded.
- Open: GATT offers aren't deferred by bd938, yet a GATT offer without
  charging was silently deferred ([uarp.md](uarp.md)); what does that is unknown.
