# Ownership and audio routing in the AirPods firmware

How the pods arbitrate between hosts: AirPods Pro 2 (USB-C), build 9A348,
image `rkof` (hashes and tools: [firmware.md](firmware.md)). Static analysis,
every claim cited `[rkof 0xADDR]`; Known = read from code, Inferred =
marked. Most of `rkof` is built with LLVM's machine outliner: many `bl` into
0x1a…–0x1b9… are 2–6 instruction fragments that use the caller's registers
and stack, so Ghidra's decompiler output is unreliable here; read the
disassembly with those fragments inlined.

## AACP receive layer is a pure parser [Known]

- The AACP opcode dispatcher FUN_0006ceb0 (TBH at 0x6cf48) parses each frame
  and calls a callback in a table `*0x2004cf6c` [rkof 0x6d1a4]; the table is
  0x2005ec34 [rkof 0x8af88..0x8af8e], filled by FUN_000cb170 [rkof 0xcb170].
- Opcode 0x09 → callback +0x20 = 0xcb4fc [rkof 0xcb292, lit 0xcb388];
  opcode 0x10 → +0x30 = 0xcb5e8 [rkof 0xcb282].

## 0x10 smart routing: relayed verbatim [Known]

0xcb5e8(session, target mac, len, data) [rkof 0xcb5e8-0xcb672]:
- any arg null → error 0x65; target mac not a connected AACP session
  (0x6ca30 lookup) → 0x7a; global flag `[0x200c41ec+0x1c]` set → 0x76
  (nothing sent); else builds 0x11: opcode, the *sender's* mac (fragment
  0x1ad4fc writes bytes reversed into +2..+7), len LE at +8/+9, data memcpy
  at +10, sent to the target with 0x6e55c, then logs (0xc8110 id 0x44766f12).
- No parsing of the payload (no reason / ownership keys). The pods do not act
  on smart-routing content at all; it is host-to-host.
- Same flag `[0x200c41ec+0x1c]` also blocks the 0x0E sender [rkof 0x87616].

## 0x09 control → message to the ownership arbiter [Known]

- 0xcb4fc builds {mac(6) of session, id, value} and posts it to handler
  0x879c0 (task context) [rkof 0xcb4fc-0xcb51e].
- 0x879c0: `TBH` on id-1 at 0x879ec (table 0x879f0). Ids 6, 0x20, 0x36 →
  0x87aea: forwards a broker message, class 0x4161, type 0x21, {mac, id,
  value}, through `[[0x200c33a8+0x30]]->vtable[0]` to destination
  `[0x200c1c2c+0x24]` (if that is 0: abort "Spicy_LogAbort") [rkof 0x87aea-0x87b1c].
- Receiver: the "arbiter" component, object 0x200b9cb8, vtable 0x1c1e58,
  constructed at [rkof 0x1608b4-0x16095a] (subscribes classes 'CT' 0x4354,
  'AS' 0x4153, 'TV' 0x5654, 'Aa' 0x4161, 0x5352). Its message entry
  FUN_00081b88: class 0x4161 type 0x21 id 6 → event 0x104 (value!=0) or 0x105
  (value==0), with mac and bool [rkof 0x81bd4-0x81c06]. Other ids of that
  class are dropped here.

## The ownership arbiter (object 0x200b9cb8) [Known unless marked]

State machine framework: active objects (active_object.cpp) running a
table-driven HSM. Engine FUN_00072982 [rkof 0x72982]: descriptor
D = {+0 states (8 B: u16 first transition, +4 parent, +5 initial child,
+6 depth, +7 count), +4 transitions (3 B: [action, event, target]),
+8 action function, +0xc guards (fn, u16 transition), +0x13 guard count};
event id = low byte of the event's u16 kind at +4 [rkof 0x74168].

