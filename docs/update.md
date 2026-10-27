# Updating the pods' firmware: what the pods do

AirPods Pro 2 (USB-C), build 9A348 (hashes, tools: [firmware.md](firmware.md);
the protocol: [uarp.md](uarp.md)). Static analysis, `[image 0xADDR]`;
Known = read from code or data, Inferred = marked. Nothing here has been
tried on real pods beyond the read-only probe.

Done once, Oct 27: a pair on 7.1.21 updated to 9.0.348 from Linux over
GATT, the primary staging `FTAB` and the case finishing the other bud and
applying both (details in [uarp.md](uarp.md)).

## The buds' manifest needs no personalization (Known, checked)

The IM4M at the end of the `FTAB` payload (offset `0x771e72`) is signed
by `T2016-SDOM1-TssLive-ManifestKey-RevE-DataCenter` (under Apple Secure
Boot Root CA - G3) and its `MANP` holds only `BORD 6`, `CEPO 1`,
`CHIP 0x2016`, `CPRO true`, `CSEC true`, `SDOM 1`, `augs 1` and a 20-byte
`srvn`: no `ECID`, no nonce (`BNCH`). Each image (`acib aopf bver lpda
phyb rkof rkos rtpf sbd1 sbd2`) has `DGST` (SHA-384), `EKEY`, `EPRO`,
`ESEC`. So one Apple-signed manifest covers every pair of this model and
ships in the package: a host delivers the file as it is, with no call to
Apple's signing server. (`ECID` appears only in the certificate's list of
allowed properties.) Only `R1FW`, the case's UWB firmware, is marked
"Personalization Required".

## The other bud and the case (summary; details in [sibling-and-case.md](sibling-and-case.md))

- Staging on the primary is not inert (Known code, trigger conditions
  partly Inferred): with both buds in the case, the case compares the buds'
  running and staged versions and copies the newer image to the other bud
  (`R2L`/`L2R`, through the buds), then applies (`APPLY_BOTH`), without a
  host sending Apply. It acts only with both buds present, comparable
  versions and no copy running, and needs a lid flag and enough battery or
  charging.
- A bud accepts a sibling's image only if strictly newer, and stages it the
  usual way (digest-checked, A/B). Nothing found refuses audio or pairing
  on a version mismatch.
- The case's own firmware (`STFC`/`STFW` its MCU, `NERF` its BLE chip,
  `R1FW` the UWB chip, the `AU*` sounds of its speaker) comes in the same
  package but is the case's own UARP accessory's business: the buds never
  pull or keep those payloads. The case takes an offer as downstream 1
  through a bud for an Apple host's AAP session (not over GATT, as
  observed) or over its own BLE UARP service, which needs an encrypted
  link and refuses new pairings once it belongs to a Find My owner;
  `R1FW` needs a live ticket. A buds-only update leaves the case on its old
  firmware; nothing found requires the two to match. Details:
  [case.md](case.md).
