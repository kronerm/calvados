# Which host the pods connect to

AirPods Pro 2 (USB-C), build 9A348, image `rkof` (hashes and tools:
[firmware.md](firmware.md)). Cited `[rkof 0xADDR]`; Known = read from code,
Observed = our logs, Inferred = marked. Read Nov 9 for the laptop that
connected first and locked the phone out.

## The rule

The pods page out only while **no host is connected**, one host at a time,
and stop once one connects. A host that pages them first (the laptop's
presence scan, ~0.4 s after the lid opens) ends it: the host they would
have paged is never paged. Nothing over AACP makes them connect both.

## The reconnect machine [Known]

Object 0x2004f7ec, an HSM at +0x30 (descriptor 0x1c6d38, actions
FUN_000dafa0, events posted by 0x99228). Fields: targets 0 and 1 (+0, +6),
the MAC being paged (+0xc), per-target last failure and count (+0x14,
+0x26), the wait (+0x1c, ms), hold reasons (+0x24, a u16 mask), round,
step and current target (+0x28-+0x2a), paging (+0x2b), single-target mode
after a warm reset (+0x2f). Timer 0x16 drives the waits.

- State 5, armed: events 0, 8 and 9 go to paging (7) through the guard.
- State 9, a page in flight (action 0xb, 0x9d1cc(mac, 1) [0xdb1f4]):
  event 7 (attempt over) picks the next target and wait (state 11); a hold
  (1) or 0xa cancels the page; **event 6 is swallowed**.
- State 11 waits for timer 0x16, then pages again (9).
- State 10: the paged host connected (event 0xb, 0x99750 [0x8f2a0]).
- Event 6 in 7, 10 or 11 rebuilds the targets (0x997e4), restarts the
  timer and returns to 5.

**The guard**, FUN_00099998 [0x9999e-0x999bc], after rebuilding the
targets (0x999d0): no host ACL (`[0x2005ede8+0x130]` == 0), 0x9ae0c() true
(the HSM at [0x200c1d24+4] in state 2 or 8, +0x38 == 0: no host link being
established), and a target.

**Arming**, FUN_00099334(arg): the wait set to 500 ms, event 6. Args: 0
cold boot and 1 warm boot (the init FUN_00151510 [0x152ee4]; or held,
0x997ac(1), until released), 4 a host link dropped (0x98534 → 0x99420),
5 a host's device slot reached state 5 (FUN_0008544c [0x85466]), 10 a
host excluded or opcode 0x14 [0xcd1b8], 2 broker 'AP' type 0 [0x8e3b6], 3
'UT' type 4 (shortens a wait); 8, 9, 0xb stop it.

'AP' (0x4150, object 0x200ba890, HSM 0x1c8124) in 0x8e1ec: type 2 holds
paging, 3 releases it, 1 **disconnects every host with reason 2**
[0x8e46e → 0x9a300]. Inferred: the put-away policy. FUN_0008544c drops
every host the same way when 0xb6514() (an 'AP' query) holds.

So when a host connects mid-wait its slot's event 6 returns the machine
to 5 and the guard fails; mid-page the event is swallowed, the page runs
out, and the next attempt's guard fails.

## The targets [Known]

0x997e4 → 0x93144:

- **Target 1**: the host in NVRAM key `bvi05` ("bvi%02X", [0x1e21fa];
  read 0x93a68, written 0x93a30) if allowed, else the first allowed host
  in the paired-device list (0x2000bcb8: count +0x44, order bytes +0x4c,
  persisted). The list is most recent first: a host moves to the front at
  its first profile connect [0x9c836: 0x93704(mac, 0)] and on a new link
  key [0x93efa].
