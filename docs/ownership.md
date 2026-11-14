# Ownership and audio routing in the AirPods firmware

How the pods arbitrate between hosts. AirPods Pro 2 (USB-C), build 9A348,
image `rkof` (hashes and tools: [firmware.md](firmware.md)). Static
analysis, cited `[rkof 0xADDR]` (9A348; in 9B47 the arbiter's table is
unmoved, code shifted by 2-58 bytes, nothing relied on changed); Known =
read from code, Observed = our logs, Inferred = marked.

Most of `rkof` is built with LLVM's machine outliner: many `bl` into
0x1a…–0x1b9… are 2–6 instruction fragments using the caller's registers
and stack, so Ghidra's decompiler is unreliable here; read the disassembly
with those fragments inlined.

## Summary for a host

- `OwnsConnection` (0x09 id 6) only feeds the arbiter. The last host to
  write 1 owns at once, outranking a streaming host; no timer or guard.
  Writing 0 hands ownership to the best other host.
- The pods send `09 06 = 1` to the new owner and `= 0` to the old one only
  when the owner changes; writing 1 while owner gets no answer. So after
  letting go without writing 0 the pods still count this host as owner
  (calvados: `PodsOwner`). Repeated writes from both hosts flip the owner
  each time.
- Nothing on this path pauses, suspends or touches A2DP/AVRCP directly.
- Smart-routing frames (0x10) have no effect on the pods; their order
  relative to `OwnsConnection` matters only to the peer host.
- `AudioSource` (0x0E) is not "who owns": it's the last host to start
  something plus an activity mask over all hosts. Accept kinds other than
  0/1/2.

## AACP receive layer is a pure parser [Known]

- The opcode dispatcher FUN_0006ceb0 (TBH at 0x6cf48) parses each frame
  and calls a callback from table `*0x2004cf6c` [rkof 0x6d1a4]; the table
  is 0x2005ec34 [rkof 0x8af88..0x8af8e], filled by FUN_000cb170.
- Opcode 0x09 → callback +0x20 = 0xcb4fc [rkof 0xcb292, lit 0xcb388];
  opcode 0x10 → +0x30 = 0xcb5e8 [rkof 0xcb282].

## 0x10 smart routing: relayed verbatim [Known]

0xcb5e8(session, target mac, len, data) [rkof 0xcb5e8-0xcb672]:
- any argument null → error 0x65; target not a connected AACP session
  (lookup 0x6ca30) → 0x7a; global flag `[0x200c41ec+0x1c]` set → 0x76.
  None of these sends anything.
- Else builds 0x11: opcode, the *sender's* mac (fragment 0x1ad4fc writes
  it reversed into +2..+7), len LE at +8/+9, data at +10; sent to the
  target with 0x6e55c, logged (0xc8110 id 0x44766f12).
- The payload isn't parsed (no reason or ownership keys).
- The same flag also blocks the 0x0E sender [rkof 0x87616].

## 0x09 control → message to the ownership arbiter [Known]

- 0xcb4fc posts {session mac, id, value} to handler 0x879c0 (task
  context) [rkof 0xcb4fc-0xcb51e].
- 0x879c0: `TBH` on id-1 at 0x879ec (table 0x879f0). Ids 6, 0x20, 0x36 →
  0x87aea: a broker message, class 0x4161 type 0x21 {mac, id, value},
  through `[[0x200c33a8+0x30]]->vtable[0]` to destination
  `[0x200c1c2c+0x24]` (0 → "Spicy_LogAbort") [rkof 0x87aea-0x87b1c].
- Receiver: the arbiter, object 0x200b9cb8, vtable 0x1c1e58, constructed
  at [rkof 0x1608b4-0x16095a]; subscribes 'CT' 0x4354, 'AS' 0x4153, 'TV'
  0x5654, 'Aa' 0x4161, 'SR' 0x5352. Its entry FUN_00081b88: class 0x4161
  type 0x21 id 6 → event 0x104 (value ≠ 0) or 0x105 (value 0) with mac and
  bool [rkof 0x81bd4-0x81c06]; other ids of that class are dropped there.

## The ownership arbiter (object 0x200b9cb8) [Known unless marked]