- A GATT host gets no downstream endpoints: they're added only when the
  host session is on transport 0xd (Inferred: an Apple host's AAP channel).

## 1. A/B slots in rkof (Known)

- Two firmware files in the flash filesystem (FS object 0x200020c0): `firm.00`, `firm.01`
  [rkof 0x19ac8, 0xa5684, 0x1cf50]; the same names in the boot stage `rkos` (FTAB) with
  "Bad file %s (%x)", "Bad FTAB %s (%x)", "Digest check failed(%s)" [FTAB/rkos str 0x169ee–0x16a25].
- Boot-control register read/written through 0x18ad5c/0x18ad84 on object 0x20001bd0, word 0
  [rkof 0x1999c, 0x199c8]: bit 3 = slot currently running (`FUN_00019938`; `FUN_00019946` = the
  other one), bit 2 = read by `FUN_000199b8` (Inferred: "booted via a pending switch", i.e. trial
  boot), bits 0–1 written by `FUN_000199c8(slot)`: slot 0 → 1, slot 1 → 3 (Inferred: bit 0 = switch
  request, bit 1 = target slot).
- `FUN_000a5674`: opens the *inactive* slot's file, reads 0x30 bytes, accepts it only if it has the
  `rkosftab` magic at +0x20 and sane fields; returns that slot or 0xff [rkof 0xa5674–0xa5704]. This is
  the "is something staged" test; header only, no signature.
- `FUN_000ac350` (commit): slot = a5674(); if valid and not done yet → `199c8(slot)` (request boot
  of the staged slot), NVRAM `fwTries` += 1 (0xa6038/0xa6058, var name 0x1c3302), 0x78e3c(0), flag
  [0x200c1650+0x7e]=1 [rkof 0xac350–0xac3ca]. Called from 0xbe36a (UARP glue), 0xc2ad2, 0xd69fe,
  0x154e60, 0x15a1c6.
- `FUN_000ac3ec` (give up / clean): `19a38(other slot, 0)` (mark the inactive slot not valid),
  `fwTries` = 0 [rkof 0xac3ec–0xac40e].
- Boot-time decision in rkof init [rkof 0x154dd4–0x155026] (skipped entirely when reg word 0x1c
  reads 0xbacf0bac [0x154de4]):
  - bit 2 set (running the newly switched-to slot): version check (0xbfef4 vs bver data at
    0x2004fd60+0x38); if OK → `19a38(current, 1)` = mark current slot valid; then mark the other
    invalid, `ac3ec`, status 4 [0x154ed4–0x15500c]. (Inferred: this is the "confirm" of an A/B
    update; the old slot is dropped only after the new one booted far enough to run rkof init.)
  - bit 2 clear (normal boot): if the inactive slot holds a valid header and `fwTries` ≠ 0, the
    previous attempt did not stick: `fwTries` ≤ 2 → `ac350` again (retry, status 1); > 2 →
    `bc164(0)`, `ac3ec` (abandon staged image, status 2) [0x154e16–0x155022]. `fwTries` = 0 → leave
    it staged (status 0).
- So: staging writes the *inactive* slot; the running image is not overwritten; a failed new image
  gets 3 tries then is dropped. (Known for the rkof side; how the boot stage reacts to a crash of
  the new slot, i.e. who reverts bit 3 to the old slot, is in rkos/ROM — see §2.)

## 2. Manifest / digest storage (Known)

`FUN_000bd0ac(payload tag, TLV type, len, value)` — payload metadata TLV handler for FTAB/LTAB/RTAB:
- `0x88B29130` (personalized IM4M): `a5b90()` = bud side ([0x200c1da0+0x34]+0x41; 1/2). If tag is the
  *other* bud's table (side 1 + RTAB, side 2 + LTAB) the blob is only kept in RAM
  ([0x200c1f90+0x28/+0x2c]) [0xbd15c–0xbd172] (Inferred: to relay to the other bud). Otherwise:
  builds a 0x88-byte header {16 B version from 0x200c3094+0x60, len, CRC16 of blob (0x181d98),
  8 B from 0x20054164+6, 32 B from 0x20054164+0xe, CRC16 of header} [0xbd174–0xbd1cc], opens
  file **`mani.00`** for write (mode 2), truncates, writes header + blob, closes [0xbd1d0–0xbd288];
  then reopens and reads it back, checks header CRC and blob, compares versions (0xbfe8c/0xbfef4)
  and stores the result at [0x20054164+0x50] [0xbd28c–0xbd2fa]; logs "Personalized Manifest".
  No signature, ECID or nonce check in this function; the write happens on receipt.
  Only ONE `mani.00` (not per slot).
- `0xCCD28107` (32 bytes): for this bud's table, kept in RAM [0x200c1f90+0x18/0x1c] and written to
  NVRAM var **`sha_1` if the running slot is 0, else `sha_0`** (0x1b894 set, 0x1b9b8 commit)
  [0xbd200–0xbd220]: the expected digest of the image being staged into the inactive slot.
  (Inferred: rkos "Digest check failed(%s)" compares against it at boot.)

## 3. Staging an FTAB: header withheld until verified (Known)

- Offer filter `FUN_000bcfa8(slot, payload tag, version)` [rkof 0xbcfa8–0xbd088]: only payload tag
  **FTAB** (and HSML) is staged. For FTAB: offered version vs `bfec0` (running/staged); not newer
  and flag [0x200c16d0+0x48] clear → deny 2; otherwise `bf414(slot, 2)` opens the staging file.
  Other tags (R1FW, STFC, LTAB/RTAB, …) return nonzero and go to the personalization builder
  `0x76860` instead [rkof 0x75f80–0x75f9a].
- `FUN_000bf414(…, 2)` [rkof 0xbf414–0xbf538]: when no staging is in progress ([0x200c16d0+0x49]),
  takes the *inactive* slot's partition (table 0x1bd694[other slot]), reformats it (partition
  vtable[0](obj, 2)), opens its file for write and sets +0x49 = 1; a later call resumes the same
  file. The running slot is never opened for writing.
