# Updating the pods' firmware: what a bud does

AirPods Pro 2 (USB-C), build 9A348 (hashes, tools: [firmware.md](firmware.md)).
Static analysis, `[image 0xADDR]`; Known = read from code or data, Inferred =
marked. The protocol, and the updates on real pods (Oct 27 over GATT,
installed by the case; Nov 14 over AACP, installed by the case: 9.1.47 running at 12:10): [uarp.md](uarp.md). The host's side: [update-host.md](update-host.md).

## Summary

- A host stages `FTAB` into the primary's **inactive** A/B slot; the
  running image is never written (§1, §3). An interrupted or bad transfer
  leaves nothing bootable (§3).
- The buds need no ticket: the package's `FTAB` carries an Apple-signed
  manifest valid for every pair of the model (§4).
- No apply is needed: with both buds in the case, the case copies the
  newer staged image to the other bud (`R2L`/`L2R`) and applies both
  (`APPLY_BOTH`), gated on both buds present, comparable versions, no copy
  running, a lid flag and enough battery or charging (Known code, triggers
  partly Inferred; [sibling-and-case.md](sibling-and-case.md)). A bud takes
  a sibling's image only if strictly newer, staged the usual way. Nothing
  found refuses audio or pairing on a version mismatch.
- The case's own payloads (`STFC`/`STFW` its MCU, `NERF` its BLE chip,
  `R1FW` its UWB chip, the `AU*` sounds) are never pulled by the buds; a
  buds-only update leaves the case as it was, and nothing found requires
  the two to match ([case.md](case.md)).
- Production pods refuse downgrades and the same build (§6).

## 1. A/B slots in rkof (Known)

- Two firmware files in the flash filesystem (FS object 0x200020c0): `firm.00`, `firm.01`
  [rkof 0x19ac8, 0xa5684, 0x1cf50]; the same names in the boot stage `rkos` (FTAB) with
  "Bad file %s (%x)", "Bad FTAB %s (%x)", "Digest check failed(%s)" [FTAB/rkos str 0x169ee–0x16a25].
- Boot-control register, object 0x20001bd0 word 0, via 0x18ad5c/0x18ad84 [rkof 0x1999c,
  0x199c8]: bit 3 = running slot (`FUN_00019938`; `FUN_00019946` the other), bit 2 read by
  `FUN_000199b8` (Inferred: trial boot after a switch), bits 0–1 written by
  `FUN_000199c8(slot)`: slot 0 → 1, slot 1 → 3 (Inferred: bit 0 switch request, bit 1 target).
- `FUN_000a5674`, "is something staged": opens the *inactive* slot's file, reads 0x30 bytes,
  accepts it only with the `rkosftab` magic at +0x20 and sane fields; returns that slot or 0xff
  [rkof 0xa5674–0xa5704]. Header only, no signature.
- `FUN_000ac350` (commit): slot = a5674(); if valid and not done yet → `199c8(slot)`, NVRAM
  `fwTries` += 1 (0xa6038/0xa6058), 0x78e3c(0), flag [0x200c1650+0x7e]=1
  [rkof 0xac350–0xac3ca]. Callers: 0xbe36a (UARP apply timer, §6), 0xc2ad2, 0xd69fe,
  0x154e60, 0x15a1c6.
- `FUN_000ac3ec` (give up): `19a38(other slot, 0)` (inactive slot invalid), `fwTries` = 0
  [rkof 0xac3ec–0xac40e].
- Boot-time decision in rkof init [rkof 0x154dd4–0x155026] (skipped when reg word 0x1c reads
  0xbacf0bac [0x154de4]):
  - bit 2 set (running the newly switched-to slot): version check (0xbfef4 vs bver at
    0x2004fd60+0x38); if OK → `19a38(current, 1)` (mark valid), the other invalid, `ac3ec`,
    status 4 [0x154ed4–0x15500c]. Inferred: the A/B confirm; the old slot is dropped only once
    the new one reached rkof init.
  - bit 2 clear: if the inactive slot holds a valid header and `fwTries` ≠ 0, the last attempt
    didn't stick: `fwTries` ≤ 2 → `ac350` again (status 1); > 2 → `bc164(0)`, `ac3ec` (staged
    image dropped, status 2) [0x154e16–0x155022]. `fwTries` = 0 → stays staged (status 0).