State machines are active objects (`active_object.cpp`) running a
table-driven HSM. Engine FUN_00072982: descriptor D = {+0 states (8 B:
u16 first transition, +4 parent, +5 initial child, +6 depth, +7 count),
+4 transitions (3 B: [action, event, target]), +8 action function, +0xc
guards (fn, u16 transition), +0x13 guard count}; the event id is the low
byte of the event's u16 kind at +4 [rkof 0x74168].

Arbiter descriptor 0x1c5a9c [rkof 0x1608d2]: one state, 13 transitions
`[n, n, ff]` for n = 0..0xc [bytes at 0x1c5a74]: every event 0x1nn runs
action n with no state change. Actions: FUN_000d85e8, TBH at 0xd85fa.

Per-host records in a list at +0x50 (record: +8 mac, +0xe flags, +0xf
"current owner") [rkof 0x81ae0, 0xd8618]. Re-evaluation FUN_00081e44
[rkof 0x81e44-0x81ecc]: the record with the largest `flags & 0x7f` wins
(ties: the earlier record); it's marked current; if it differs from the
previous owner, event 0x109 {new mac, old mac or zero, new flags}; an
empty list → event 0x10c.

| Event | Source | Action [rkof] | Effect |
|---|---|---|---|
| 0x100 | 'CT' type 1 [0x81bc6] | 0 [0xd8618] | append record, flags 0x02 |
| 0x101 | 'CT' type 0 [0x81ca0] | 1 [0xd87a0] | remove record |
| 0x102 | 'CT' type 3 [0x81c90] | 2 [0xd873a] | remove all |
| 0x103 | method 0x81a88 [0x81aa6] | 3 [0xd875a] | append record with given flags, current = 1 |
| 0x104 | 0x09 id 6 ≠ 0 [0x81bea] | 4 [0xd866c] | all *other* records: clear 0x20, 0x80; this host gets 0x10, exclusively (FUN_00081f0e [0x81f0e]); re-evaluate |
| 0x105 | 0x09 id 6 = 0 | 5 [0xd87c8] | this host: clear 0x10, 0x20, 0x80; the best other record gets 0x10; re-evaluate |
| 0x106 / 0x107 | 'AS' types 7–13 / 21–27 (table 0x1dba80, mask 0x1fc07f) [0x81c08] | 6 / 7 | set / clear 0x08 (not exclusive) |
| 0x108 | 'TV' type 0x22 [0x81c40] | 8 | exclusive bit 0x04 |
| 0x109 | re-evaluation | 9 [0xd86d6] | broadcast 'AD' 0x4144 type 1 {new mac, priority}; **send 0x09 id 6 = 1 to the new owner, = 0 to the old one if any** [0xd8708-0xd8738] |
| 0x10a / 0x10b | 'SR' type 0x42, event = id ^ 0x0b [0x81c62] | 10 / 11 | exclusive bit 0x20 / clear 0x20, 0x80 |
| 0x10c | no records | 12 | broadcast 'AD' type 0, zero mac |

The `OwnsConnection` sender: field +0x68, an interface with vtable
0x1d5b24 [rkof 0x1608c4, 0x1608fa]; slot 2 = FUN_000c893e → FUN_0006e804
(session by mac, id 6, u32 value) [rkof 0xc893e-0xc8946, 0x6e804], the
only `0x6e804(…, 6, …)` caller, so action 9 is the only source of id 6.