Arbiter descriptor 0x1c5a9c [rkof 0x1608d2]: one state, 13 transitions
`[n, n, ff]` for n = 0..0xc [bytes at 0x1c5a74] — every event 0x1nn runs
action n with no state change. Actions: FUN_000d85e8, TBH at 0xd85fa.

It keeps a list of per-host records at +0x50 (record: +8 mac, +0xe flags,
+0xf "current owner") [rkof 0x81ae0, 0xd8618]. Re-evaluation FUN_00081e44
[rkof 0x81e44-0x81ecc]: the record with the numerically largest
`flags & 0x7f` wins (ties: the earlier record in the list); it is marked
current; if it differs from the previous owner, event 0x109 {new mac, old mac
(zero if none), new flags}; empty list → event 0x10c.

| Event | Source | Action [rkof] | Effect |
|---|---|---|---|
| 0x100 | 'CT' 0x4354 type 1 [0x81bc6] | 0 [0xd8618] | append record, flags 0x02 |
| 0x101 | 'CT' type 0 [0x81ca0] | 1 [0xd87a0] | remove record |
| 0x102 | 'CT' type 3 [0x81c90] | 2 [0xd873a] | remove all |
| 0x103 | method 0x81a88 [0x81aa6] | 3 [0xd875a] | append record with given flags, current=1 |
| 0x104 | AACP 0x09 id 6 value≠0 [0x81bea] | 4 [0xd866c] | all *other* records: clear 0x20,0x80; this host: set 0x10, cleared on all others (FUN_00081f0e is exclusive) [0x81f0e]; re-evaluate |
| 0x105 | AACP 0x09 id 6 value=0 | 5 [0xd87c8] | this host: clear 0x10,0x20,0x80; the best other record gets 0x10; re-evaluate |
| 0x106 / 0x107 | 'AS' 0x4153 types 7–13 / 21–27 (table 0x1dba80, mask 0x1fc07f) [0x81c08] | 6 / 7 | set / clear 0x08 (not exclusive) |
| 0x108 | 'TV' 0x5654 type 0x22 [0x81c40] | 8 | exclusive bit 0x04 |
| 0x109 | internal (re-evaluation) | 9 [0xd86d6] | broadcast 'AD' 0x4144 type 1 {new mac, priority}; **send AACP 0x09 id 6 = 1 to the new owner**, and **= 0 to the old owner** if there was one [0xd8708-0xd8738] |
| 0x10a / 0x10b | 'SR' 0x5352 type 0x42, event = id^0x0b [0x81c62] | 10 / 11 | exclusive bit 0x20 / clear 0x20,0x80 |
| 0x10c | internal (no records) | 12 | broadcast 'AD' type 0, zero mac |

The OwnsConnection sender: object field +0x68 = interface with vtable
0x1d5b24 [rkof 0x1608c4, 0x1608fa]; slot 2 = FUN_000c893e → FUN_0006e804
(session by mac, id 6, u32 value) [rkof 0xc893e-0xc8946, 0x6e804].

Priority order (Known from the comparison, meaning of the bits Inferred):
0x20 ('SR' id 1) > 0x10 (host asserted OwnsConnection=1, most recent
writer only) > 0x08 ('AS' stream states, probably A2DP streaming) > 0x04
('TV') > 0x02 (connected). So a host writing OwnsConnection=1 beats a host
that is streaming, at once, with no timer or guard in this component.

## AudioSource 0x0E [Known]

- Change path FUN_00084c4c [rkof 0x84c4c-0x84d44]: consumes broker class
  'AS' 0x4153 messages `{class, type byte +2, mac +3..+8}`; keeps ONE global
  bitmask (obj+0x1e) and ONE mac (obj+0x18, the mac of the last message):
  type 7/0x15 set/clear 0x02, 8/0x16 0x04, 9/0x17 0x01, 0xb/0x19 0x20,
  0xc/0x1a 0x08, 0xd/0x1b 0x10 (TBB at 0x84c7e). Bitmask 0 → mac zeroed.
  Sent (0x875e8, not forced) to every AACP session whenever the bitmask or
  the mac changed [0x84d10-0x84d3e].