- Data handler `FUN_000bc404` [rkof 0xbc404–0xbc716]: at offset 0 of an FTAB, the first 0x30 bytes
  (the `rkosftab` header) are saved in RAM (0x200c54d0) and **0xFF×0x30 is written instead**
  [0xbc4ca–0xbc4da]; every chunk feeds a SHA-256 [0xbc4a4]; the bytes in the header's ticket
  range (+0x10 offset, +0x14 length) are copied to RAM [0x200c1f90+0x28/+0x2c] (the FTAB's
  embedded IM4M).
- Completion `FUN_000bc978` [rkof 0xbc9de–0xbcc4e]: requires the expected digest (metadata TLV
  `0xCCD28107`, 32 B) — missing → fail; computed SHA-256 ≠ expected → fail; no IM4M in RAM → fail;
  `0xd270` (Img4 decode of that IM4M) and `0xd224(…, 'bver', 'DGST')` must succeed; only then is
  the saved 0x30-byte header written at offset 0 [0xbcc26–0xbcc38]. Then version check of the
  staged bver (`bfef4`); if not newer the inactive slot is marked invalid (`19a38(other, 0)`)
  [0xbcaa8–0xbcabc]; +0x49 = 0; logs "successful"/"fail".
- So an interrupted, rescinded, corrupt or mismatching transfer leaves the inactive slot without
  a valid header: `a5674` returns 0xff, nothing is "staged", nothing boots it. (Known for rkof.)
- What rkof checks of the IM4M: it decodes and has a bver DGST. No ECID/nonce/signature check was
  found in this path (signature check, if any, is inside 0xd270 — not traced). The boot stage checks
  each image's SHA-384 against the manifest: FTAB/rkos `FUN_00004924` hashes, compares 48 bytes,
  panics "Digest check failed(%s)" [FTAB/rkos 0x4924–0x4990].

## 4. The stock package needs no ticket for the buds (Known data, Inferred conclusion)

- The FTAB payload already carries an IM4M at its ticket range (FTAB header +0x10 = 0x771e72,
  +0x14 = 0xd7f): MANP = {BORD 6, CEPO 1, CHIP 0x2016, CPRO true, CSEC true, SDOM 1, augs 1, srvn},
  **no ECID, no BNCH/nonce** — a global (non-personalized) ticket signed by Apple (cert chain to
  "Apple Secure Boot Root CA - G3"), with SHA-384 DGSTs of acib aopf bver lpda phyb rkof rkos rtpf
  sbd1 sbd2. (parsed from FTAB/payload.bin)
- SuperBinary metadata (firmware.uarp, parsed): FTAB has only `0xCCD28107` (SHA-256 =
  57b3289f…22a3) and `0xCCD28112` = 1 — **no** "Personalization Required". CTAB carries the same
  digest + `0xCCD28119` = 0x773000 (= FTAB size) + 16 KiB chunk table `0xCCD28113` (Inferred: a
  compressed FTAB). Only **R1FW** has `0x88B29100` = 1 (Personalization Required), `0x88B29103` =
  "Rap", and two `0x88B29110` measurements (rkos/"RTKitOS", sbd1/"SoftwareBinaryDsp1", SHA-384).