**Priority** (from the comparison): 0x20 > 0x10 (the most recent host to
write 1) > 0x08 ('AS' stream states) > 0x04 > 0x02 (connected). 0x80 is
outside it: the winner is chosen on `flags & 0x7f` [rkof 0x81e44] and
FUN_00081f0e stores every record `& 0x7f` [rkof 0x81f46]; 0x80 enters
only through event 0x103 (Inferred: the buds' state transfer), so it never
decides the owner.

Where the upper bits come from [Known]:
- **0x20**: AACP 0x44 from a host, `44 00 [03 00] [04 00] [v]` (subtype
  4): handler FUN_000cc368 publishes 'SR' type 0x42 {sender's mac, v ≠ 0}
  [rkof 0xcc436-0xcc47a], the only publisher of that type; v = 1 sets
  0x20 (event 0x10a), v = 0 clears 0x20/0x80 (0x10b). Any later
  `OwnsConnection = 1` clears it (action 4). Inferred: an Apple host's
  take; calvados then sees only `OwnsConnection = 0`, no yield request.
- **0x04**: AACP 0x14, a host-sent list of MACs; only a list of exactly
  two reaches 'TV' type 0x22, and the *first* MAC gets the bit [rkof
  0x6d4e6 → 0xcb72c → 0xcd058, publish 0xcd0aa] (opcode 0x14's other
  effects: [paging.md](paging.md)).
- calvados sends neither 0x44 nor 0x14.

## Owner changes ('AD') [Known unless marked]

Consumers of 'AD':
- a broker adapter [rkof 0x78b4a] whose callback is a `bx lr` stub [rkof
  0x150c84];
- an idle-timer HSM (descriptor 0x1c6c10, a 480 000 ms timer [rkof
  0xdaf34], fed by FUN_00094878);
- FUN_000a7708 [rkof 0xa7726-0xa77b8]: sends one bool through
  0x7f410(0xd, 0xe1), whether the owner's device slot has profile bit 0x80
  (the Device ID read, [session-start.md](session-start.md));
- 0xb6e40: the retained owner MAC that a warm reset pages, and `bvi05` in
  AutomaticConnection mode 2 ([paging.md](paging.md)).

AVRCP passthrough sender: FUN_00070328(mac, op), press + release; PLAY
0x703cc, PAUSE 0x703e8 [rkof 0x70328-0x70400]. Callers 0x85ef0/0x85f14 (a
28-command dispatcher FUN_00085c88), 0x9556c (AudioSeqManagerAPI area),
0xca0a6 (Inferred: inside 0xca0a4, the stream-sync timeout's PAUSE,
[firmware.md](firmware.md)); none tied to the arbiter.

## AudioSource 0x0E [Known]

- Change path FUN_00084c4c [rkof 0x84c4c-0x84d44]: consumes 'AS' messages
  `{class, type +2, mac +3..+8}`; keeps ONE global bitmask (obj+0x1e) and
  ONE mac (obj+0x18): types 7/0x15 set/clear 0x02, 8/0x16 0x04, 9/0x17
  0x01, 0xb/0x19 0x20, 0xc/0x1a 0x08, 0xd/0x1b 0x10 (TBB at 0x84c7e). Sent
  (0x875e8) to every AACP session whenever mask or mac changed
  [0x84d10-0x84d3e].
- The mac is the last host to *set* a bit: a clear leaves it [rkof
  0x84cde-0x84cea vs 0x84cf6]; an empty mask zeroes it [rkof
  0x84cfe-0x84d0c]. One host's stop clears a bit another host also set.
- Publishers: one state machine per (host, stream), FUN_000d8b28; the
  stream kind picks the type: A2DP → 0x02 (media), SCO → 0x01 (call:
  cellular, and VoIP over the headset profile), two unnamed kinds → 0x04
  (state in the A2DP record, +0x34) and 0x20 (state in the transport-0xd
  session table 0x200c4f38), one more → no 0x0E bit but the arbiter's
  0x08. 0x08/0x10 come from a separate one-device module (callbacks
  0x1c4a08, global 0x200c1f90). No publisher for ringing, Siri or the
  pods' own tones found. Observed (laptop, Oct 19-28): only 0x00, 0x01,
  0x02.
- At session open FUN_00087594 derives it from the profile state instead,
  via FUN_00098d04: the first of 2 device slots (table 0x2005ede8, 0x98
  bytes each) with an active call (SCO state 0x2004f8dc) or A2DP stream
  (0x200c3bf8, +0x34 == 2), mapped by table 0x1c22a8 (values 0, 0x01,
  0x02, 0x04, 0x06, 0x08, 0x10, 0x20, 0x22). With nothing playing it
  names the *receiving* host with kind 0, not a zero mac (Observed ~60
  times).
- The same 'AS' types also set/clear the arbiter's 0x08 (events
  0x106/0x107).

## The A2DP switch [Observed on 9A348, code partly Known]

From the laptop's HCI lines (Oct 27-28) and the image:
- A second host's AVDTP START is never refused: 77 of 78 accepted (the
  78th lost to a link timeout). While another host streams, the accept
  comes 0.6-0.9 s late instead of 0.04-0.35 s, `AudioSource` changing in
  between: the pods defer it. The deferring code is not found.