- So the 0x0E kind is a bitmask (0x02 media, 0x01 call per calvados'
  observations; 0x04/0x08/0x10/0x20 other activities, meanings unknown), and
  the mac is whoever changed state last, not an arbitration result: one
  host's stop clears a bit another host also set. Values like 0x03 or 0x08
  are possible (calvados' `Activity` keeps every bit).
- Session-open path FUN_00087594 [0x87594] instead derives it from the
  profile state via FUN_00098d04 [0x98d04]: the first of 2 device slots (table
  0x2005ede8, 0x98 bytes each) with an active call (SCO state 0x2004f8dc) or
  A2DP stream (0x200c3bf8, +0x34 == 2), mapped by table 0x1c22a8.
- The same 'AS' messages types 7–13 / 21–27 set/clear the arbiter's 0x08
  bit for that host (events 0x106/0x107).

## Crash candidate: the arbiter has exactly two host records [Known code, trigger Inferred]

- Record pool: `block_mem_pool(arbiter+0x58, mem 0x200c47b0, 0x20 bytes,
  0x10 per record)` [rkof 0x1608de-0x1608ea, 0x739dc] = **2 records**.
- Action 0 (event 0x100, 'CT' type 1) [rkof 0xd8618-0xd8650] and action 3
  (event 0x103) [rkof 0xd875a-0xd878e] allocate a record with FUN_00081f54
  [rkof 0x81f54] **without checking whether that mac already has one**; an
  empty pool → `Spicy_LogAbort 0xdfee06` / `0xdfee07` → FUN_0001823c
  (the panic used by every ASSERTION FAILED) [rkof 0xd888c-0xd889a].
- So a third "host added" while two records exist — a duplicate add for a
  host already listed, or an add before the matching remove — reboots the
  pods. Whether 'CT' can deliver such a sequence (e.g. a reconnect racing a
  disconnect) is not established.
- Other aborts in this component: unknown broker class [rkof 0x81ccc]
  (0xdfee05), record lookups failing in the serialization interface
  [rkof 0x81a72, 0x81ac2] (0xdfee03/04), action 9/7/6/11 on an unknown mac
  only log (0x5dfee08..0a) [rkof 0xd8860-0xd8870].
- Event 0x103 comes from FUN_00081a88 [rkof 0x81a88], a method of the
  arbiter's interface at +0x80 (vtable 0x1c1e0c, registered as 0x414d
  [rkof 0x160900-0x16091e]): a buffer `[mac 6][flags]`, ≥ 8 bytes; its
  sibling FUN_00081a58 returns the current owner's mac. Inferred: state
  transfer between the buds (primary switch).

## The 'CT' publisher: host connection tracker (object 0x200b9c18) [Known]

- Constructed at [rkof 0x16080e-0x16084e] with HSM descriptor 0x1c6f30,
  actions FUN_000db594 (TBB at 0xdb5a4); publishes 'CT' through +0x48.
- Its HSM has states for 0, 1 and 2 tracked hosts (8, 9, 10 in
  the HSM table at 0x1c6f30): event 0x0e (host added) moves 8→9→10 publishing
  'CT' type 1 {mac} (action 4, [rkof 0xdb5ce]); event 0x0d (removed) moves
  back, publishing type 0 (action 3, [rkof 0xdb600]); entering the parent
  counts the list at +0x2c and asserts < 3 hosts (action 7, abort 0x369f0b,
  [rkof 0xdb684-0xdb6b8]); a type-2 publish asserts exactly one host
  (action 2, abort 0x369f0c, [rkof 0xdb5d2-0xdb5ea]); clearing publishes
  type 3 (action 6).
