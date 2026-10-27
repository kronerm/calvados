# The other bud and the case during an update

AirPods Pro 2 (USB-C), build 9A348 (hashes, tools: [firmware.md](firmware.md);
the protocol: [uarp.md](uarp.md); a bud's own update: [update.md](update.md)).
Static analysis of `rkof` and the case's `STFC`, cited `[image 0xADDR]`;
Known = read from code or data, Inferred = marked. The case decision names
(`R2L`, `L2R`, `APPLY_L/R/BOTH`, `ERR_MANI`, "Bud server/client") are in
`STFC`'s strings.

## Memory map note (Known)
rkof's .data image: file 0x1f0000.. is copied to RAM 0x20000000 (checked: the table
RAM 0x20002ca8 [literal rkof 0xbfa4c] holds at file 0x1f2ca8 the tags/filename pointers below).

## The bud keeps SuperBinaries for others in its own flash (Known)
Table RAM 0x20002ca8 (init at file 0x1f2ca8), 5 entries x 0x1c {u32 4cc, char *file, 16 B version, u8 loaded}:
STFW -> file "ioe1", IOHM -> "iohm", IOHS -> "iohs", ESM1 -> "esm1", PDFW -> "pdfw".
FUN_000bf950(index, ep) [rkof 0xbf950]: opens the file, reads 0x2c bytes (a SuperBinary header),
takes its version (BE words), caches it, then offers it through the CONTROLLER role:
FUN_00077adc ("UARPL3Controller_offerSBFile" [str 0x1e18fb]) -> 0x106c38 (msg 0x05).
Callers of bf950: rkof 0x7a0e8, 0x7a11a, 0x7a254, 0x7a4ac (the platform event glue).

## The UARP platform task (Known) — rkof 0x78ec8 (Ghidra didn't define it; `dec 0x78ec8` creates it)
Event kind 4 = commands for the CONTROLLER role, sub-command at msg[0] [switch rkof 0x7a05a], target
remote accessory id at msg+8:
- 1: inbound bytes from a transport; role 1 -> accessory rx 0x76e80, role 2 -> controller rx 0x77a7c.
- **2: offer this bud's own FTAB** to the remote accessory [rkof 0x7a2ce..0x7ac9c]: bc020(1);
  slot = inactive if a valid staged header (a5674 != 0xff) else running [0x7a7d0-0x7a7e2];
  opens firm.00/firm.01, checks 'rkosftab', size rounded up to 4 KiB, version = bfec0
  (staged/running), payload tag 'FTAB' [lit 0x7acf4], metadata TLV 0xCCD28107 (32 B) read from
  NVRAM sha_0/sha_1 per slot [lits 0x7acfc/0x7ad00, 0x1b7e0], data callback FUN_000bd760
  [lit 0x7a91c] -> 0x77588 build -> 0x106c38 (msg 0x05) "%s: Asset <%u> offered." [0x7ac90].
  On failure bc0c4(0,7).
- 4, 7: build and offer dynamic SuperBinaries (0x17-payload list; flags +0x4a / +0x4d) via 0x76ff8.
- 5: Sync to the endpoint (controller or accessory side).
- **0x12: offer the stored file "case"** (tag 'CASE' [lit 0x7a940 = 0x45534143]) via
  UARPL3Controller_offerSBFile 0x77adc [0x7a834-0x7a8c6]; version cached at flag +0x47.
- **0x18/0x19/0x1a/0x1b: offer stored SuperBinary files** via bf950: 0x18 table[0] STFW "ioe1";
  0x19 table[1] IOHM if side()==2 else table[2] IOHS [0x7a0d8-0x7a0e8]; 0x1a ESM1; 0x1b PDFW.
- 8 bea0c, 0xa be2b4, 0xb/0xc and 0x1c/0x1d bf378(x, 1/0), 0xd/0xf bf2b0, 0xe release assets,
  0x10 ac420 + store, 0x13 0x107262, 0x14 bff00, 0x15 0xc6768(0x74,...), 0x16 reset-ish.