- So a failed new image gets 3 tries, then is dropped. Who reverts bit 3 after a crash of the
  new slot is in rkos/ROM (§9).

## 2. Manifest and digest storage (Known)

`FUN_000bd0ac(payload tag, TLV type, len, value)`, the payload metadata TLV handler for FTAB/LTAB/RTAB:
- `0x88B29130` (personalized IM4M): `a5b90()` = bud side (1/2). The *other* bud's table (side
  1 + RTAB, side 2 + LTAB): kept only in RAM ([0x200c1f90+0x28/+0x2c]) [0xbd15c–0xbd172]
  (Inferred: to relay). Otherwise a 0x88-byte header {16 B version from 0x200c3094+0x60, len,
  CRC16 of the blob (0x181d98), 8 B from 0x20054164+6, 32 B from 0x20054164+0xe, CRC16} is
  written with the blob to **`mani.00`** (truncated first) [0xbd174–0xbd288], read back,
  checked (CRCs, versions 0xbfe8c/0xbfef4), the result stored at [0x20054164+0x50]
  [0xbd28c–0xbd2fa]; logs "Personalized Manifest". No signature, ECID or nonce check; written
  on receipt. One `mani.00`, not per slot.
- `0xCCD28107` (32 bytes): for this bud's table, kept in RAM [0x200c1f90+0x18/0x1c] and
  written to NVRAM **`sha_1` if the running slot is 0, else `sha_0`** (0x1b894 set, 0x1b9b8
  commit) [0xbd200–0xbd220]: the expected digest of the image being staged. (Inferred: rkos'
  "Digest check failed(%s)" compares against it at boot.)

## 3. Staging FTAB: the header withheld until verified (Known)

- Offer filter `FUN_000bcfa8(slot, payload tag, version)` [rkof 0xbcfa8–0xbd088]: only
  **FTAB** (and HSML) is staged. FTAB not newer than running/staged (`bfec0`) and flag
  [0x200c16d0+0x48] clear → deny 2; else `bf414(slot, 2)` opens the staging file. Other tags
  (R1FW, STFC, LTAB/RTAB, …) go to the personalization builder `0x76860` [rkof 0x75f80–0x75f9a].
- `FUN_000bf414(…, 2)` [rkof 0xbf414–0xbf538]: with no staging in progress ([0x200c16d0+0x49]),
  takes the *inactive* slot's partition (table 0x1bd694), reformats it, opens its file for
  write, sets +0x49 = 1; a later call resumes the same file. The running slot is never opened
  for writing.
- Data handler `FUN_000bc404` [rkof 0xbc404–0xbc716]: at offset 0 the first 0x30 bytes (the
  `rkosftab` header) are kept in RAM (0x200c54d0) and **0xFF×0x30 written instead**
  [0xbc4ca–0xbc4da]; every chunk feeds a SHA-256 [0xbc4a4]; the header's ticket range (+0x10
  offset, +0x14 length, the embedded IM4M) is copied to RAM [0x200c1f90+0x28/+0x2c].
- Completion `FUN_000bc978` [rkof 0xbc9de–0xbcc4e]: fails without the expected digest
  (`0xCCD28107`), on a SHA-256 mismatch, or without the IM4M; `0xd270` (Img4 decode) and
  `0xd224(…, 'bver', 'DGST')` must succeed; only then is the header written at offset 0
  [0xbcc26–0xbcc38]. Then if the staged bver isn't newer (`bfef4`) the slot is marked invalid
  [0xbcaa8–0xbcabc]; +0x49 = 0; logs "successful"/"fail".
- So an interrupted, rescinded, corrupt or mismatching transfer leaves the slot without a valid
  header: nothing staged, nothing boots it.
- rkof checks of the IM4M only that it decodes and has a bver DGST; no ECID, nonce or
  signature check found (a signature check would be inside 0xd270, not traced). The boot stage
  checks each image's SHA-384 against the manifest: FTAB/rkos `FUN_00004924`, panics "Digest
  check failed(%s)" [FTAB/rkos 0x4924–0x4990].

## 4. No ticket for the buds (Known data, Inferred conclusion)

