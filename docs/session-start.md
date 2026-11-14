# What the pods send a new AACP session

AirPods Pro 2 (USB-C), build 9A348, image `rkof` (hashes and tools:
[firmware.md](firmware.md)). Cited `[rkof 0xADDR]` (9A348); Known = read
from code, Observed = the Linux daemon's journal (HCI and AACP lines),
Inferred = marked.

## Three triggers

- **The handshake (AAP service 4 open)** [Known]: FUN_0006ca6c, a
  three-entry session table keyed by L2CAP channel: the same channel again
  → status **0x85**, table full → **0x68**, else the session is
  registered [rkof 0x6caba-0x6caca]. Success sends the `0x2B` history at
  once [rkof 0x6cc22] and posts "AAP connected" (profile bit 0x10) to the
  host's device slot (`0x9c6dc`). Observed: the success ack can lag the
  open by seconds (5.5 s; Oct 28 11:14:41 a resend's 0x85 came 2.3 s
  *before* the first open's status-0 ack).
- **The state half**: battery 0x04, ears 0x06, `PrimaryBud` 0x08, the
  other bud's `DeviceConnection` 0x0C, `AudioSource` 0x0E, then
  `ConnectedDevices`. A synchronous reply to *each* `0x0F` request [rkof
  0xcd290 → 0x876cc], to that host only, for the bits it asks; no timer.
  Per-item gates: battery `0xc61a4()` (9B47: `0xc6184`), ears
  `[0x200c1e1c+0x44]`, `PrimaryBud` the host's AAP profile bit. Later
  change notifications need the host's notification state ≥ 2, set by
  "AAP connected". Observed: 5-100 ms after `0x0F` in nearly every
  session; five `0x0F`s queued during a stall got five bursts (Oct 28
  11:14:05-09).
- **The settings half**: `0x15`, our own `DeviceConnection` (host),
  `0x02`, `0x1D`, the noise mode and the other `0x09` settings, EQ. One
  function, FUN_00085620, run only once the host's device slot reaches
  state 5 ("connected") with AAP up [callers 0x8544c, 0xcd420]. No inbound
  opcode reaches it and `0x1D` has no other sender: **nothing solicits
  it**. Observed: a new AACP channel over a link already in state 5 gets it
  at once (0.02-0.16 s, Oct 27 20:19-23:53).

## The profile wait (state 4 → 5) [Known]

Two device slots (0x2005ede8, stride 0x98; profile masks at 0x2004f89c +
32·slot: connected +4, pending +8, expected +0xc). Once the host's Device
ID is read (profile bit 0x80; vendor 0x4C marks it Apple), the pods expect
every profile it advertises (masked by `0xdf3f` for Apple, else 7; AAP only
with a DID feature bit). While some are missing they arm timer 0x47/0x48
at **20 000 ms** [rkof 0x9cc7c] (5 000 ms before the DID is known), connect
the missing ones themselves (`0x9c30c`), and on expiry retry up to 3 times
5 s apart (timer 0x4c/0x4d, `0x9b900`), then drop them from the expected
set and go to state 5 (`0x98304`). A host marked by link record +0x20 bit
3 skips the wait (`0x27b44`; what sets the bit is not established).

Observed, time to the settings half:
- 7.1: laptop-paged fresh sessions 20.2-22.4 s (a few 33-39 s: the
  retries); pods-paged ones 0.2-1.1 s (the pods opened the profiles).
- 9A348: 0.9-1.8 s alone; 23 s and 32 s with the phone connected and
  streaming (Oct 27 22:19:16, Oct 28 11:16:31). Inferred: the "24 s until
  the other host left" in AGENTS.md was this timer.

Profile bits named so far: 0x10 AAP, 0x04 ≈ AVRCP, 0x80 DID.

## Feature flags (0x4D) [Known]

Stored per host (slot +0x68/+0x6c, if the slot exists); then
`ConnectedDevices`, the noise mode again (only to state-5 hosts), and
broker 'FM'. Readers of `0x98ef4(mac, bits)`:
- **0x04**: the noise-mode sender reports Adaptive (4) only to a host with
  it [rkof 0x87930]; otherwise the 0x200 fallback or Off (1) [rkof
  0x87942-0x87946]. Hence "Adaptive as Off" without the flags.
- **0x200**: that fallback; **0x4000**: another ear-state encoding
  [rkof 0xd466c]; **0x01**: stored per host [rkof 0xd3198], use not traced.
- No reader of 0x02, 0x10, 0x40, 0x80 found.

## For a host

- Don't re-send the handshake on the same channel: it gets 0x85 and the
  first open still completes. If no ack comes, open a new channel:
  answered within 0.01 s in all three captures (Oct 27 18:03:35,
  18:26:33, 22:35:01). calvados: `Engine::check_startup`.
- Re-sending `0x0F` is harmless: each gets a full state burst.
- The settings half can't be hurried, short of a new channel once the
  pods count the host connected.

## Open

- Why the first open's ack lags (AAP core around 0x3f778-0x40da0).
- Which profiles are awaited; a btmon trace of a slow session would name
  them. Handlers in `0x9c30c`: 0x1 `0x7230c`, 0x2 `0x6c728`, 0x8
  `0x67df8`, 0x20 `0x689d0`, 0x40 `0x72818`, 0x100 `0x691e0`, 0x200
  `0x6a060`, 0x400 `0x68344`, 0x800 `0x6809c`, 0x1000 `0x69d08`, 0x4000
  `0x69a78`, 0x8000 `0x674b0`.
- What the battery gate `0xc61a4` checks; timer 0x46 (6 s with two hosts,
  started in `0x85660`).