## Who posts those commands: transport up/down, FUN_000be4ec(up, transport id) (Known)
Switch tables decoded [tbh 0xbe536 ids 1-9, tbh 0xbe58a ids 0x10-0x1b, tbb 0xbe5ca ids 0x2d-0x30]:
on transport UP:
| id | action |
|---|---|
| 0x02, 0x09 | 0x76cb0 = add an ACCESSORY-role endpoint (a host updates us) [0xbe54c] |
| 0x10, 0x11 | 0x84a76(id,0) then accessory endpoint [0xbe5a6] (0x10 = GATT UARP service, firmware.md) |
| 0x01 | controller add (0x777e4) + post cmd 2 "offer own FTAB" to remote 0x01 [0xbe6c6-0xbe6dc] |
| **0x18** | side()==1 -> accessory endpoint; side()==2 -> controller add + **cmd 2 (offer own FTAB)** to 0x18 [0xbe6a0-0xbe6c4] |
| **0x19** | side()==2 -> accessory endpoint; side()==1 -> controller add + **cmd 2** to 0x19 [0xbe67a-0xbe69e] |
| 0x1a, 0x1b | controller add + cmd 0x12 (offer stored "case" SuperBinary, tag CASE) to that id [0xbe65a-0xbe678] |
| 0x2d / 0x2e / 0x2f / 0x30 | controller add + cmd 0x18 STFW("ioe1") / 0x1b PDFW / 0x19 IOHM|IOHS / 0x1a ESM1 [0xbe5d2, 0xbe758, 0xbe72c, 0xbe742] |
| 0x03 | 0xbee88 -> 0x77ef0 (downstream add) [0xbe6de-0xbe72a] |
Also 0xbe8ba: data on 0x18/0x19 is fed to the controller (be8ac, role 2) or accessory (be834,
role 1) instance by the same side rule [0xbe8c6-0xbe90e].

=> (Known) 0x18 and 0x19 are a pair of mirrored channels: on 0x18 the side-2 bud is controller and
the side-1 bud accessory, on 0x19 the reverse. When such a channel comes up, its controller bud
offers its own FTAB (staged image if one is valid, else the running one). (Inferred, strong:
these are the bud-to-bud UARP channels; every link-up is an automatic sibling firmware sync
offer in both directions.)

## Transport-id sets used by the update logic (Known, rkof data)
- 0x1c4958 (FUN_000bf0c8, 6 ids): 01 02 10 11 18 19
- 0x1c4964 (2 ids, be420/bf0fc): 04 0d
- 0x1c4968 (FUN_000bf3ec, 4 ids): 01 02 18 19  -> "sibling sync" family: on asset release over
  one of these, cmd 8 (bea0c) is posted and bc0c4(1,0) [bccd0 path 0xbcdda-0xbcdf4]; an accepted
  offer from one of them calls bc020(0) [bd938 end]; the FTAB push (cmd 2) starts with bc020(1).
- The "endpoint type" compared in bd938/bc390 is the TRANSPORT ID: apply (0x0A) is honoured only
  from 0x10/0x11 [bc3a0]; bd938 treats 0x10/0x11 offers as host offers (may be not-newer),
  other transports' not-newer offers are denied with reason 1 [bd938: 1082e4 & 1 == 0 && type&~1 != 0x10].
- Accessory endpoints 4, 0xd, 0xe are created by FUN_000bea24 (not through the registry);
  offers from 4/0xd fill a status record (versions, state 2) [bd938, 0xbda22..]; Inferred: UARP
  tunnelled over AAP from an Apple host.

## The case (STFC; STFW has the same strings) (Known unless marked)
STFC is an ARM Cortex-M image loaded at **0x08001800** (2061 of 2134 strings resolve as pointers at
that base; a separate Ghidra project, imported with `tools/firmware/ghidra/import.sh` after setting the base address to 0x08001800). Addresses below `[STFC 0x0801....]`.
- FUN_0801d6cc [STFC 0x0801d6cc]: reads 4 versions from the two bud records (+0x4c,+0x4f / +0xc,+0xf,
  3 bytes each; Inferred: each bud's active and pending/staged version) via 0x0801e148, only when
  both buds are present (0x0801e19c: +0x54/+0x55 flags). If one bud's active equals the other's
  pending ("cross") or both pendings match and are non-zero ("pend") -> logs "Force apply for %s
  match" and sets flag [0x20034fc8+0x5d]=1. If both actives are equal -> apply-state handling
  (0x0801d628(1|7)), return 1. Otherwise -> "mismatch fw ver" path: 0x0801e05c(), log, posts event 0xc001.

### The case's bud-update decision, FUN_0803542c(L_act, L_pend, R_act, R_pend, mani bits, mani_required) [STFC 0x0803542c] (Known)
Return codes index the name table at STFC 0x0806d29c: 0 "R2L", 1 "L2R", 2 APPLY_L, 3 APPLY_R,
4 APPLY_BOTH, 5 APPLY_L_ERROR_R, 6 APPLY_R_ERROR_L, 7 ERROR_L, 8 ERROR_R, 9 ERROR_BOTH,
10 ERR_MANI, 0xb nothing. (Which record is L: Inferred from the APPLY_L case = the first pair, rec+0x4c.)
- L_act == R_act, no pendings -> 0xb (in sync).
- max(R) < max(L) and L's pending empty or >= L_act -> 1 "L2R" (10 ERR_MANI if a manifest is
  required [flag +0x57] and the bit for it is clear); mirror -> 0 "R2L".