- `FTAB`'s IM4M (ticket range: header +0x10 = 0x771e72, +0x14 = 0xd7f) is signed by
  `T2016-SDOM1-TssLive-ManifestKey-RevE-DataCenter` (under Apple Secure Boot Root CA - G3).
  Its `MANP`: `BORD 6`, `CEPO 1`, `CHIP 0x2016`, `CPRO true`, `CSEC true`, `SDOM 1`, `augs 1`,
  a 20-byte `srvn`: **no `ECID`, no nonce (`BNCH`)** (`ECID` appears only in the certificate's
  allowed properties). Each image (`acib aopf bver lpda phyb rkof rkos rtpf sbd1 sbd2`) has
  `DGST` (SHA-384), `EKEY`, `EPRO`, `ESEC`. So one Apple-signed manifest covers every pair
  and ships in the package; a host sends the file as it is, no call to Apple's signing server.
- SuperBinary metadata: FTAB has only `0xCCD28107` (SHA-256 57b3289f…22a3) and `0xCCD28112` =
  1, no "Personalization Required". CTAB carries the same digest, `0xCCD28119` = 0x773000
  (FTAB's size) and a 16 KiB chunk table `0xCCD28113` (Inferred: a compressed FTAB). Only
  **R1FW** has `0x88B29100` = 1 (Personalization Required), `0x88B29103` = "Rap" and two
  `0x88B29110` measurements (rkos/"RTKitOS", sbd1/"SoftwareBinaryDsp1", SHA-384).
- R1FW is the case's UWB firmware (uwb_power.c, uwbsec.c, rmac.c, AoA_lookup_tbl.c; bver
  "chip_revision_c0"); "Rap" is its TSS key prefix. Only it needs a live ticket.
- Observed: the Oct 27 and Nov 14 transfers staged with no personalization request; the
  primary pulls the SuperBinary header, the payload headers and `FTAB`'s metadata and data,
  nothing of the other payloads ([uarp.md](uarp.md)).

## 5. Identity values: Bulk Info Query 0x23/0x25 (Known)

`FUN_000771cc` (builds the 0x25 response for a stored 0x23 query, by context 4cc) maps each
TLV type to a selector of `FUN_000bdf50` [rkof 0x77264–0x7731a, tables 0x7726e/0x77294;
getter 0xbdfa0, cases 0xbdfce–0xbe1ae]:

| TLV type | value | source |
|---|---|---|
| 0x88B29104 | u32 | register 0x20001bd0 reg 0x5c, 0xfe258(0x1f30) >> 16 (Inferred: BoardID) |
| 0x88B29105 | u32 | `FUN_00019208`: 0x2016 for raw 0x5201/0x5702: **ChipID** |
| 0x88B29106 | 8 B | **ECID**: own (object 0x200221e0 vt+0x2c), or for the other bud's table the peer's stored ECID [0x20054164+6] [0xbdf60–0xbdf94, 0xbe166] |
| 0x88B29107, 0x88B29108 | 32 B | **constant** bytes at rkof 0x1c39ec (`ca4a9d7c…8a7e`), the same for every unit (Inferred: the nonce slot) |
| 0x88B29109 | u32 | `FUN_00018fc0`: fuse words +0x6058/+0x605c ≠ 0xa050c030 (bits 0/1) (Inferred: security mode) |
| 0x88B2910A | u32 | fuse +0x6078 ≠ 0xa050c030 (Inferred) |
| 0x88B2910B | u32 | `FUN_00018f2c`: NVRAM `is_prod_override`: **ProductionMode** |
| 0x88B2910C | u32 | fuse +0x6080 bits 3..9 (Inferred: security domain / revision) |
| 0x88B2910F | u32 | object 0x200221e0 vt+0x10 |
| 0x88B29118 | — | empty |
| 0x88B29137 | 8 B | test constant 0x12345678 0x98765432 |
| 1, 2, 3, 4, 6, 9, 0xa, 0xb | strings/versions | manufacturer, model, serial, HW version, version (16 B), name, "prod", +0xd4 string |

- Vendor properties 1/2 (msg 0xFFFF) are `bc8b0` selectors 0xb (a string, 0x15efb0) and 0xc
  (the production flag) [rkof 0xbc8b0–0xbc95c].
- The personalization asset ([uarp.md](uarp.md) §5) only names these TLV types; a host reads
  the values with 0x23, context = the payload tag. It is requested only when the payload's
  `0x88B29100` ≠ 0 [rkof 0x7605a–0x7606a, 0x7686c–0x76872]; else skipped (0x76bd4).