- **Target 2**: only with multipoint on for target 1 (0x93090), and only
  from a partner record (type-2 flag 0x20, written by opcode 0x14 or the
  buds' sync) or, in AutomaticConnection mode 1, an account partner.
  Inferred: none for calvados hosts.
- **Allowed**: a paired record, its flag 0x40 clear (`AllowAutoConnect`
  off sets it), and its bit clear in the no-page mask
  `**(0x200c1ca8+0x20)` [0x94270].
- **The no-page mask** is set by a disconnect whose reason is in the table
  at 0x1c2a8c: 713, 715, 719-722, i.e. 700 + HCI 0x0D, 0x0F, 0x13 (remote
  user terminated), 0x14, 0x15 (remote power off), 0x16 (local host
  terminated) [0x99420 → 0x9423c(mac, 1)], and cleared at that host's next
  profile connect [0x9c82e]. A link loss (0x08) or a page timeout (0x04)
  doesn't set it. So a host disconnected on purpose, or dropped as the lid
  closes (722 for every host), isn't paged until it connects by itself.
- **After a warm reset** (0xc1b3c bit 0) → 0x99334(1), one target: the MAC
  at +0x10 of the same retained block, written on every owner change ('AD'
  → 0xb6e40). So after a crash the pods page the last audio owner.

## Attempts [Known]

Schedule table 0x1c2a68, rows `{rounds, gap after target 1, gap after
target 0}` ms: `{3, 1000, 500}`, `{5, 10000, 1000}`, `{forever, 60000,
2000}` [0xdb08c-0xdb1a0]; two targets alternate. A page lasts until the
controller's page timeout (Observed ~5 s: tries 6 s apart). 0x99420 on
failures: auth failure or a missing key twice deletes the key [0x1b11bc],
0x2cf more than twice excludes the host, 0x2c7 (ACL exists) moves on.

The 0x2B history logs the outgoing connection's completion as kind 9
(`PagedHost`) [0x54474], and a disconnect the pods start as kind 0x0b
(`Dropped`, 0x99d88) with their reason: 2 every host (put away), 4 a UARP
hold with the lid closed, 0xc a third host after opcode 0x14. Observed Nov
9: the phone paged (9, status 0), dropped a minute later (0x0b, 2), then
its `HostLeft` 722.

## What a host could send [Known]

None of these makes the pods connect two hosts, and all write NVRAM:

- 0x09 id 0x36 `AllowAutoConnect` (handler 0xb7418): 2 sets the sender's
  flag 0x40 (never paged, never `bvi05`), 1 clears it.
- 0x09 id 0x20 `AutomaticConnection` (0x98a84): record flag bit 0 (mode
  1) or bit 2 (mode 2). In mode 2 the host becomes `bvi05` whenever it
  becomes the audio owner ('AD' → 0xb6e40 → 0xb6f24), so the pods page it
  first (Inferred: iOS's "When last connected").
- Opcode 0x14 `[2] [mac A] [mac B]` (0x6d4e6 → 0xcb72c → FUN_000cd058):
  stores A and B, gives A arbiter bit 0x04, links them as partners (each
  the other's target 2), **disconnects every host but A and B** (0x9a364,
  reason 0xc), re-arms paging.
- Opcode 0x44 subtypes 2, 3, 5: partner records, `bvi05` = the sender, a
  preferred MAC told to the other bud.

## The advertisement [Known unless marked]

Built by FUN_000dce34 (HSM 0x1c7a70). The proximity format (`07 19 01` +
24 bytes, from the type byte): byte 5 the status (0x40 when 0xc61a4() is
false), byte 8 the lid (bit 3 only docked with the lid closed, 0x10
always, an open counter in bits 0-2), **byte 10 in plaintext: bits 2-3 the
host ACLs plus one while one is being established (the guard's terms),
capped at 3; bits 0-1 the audio's kind**, bytes 11-26 encrypted. The worn
capture in `ble.rs` reads 2 hosts. Observed Nov 13 15:11:35, the lid
opening: 0, then 1 0.2 s later (the phone), then 2 10 s later (the laptop). Nothing stops it while hosts are
connected. Inferred: 181.25 ms apart, 20 ms for 10 s after the lid opens.
A short format (`07 11 06` + 16 encrypted bytes) exists too; its byte 8 is
ciphertext, so the Android scan's lid filter drops about half of them.
What selects it is open.

## The reset in the case [Inferred]

A shutdown manager, FUN_0015f804(reason 1-8), runs from the case's
messages (reasons 2, 3, 5, 6), the main loop (4) and a 1 s poll while
charging (1). The case puts the buds into ULP after the lid has been
closed for 30 s (STFC, [update-host.md](update-host.md)). The pods'
uptime restarted 5-30 s after both buds went into the case (Nov 6, Nov 9)
and the laptop's link then timed out (0x08) instead of ending: a power
down with no LMP detach. Whether the retained block survives it is open.

## For calvados

- Linux presence races the pods: when they advertise 0 hosts and the
  laptop wasn't the last owner, wait for the count to rise (the other host
  got paged) or for one page to run out (~0.5 s + ~5 s) before connecting.
  Pending: logs of the count (`our pods advertising`).
- After a lid-close drop or a manual disconnect, the pods won't page that
  host again: it has to connect itself (the redial and presence do).
- Don't send 0x36, 0x20, 0x14 or 0x44 by default.