- Otherwise, if max(act) <= max(pend): L_pend == R_pend -> 4 APPLY_BOTH; L_pend == R_act and
  R_pend < L_pend -> 2 APPLY_L (5 if R has some other pending); L_act == R_pend and L_pend < R_pend
  -> 3 APPLY_R (6); else 7/8/9/0xb errors.
- Actions [STFC 0x08037600-0x0803771a, reached from 0x08037044 case 0xc001 and 0x080373ec]:
  R2L/L2R -> FUN_08037878(dir): sets B2B-in-progress [+0x4b]=1, opens the bud tunnels
  (0x080379dc(dir,1,0), (dir,2,0); strings "B2B, tid:%s", "Bud server tid:%s %s", "Bud client tid:%s %s",
  "Open tid:%d sid:%d"); APPLY_* -> FUN_080641f8(side)/FUN_08037960(side,0): sends message 0xd0
  (0x1d bytes) to that bud via 0x0801c4ec (Inferred: the apply command; with the force flag +0x5d it
  carries 6 bytes of the other bud's record +6, Inferred its address).
- Trigger conditions, case event 0xc001 in FUN_08037044 (logged as "bud fwup cond:%d forced:%d,
  bud:%d,chrg:%d,lid:%d,tunnel:%d,caseOTA:%d" [str 0x08077982, lit 0x080373c4]):
  cond = both buds present with comparable versions (0x0801e19c: both present, first two version
  bytes equal) AND [+0xb]==1 AND not already in B2B [+0x4b]==0; unless forced [+0x5d], also
  AND lid flag [+0x4f] AND (0x0802aa08() > 10 OR [+0x44] != 0) (Inferred: battery > 10 % or
  charging); and when [+0x54] (tunnel) is set, AND NOT case-OTA (0x08035838). The mismatch check
  0x0801d6cc posts 0xc001 [STFC 0x0801d76x: 0x08062aaa(0xc001)].

## Downstream endpoints (0x17) and why we saw none (Known code, Inferred meaning of ids)
- UARPL3Downstream_addDownstreamAccessory = FUN_00077ef0 (strings at 0x77f1e..0x77ffe; announces with
  0x106f40 = msg 0x17). Its only callers: transport 0x03 coming up [rkof 0xbe70a] and the handler of
  inter-bud message 0x74 kind 2 [rkof 0xc0062] (handlers registered 0x15421a: 0x74 -> 0xbff65,
  0x75 -> 0xc011d, sent through the object at 0x200c3e50+0x30 [0xc6730/0xc6768]).
- 0x74 handshake: FUN_000bff00 sends 0x74 {1, address} only when this bud's current host UARP
  session (FUN_000bee88: tables 0x200c4f38+0x34 / 0x200c3174, which hold only transport 0xd / 4
  sessions) is on transport **0xd**, or when there is none and 0xc6214() holds [0xbff0e-0xbff42].
  The receiver of kind 1 creates accessory endpoint 0xe (its UARP reachable through relay msg 0x75)
  and answers kind 2 (loop cmd 0x15) [0xbff7a-0xbffa6, 0xbffbc-0xc0018]; the receiver of kind 2 adds
  the sender as **downstream id 2** with send function 0xc00d5 (msg 0x75), provided it has a 0xd/4
  host session [0xc001a-0xc0066]. The relay: 0x1b from host -> 0x78280 -> 0x75 to the sibling.
- So a host on GATT (transport 0x10) never triggers the handshake (bee88 finds no 0xd/4 session):
  0x15 gets 0x16 status 0 and no 0x17, as observed. (Inferred: 4/0xd are the AAP-tunnelled UARP
  sessions an Apple host opens [created by 0xbea24, called from 0x9ca4c]; the secondary is then
  exposed to that host as downstream 2.)

## Bud side of the case-driven update (Inferred links, Known pieces)
- The case opens a "Bud server" (type 1) on the source side and a "Bud client" (type 2) on the
  other [STFC 0x080379dc(side, 1|2, ...)], timeouts 5/10/15 s. The bud's transport 0x01 is the one
  where it becomes controller and pushes its FTAB, 0x02 the one where it is accessory (fixed roles,
  [rkof 0xbe6c6, 0xbe54c]); both are in the "sibling sync" set 0x1c4968. Inferred: case tid 1/2 =
  bud transports 0x01/0x02. 0x18/0x19 are the side-mirrored pair, same FTAB push; who opens them
  (case or the other bud, wirelessly) not established.
- Transport state is set from inbound control messages: [rkof 0x15bdd0-0x15be08] takes (id, state)
  from a message and calls 0x846d4 (which runs be4ec), inside the big dispatcher 0x158d6c whose
  handlers use case_sn/caseVol/caseCoup/"case session" (Inferred: the case<->bud protocol).
- Receiving bud: any non-host transport offer must be strictly newer than running (deny reason 1)
  [bd938], then the normal FTAB staging (digest TLV 0xCCD28107 from the sender's NVRAM sha_x, IM4M
  inside the FTAB). The pushed asset is tag FTAB with the ticket intact (bd760 zeroes ticket fields
  only for LTAB/RTAB).
- Apply on the receiving bud: 0x0A is honoured only from 0x10/0x11 [bc3a0], so a B2B-staged image is
  applied by another path: Inferred the case's message 0xd0 (29 bytes) -> the bud's commit 0xac350,
  e.g. [rkof 0x15a1c6] in the case dispatcher gated by 0xc58a0, next to a 29-byte length check
  [0x1595d0] and an id-0x7e handler that calls the pre-reboot save 0x86b1c [0x1595b8].

## Case firmware (Known strings/data, Inferred flow)
- Case payloads: STFC/STFW (case MCU, two hardware variants, version 90.390, rkosftab inside),
  NERF (case BLE chip, "Added App UARP Service", "VS UUID UARP Service addition index"),
  R1FW (UWB, the only payload marked Personalization Required). Each case payload carries
  0xCCD28107 (SHA-256) + 0xCCD28106 (256-byte signature) + 0xCCD28105 = 2; FTAB has no 0xCCD28106.
- The case stages them itself: "ST pend", "NRF pend", "R1 pend", "Apply R1 image, pend bank %d",
  "!! Upd nRF !! Req", "Apply BLE"; transports "uart|usb|tunnel", "Tunnel CLI: Kick USB/UART".
  Inferred: a host updates the case directly over the case's own BLE UARP service (NERF), not
  through a bud; caseOTA in progress blocks bud updates through the tunnel ("bud fwup cond" caseOTA).
- rkof can offer stored files "case" (tag CASE) to transports 0x1a/0x1b and STFW("ioe1")/IOHM/IOHS/
  ESM1/PDFW to 0x2d-0x30 [be4ec table, bf950], each only if the file exists; no writer of these
  files was found, so whether this model ever relays case firmware through a bud is not established.
- No bud-side check that the case is on a matching version was found.

## Bottom line: a host stages / applies FTAB on the primary only
Known:
- The primary stages FTAB into its inactive slot; its push routine (loop cmd 2) always offers the
  staged image when one is valid, else the running one, to the sibling over 0x01/0x18/0x19.
- The case compares both buds' active/pending versions and, when they differ, runs a B2B transfer
  newer -> older (L2R/R2L), then sends apply per bud (APPLY_L / APPLY_R / APPLY_BOTH) once a
  pending version matches the other bud's active or both pendings match.
- Gates for that (case event 0xc001): both buds in the case and reporting versions, not already in
  B2B, and unless forced: lid flag and (level > 10 or charging), and no case OTA when tunnelled.
Inferred:
1. Stage only (no 0x0A): with both buds in the case under those gates, the case sees
   L_pend=new, R_act=old -> L2R pushes the staged image to R -> both pending equal -> APPLY_BOTH:
   **the case can install a host-staged image on both buds without any apply from the host.**
   Staging is not inert.
2. Stage + 0x0A on the primary: the primary reboots into the new image alone (A/B, 3 tries).
   The pair is mismatched until both buds are in the case under the gates; then L2R from the
   running image, APPLY_R. No code was found that refuses audio/pairing on a mismatch; how a
   mismatched pair behaves outside the case is not established.
3. Unusable pair: no path found that bricks the secondary; every B2B step uses the same
   newer-only, digest-checked FTAB staging and A/B commit. A risk remains if the new image
   can't do B2B with the old one, or if a reboot during B2B goes wrong; not visible statically.
4. Case: not touched by a bud update; it updates itself from its own payloads (NERF/STFC/STFW/R1FW).
   R1FW needs a TSS ticket. No requirement that the case's version matches was found.
What a host should do to update the set like Apple does (Inferred): put both buds in the case on
the charger with the lid as the case wants, stage FTAB on the primary and let the case run
B2B + apply (or apply the primary and let the case catch up the other one). Or offer to each bud
separately. A host on transport 0xd (Apple AAP) also sees the sibling as downstream 2 via 0x74/0x75.
GATT hosts don't.
Not established: the physical link behind each transport id; who opens 0x18/0x19; whether the
lid flag means open or closed; the exact bud message for the case's apply (0xd0) and for
opening a tid; whether the manifest-required flag (+0x57, ERR_MANI) is set for this model.