- No per-update nonce on the bud side (the 32-byte fields are build constants), so a bud
  ticket wouldn't be session-bound (Inferred). The case has nonces: STFC/STFW "Boot nonce: %s",
  "Apply R1 image, pend bank %d", "Add IM4M to hdr"; NERF "BOOT_NONCE" (strings only).

## 6. Apply and the switch (Known unless marked)

Calvados never sends 0x0A; this is for completeness.
- 0x0A → `FUN_000bc390` [rkof 0xbc390–0xbc3d8]: from transport 0x10/0x11 only (else flags 6).
  If staged > active (`bfd5c`, `1082e4`) → `FUN_00086b1c` (NVRAM writes: chimeVol, pse_var,
  hpVar3, fprox, pme_var, aaStren, ysmode, …; Inferred: pre-reboot save) [0x86b1c–0x86ce6];
  reply flags 3; timer 0x62, 1000 ms.
- Timer 0x62 → `FUN_000be360` [registered 0x154140–0x154144] → `ac350`: boot request for a
  valid staged slot, `fwTries`++, `0x78e3c(0)` [0xbe360–0xbe36a]. Inferred: a reset follows.
- Commits without 0x0A: 0xc2ad2 (a 24-byte compare, then commit), 0xd69fa (a callback),
  0x15a1c6 (a command dispatcher gated by 0xc58a0; Inferred the case's apply,
  [sibling-and-case.md](sibling-and-case.md)).
- Version gate: an offer not newer than running is denied (2) unless SuperBinary TLV
  0x0381d2cc ("Ignore Version") sets [0x200c16d0+0x48], honoured only when `0x18f2c()`
  (production) is 0 [rkof 0xbd70e–0xbd726]. Production pods: no downgrade, no reinstall.

## 7. The other bud as an asset source (Known code, partly Inferred)

The copy itself: [sibling-and-case.md](sibling-and-case.md). Bud-side pieces:
- `FUN_000bd760` serves FTAB/LTAB/RTAB from the slot file (`bf414(…,1)`: the staged slot if
  valid, else the running one); for LTAB/RTAB at offset 0 it zeroes header +8..+0x17 (ticket
  offset/length) [rkof 0xbd780–0xbd7e8]. With bd0ac keeping the other bud's manifest and bdf50
  answering the other bud's ECID: Inferred, a relay where the secondary gets the image without
  a ticket plus its own manifest. A secondary staging LTAB/RTAB then needs a 0x88B29130
  manifest TLV, else bc978 fails (§3). The FTAB push between buds keeps the ticket.
- "UTP"/"Request Swap"/"Apply Grants" HSM strings [rkof 0x1e012e–0x1e023e] may be bud-to-bud
  coordination (not traced).

## 8. Safe and unsafe steps (Inferred from the above)

| Step | Effect on flash | Risk |
|---|---|---|
| Sync, version, info tags, Bulk Info Query | none | none |
| Offer (0x05) then rescind (0x0C) or deny | none, or opens staging (erases the inactive slot) | low: running slot untouched |
| Transfer FTAB, then abandon/disconnect | inactive slot erased and partly written, header 0xFF | low: not bootable, not staged; cleanup `bccd0` → `ac3ec` [0xbce78–0xbce90] |
| Full transfer, digest OK, never apply | inactive slot valid, `sha_x` written | low: stays staged (fwTries 0) until the case copies and applies it (observed), or another commit path |
| Metadata TLV 0x88B29130 | rewrites `mani.00` at once, unvalidated | low-medium: read by relay/queries only (consumer 0x15a93a) |
| 0x0A Apply (newer staged) | NVRAM writes, boot-slot request, reset | the irreversible step; A/B + 3 tries in rkof; ROM side unseen |
| Downgrade | refused on production | — |

## 9. Not established

- The boot ROM: how register bits 0/1/3 are consumed, whether a new slot that panics before
  rkof init falls back by itself (rkof's retries imply a one-shot request, Inferred), and
  whether a global (non-ECID) IM4M is accepted on production fuses (it was in practice: the
  Oct 27 update booted).
- Whether 0xd270 verifies the IM4M's signature and chain.
- Who writes [0x20054164] (peer ECID/32 B).