- **A third "host added" in the 2-host state runs action 5 = FUN_0009b6b4:
  an unconditional `Spicy_LogAbort 0x369f08`** [rkof 0xdb6c2, 0x9b6b4-0x9b6c0].
- Where events 0x0d/0x0e come from (ACL? AACP session? profile connect?)
  is not established.

## Answers so far

1. OwnsConnection (0x09 id 6) [Known]: a host's write only feeds the
   arbiter (events 0x104/0x105). Value≠0 makes that host the exclusive holder
   of priority bit 0x10 and drops the 0x20/0x80 bits of every other host;
   value 0 drops this host's 0x10/0x20/0x80 and gives 0x10 to the best other
   host. If the winner changes, the pods send `09 id6 = 1` to the new owner,
   `= 0` to the previous owner (if any), and broadcast 'AD' {new owner}
   internally. Nothing in this path pauses, suspends, or touches A2DP/AVRCP
   directly; no timers or guards (one-state HSM). Same id-6 echo goes out
   only through action 9 (no other sender of id 6 exists: 0xc893e is the only
   `0x6e804(…, 6, …)` caller).
2. 0x10 [Known]: relayed byte for byte to the target as 0x11 with the
   sender's mac; nothing parsed; refused silently when the target has no
   AACP session or when flag `[0x200c41ec+0x1c]` is set.
3. Partly. Ownership priority: 0x20 > 0x10 (OwnsConnection) > 0x08 ('AS'
   activity) > 0x04 > 0x02. The 0x0E AudioSource is not the arbiter's view:
   on change it is the last host whose 'AS' audio state changed plus a
   global activity bitmask. 'AD' (owner changed) consumers: a broker adapter
   [rkof 0x78b4a] (wired to stubs [rkof 0x1639d6-0x1639f0, 0x150c80]), an
   idle-timer HSM (descriptor 0x1c6c10, 480000 = 0x75300 timer [rkof
   0xdaf34]), and FUN_000a7708 [rkof 0xa7726-0xa77b8] (calls into a
   further object, not traced). **Not established**: which code accepts or
   defers an AVDTP Start from a second host, who sends the old host AVRCP
   PAUSE / AVDTP Suspend on a take, and any guard interval. AVRCP passthrough
   sender found: FUN_00070328(mac, op) press+release, PLAY 0x703cc, PAUSE
   0x703e8 [rkof 0x70328-0x70400]; its callers 0x85ef0/0x85f14 (a 28-command
   dispatcher FUN_00085c88), 0x9556c (AudioSeqManagerAPI area), 0xca0a6 —
   none tied to the arbiter yet.
4. Asserts found on these paths: two-record arbiter pool overflow
   (0xdfee06/07), connection tracker third host (0x369f08) and count
   checks (0x369f0b/0c), active_object_event_queue.cpp:0xd8 (re-entrant
   dispatch) and :0x165 [rkof 0x74266-0x74278], block_mem_pool.cpp:0x76
   (freeing a foreign block) [rkof 0x73902]. None is shown to fire during a
   takeover; the reboot mechanism remains unexplained by this analysis.
5. Rules a host can draw (Inferred from the code above):
   - Ownership is "last writer of 1 wins" and is decided immediately;
     writing 1 repeatedly or from both hosts just flips the owner each time,
     each flip sending 0x09 to both hosts and an internal 'AD'. Avoid
     ping-pong: don't write 1 again while already owner (the firmware sends
     nothing if the winner is unchanged, so it's harmless but useless).
   - Writing 0 hands bit 0x10 to the other host, i.e. makes it owner.
   - Smart-routing frames have no firmware effect; their order relative to
     OwnsConnection does not matter to the pods, only to the peer host.
   - Don't trust 0x0E as "who owns": it's last-changed device + bitmask;
     accept kinds other than 0/1/2.
   - After letting go without writing 0, the pods still count this host as
     owner, and a later write of 1 gets no answer (calvados: `PodsOwner`).