- R1FW is the UWB chip's firmware (strings uwb_power.c, uwbsec.c, rmac.c, AoA_lookup_tbl.c; bver
  "chip_revision_c0"); "Rap" is the TSS key prefix (Inferred: Rose = U1, in the case). So only the
  case's UWB firmware needs a live TSS ticket; the buds' FTAB stages with the shipped global IM4M.
  (Inferred; the boot ROM's acceptance of a global ticket on production pods is not visible here.)

## 5. Identity values: Bulk Info Query (0x23/0x25), not the vendor properties (Known)

`FUN_000771cc` (builds the 0x25 Bulk Info Response for a stored 0x23 query, by context 4cc)
maps each requested TLV type to a selector of `FUN_000bdf50` [rkof 0x77264–0x7731a, TBB tables
0x7726e/0x77294; getter TBH 0xbdfa0, cases 0xbdfce–0xbe1ae]:

| TLV type | value | source |
|---|---|---|
| 0x88B29104 | u32 | register read (0x20001bd0 reg 0x5c, 0xfe258(0x1f30) >> 16) — Inferred BoardID |
| 0x88B29105 | u32 | `FUN_00019208`: 0x2016 for raw 0x5201/0x5702 — **ChipID** |
| 0x88B29106 | 8 B | **ECID**: own via object 0x200221e0 vt+0x2c; if the context is the *other* bud's table (side 1 → LTAB, side 2 → RTAB) the peer's stored ECID [0x20054164+6] [0xbdf60–0xbdf94, 0xbe166] |
| 0x88B29107, 0x88B29108 | 32 B | **constant** bytes at rkof 0x1c39ec (`ca4a9d7c…8a7e`) — same for every unit of this build (Inferred: nonce slot; not random) |
| 0x88B29109 | u32 | `FUN_00018fc0`: fuse words +0x6058/+0x605c ≠ 0xa050c030 (bits 0/1) — Inferred security mode |
| 0x88B2910A | u32 | fuse +0x6078 ≠ 0xa050c030 — Inferred |
| 0x88B2910B | u32 | `FUN_00018f2c`: production flag (NVRAM `is_prod_override`) — **ProductionMode** |
| 0x88B2910C | u32 | fuse +0x6080 bits 3..9 — Inferred security domain / revision |
| 0x88B2910F | u32 | object 0x200221e0 vt+0x10 |
| 0x88B29118 | — | empty |
| 0x88B29137 | 8 B | test constant 0x12345678 0x98765432 |
| 1, 2, 3, 4, 6, 9, 0xa, 0xb | strings/versions | manufacturer, model, serial, HW version, version (16 B), name, "prod", +0xd4 string |

The vendor properties 1/2 (0xFFFF msg) are `bc8b0` selectors 0xb (a string, 0x15efb0) and 0xc
(`0x18f2c`, the production flag) [rkof 0xbc8b0–0xbc95c]. The personalization request asset
(0x76860) only lists these TLV types (values 0/garbage, docs/uarp.md §5): it is a template; a host
reads the values with 0x23 Bulk Info Query, context = the payload tag.
- No per-update nonce exists on the bud side: the 32-byte fields are build constants (Known),
  so a bud ticket (if any) would not be session-bound (Inferred). The case is different: STFC/STFW
  strings "Boot nonce: %s", "Apply R1 image, pend bank %d", "Add IM4M to hdr"; NERF
  "BOOT_NONCE", "boot nonce hash generation …" (strings only).
- Personalization is requested only when the payload metadata has `0x88B29100` ≠ 0
  (ctx+0x29e) [rkof 0x7605a–0x7606a, 0x7686c–0x76872]; with 0 the builder is skipped (0x76bd4).

## 6. Apply and the switch (Known unless marked)

- 0x0A Apply → `FUN_000bc390` [rkof 0xbc390–0xbc3d8]: endpoint type 0x10/0x11 only (else flags 6).
  If staged > active (`bfd5c` = {active from bver, staged from 0xa6020}, `1082e4`) → `FUN_00086b1c`
  (writes many NVRAM vars: chimeVol, pse_var, hpVar3, fprox, pme_var, aaStren, ysmode, …; Inferred:
  pre-reboot state save, not traced fully) [0x86b1c–0x86ce6]; reply flags 3; timer 0x62, 1000 ms.
- Timer 0x62 → `FUN_000be360` (registered [0x154140–0x154144]) → `ac350`: if the inactive slot has
  a valid header → boot request for it + `fwTries`++ → `0x78e3c(0)` [0xbe360–0xbe36a].
  Inferred: reset follows (not traced past 0x78e3c / boot ROM).
- Other callers of `ac350` (commit without a host 0x0A): 0xc2ad2 (24-byte compare, then commit),
  0xd69fa (a callback), 0x15a1c6 (a command dispatcher, gated by 0xc58a0) — triggers not traced.
- Version gate: an offer not newer than running is denied (2) unless SuperBinary TLV 0x0381d2cc
  ("Ignore Version") sets [0x200c16d0+0x48], which is only honoured when `0x18f2c()` (production)
  is 0 [rkof 0xbd70e–0xbd726]. Production pods: no downgrade, no reinstall of the same build.
- New-slot confirm and retries: §1 (3 tries, then the staged image is dropped).

## 7. Other bud and case (Known code, partly Inferred)

- The bud serves its staged/running image as an asset source: `FUN_000bd760` reads FTAB/LTAB/RTAB
  from the slot file (`bf414(…,1)`: staged slot if valid, else the running one) and, for LTAB/RTAB at
  offset 0, zeroes header +8..+0x17 (ticket offset/length) [rkof 0xbd780–0xbd7e8]. With `bd0ac`
  keeping the *other* bud's manifest in `mani.00` (with the peer's ECID/32 B in the record) and
  `bdf50` answering the other bud's ECID for its table, this is a primary→secondary relay where the
  secondary gets the image without ticket plus its own manifest (Inferred). Which transport/role id
  carries it, and who starts it, not established; "UTP"/"Request Swap"/"Apply Grants" HSM strings
  [rkof 0x1e012e–0x1e023e] may be the coordination between buds (not traced).