- The old host gets an AVDTP SUSPEND within a few ms of its
  `OwnsConnection = 0`; `AudioSource` goes to zero 0.15-0.4 s later, then
  names the new host. No AVRCP PAUSE (6 of 6). The SUSPEND's sender is
  FUN_000c9fc8 [rkof 0xc9fc8-0xca08a] (signal 9, only while that host
  streams), slot +0x18 of interface vtable 0x1c526c (PLAY +0x14, PAUSE
  +0x1c), installed at [rkof 0x162604]; its caller is not found. Inferred
  from timing: the owner change ('AD') drives it.
- No guard timer: none of the numbered timers (start 0xc1c7c, register
  0xc1c3c) is tied to a source switch. The "ignored" restarts on 9A348
  were the laptop never sending a START (Oct 27 22:26:30-32 and 45-48,
  Oct 28 11:06:52-54: the node `Suspend` sent nothing over the air); every
  START that went out was accepted, 1.2 s after a take at the earliest.
  The 7.5 s case (Oct 18, 7.1) predates the HCI logging.
- The phone's `START_EVT status=1` is not explained (no phone trace yet).

## Aborts on these paths [Known code, trigger Inferred]

None is shown to fire during a takeover. The one crash log fetched so far
(Nov 4) is a different assert, `stream_sync_a2dp.cpp:429`
([firmware.md](firmware.md)).

**The arbiter has exactly two host records.**
- Pool: `block_mem_pool(arbiter+0x58, mem 0x200c47b0, 0x20 bytes, 0x10
  per record)` [rkof 0x1608de-0x1608ea, 0x739dc] = **2 records**.
- Actions 0 (event 0x100) [rkof 0xd8618-0xd8650] and 3 (0x103) [rkof
  0xd875a-0xd878e] allocate with FUN_00081f54 **without checking whether
  that mac already has a record**; an empty pool → `Spicy_LogAbort
  0xdfee06` / `0xdfee07` → FUN_0001823c (the panic behind every ASSERTION
  FAILED) [rkof 0xd888c-0xd889a].
- So a third add while two records exist (a duplicate add, or an add
  before the matching remove) reboots the pods. Whether 'CT' can deliver
  that (a reconnect racing a disconnect) is not established.
- Event 0x103 comes from FUN_00081a88, a method of the arbiter's
  interface at +0x80 (vtable 0x1c1e0c, registered as 0x414d [rkof
  0x160900-0x16091e]): a buffer `[mac 6] [flags]`, ≥ 8 bytes; its sibling
  FUN_00081a58 returns the current owner's mac. Inferred: state transfer
  between the buds (primary switch).
- Other aborts here: unknown broker class [rkof 0x81ccc] (0xdfee05),
  failed record lookups in the serialization interface [rkof 0x81a72,
  0x81ac2] (0xdfee03/04); actions 9/7/6/11 on an unknown mac only log
  (0x5dfee08..0a) [rkof 0xd8860-0xd8870].

**The 'CT' publisher: host connection tracker (object 0x200b9c18).**
- Constructed at [rkof 0x16080e-0x16084e], HSM descriptor 0x1c6f30,
  actions FUN_000db594 (TBB at 0xdb5a4); publishes 'CT' through +0x48.
- States for 0, 1 and 2 tracked hosts (8, 9, 10): event 0x0e (host added)
  moves 8→9→10 publishing 'CT' type 1 {mac} (action 4, [rkof 0xdb5ce]);
  0x0d (removed) moves back, publishing type 0 (action 3, [rkof 0xdb600]);
  entering the parent asserts < 3 hosts in the list at +0x2c (action 7,
  abort 0x369f0b, [rkof 0xdb684-0xdb6b8]); a type-2 publish asserts
  exactly one host (action 2, abort 0x369f0c, [rkof 0xdb5d2-0xdb5ea]);
  clearing publishes type 3 (action 6).
- **A third "host added" in the 2-host state runs action 5 = FUN_0009b6b4,
  an unconditional `Spicy_LogAbort 0x369f08`** [rkof 0xdb6c2,
  0x9b6b4-0x9b6c0].
- Where events 0x0d/0x0e come from (ACL, AACP session, profile connect)
  is not established.

**Framework asserts**: `active_object_event_queue.cpp:0xd8` (re-entrant
dispatch) and `:0x165` [rkof 0x74266-0x74278]; `block_mem_pool.cpp:0x76`
(freeing a foreign block) [rkof 0x73902].