- A secondary staging an LTAB/RTAB needs a manifest TLV 0x88B29130 (own side → RAM) since the
  ticket range is zeroed; without it `bc978` fails (Known from §3) — so the host (or primary) must
  supply one per bud (Inferred: the global IM4M would pass rkof's checks).
- The case runs its own UARP stack (STFC/STFW = case MCU images, "Asset <%d> … Personalization
  Required", "APPLY_L/APPLY_R/APPLY_BOTH", "bud fwup cond:%d forced:%d,bud:%d,chrg:%d,lid:%d,tunnel:%d,
  caseOTA:%d"; NERF = case BLE chip with "Added App UARP Service"); R1FW = case UWB (A/B "bank").
  The bud's downstream/controller role (docs/uarp.md) is the path to it; mismatch handling between
  buds/case not established.

## 8. Safe / unsafe steps (Inferred from the above)

| Step | Effect on flash | Risk |
|---|---|---|
| Sync, version, info tags, Bulk Info Query | none | none |
| Offer (0x05) then rescind (0x0C) or deny | none, or opens staging (erases inactive slot) | low: running slot untouched |
| Transfer FTAB, then abandon/disconnect | inactive slot erased + partly written, header 0xFF | low: not bootable, not "staged"; cleanup path `bccd0` → `ac3ec` [0xbce78–0xbce90] |
| Full transfer, digest OK, never apply | inactive slot valid, `sha_x` NVRAM written | low: stays staged with fwTries 0; but other callers can commit it later |
| Metadata TLV 0x88B29130 on FTAB/other side | rewrites `mani.00` immediately, no validation | low-medium: manifest store for relay/queries only (consumers: 0x15a93a) |
| 0x0A Apply (newer staged) | NVRAM writes (86b1c), boot-slot request, reset | the irreversible step; A/B + 3-try fallback in rkof; ROM side unseen |
| Downgrade | refused on production | — |

## 9. Not established

- Boot ROM behaviour: how bits 0/1/3 of the boot register are consumed, whether a new slot that
  panics before rkof init falls back to the old slot by itself (rkof's retry logic implies the old
  slot runs again after a failed attempt, i.e. the request is one-shot — Inferred), and whether a
  global (non-ECID) IM4M is accepted on production fuses (it is what Apple ships in the FTAB).
- Whether 0xd270 verifies the IM4M signature/certificate chain.
- Who writes [0x20054164] (peer ECID/32 B); what triggers the bud-to-bud relay; R1FW/case flow.
- What the bud does with non-FTAB payloads it cannot stage (pulled? skipped?) when the stock
  SuperBinary (with R1FW "Personalization Required") is offered to a bud.
