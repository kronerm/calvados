# Apple's AirPods features on a non-Apple host

What iOS 26 offers for AirPods Pro 2 (USB-C), and how calvados can or
can't. Researched Nov 14 from LibrePods (`docs/AAP Definitions.md`,
written against 7A305, and its Android and Linux code), AirGuard, the
DULT draft, and our firmware images (9A348/9B47 `rkof`, 7B21/9B47 NERF).
**C** = confirmed in our code or captures, **R** = reported by others,
**G** = guessed. Nothing below was captured from an iPhone on 9B47.

## Settings the pods keep (`protocol::pod_settings`)

All are AACP control commands (0x09) `[id] [v1 v2 v3 v4]`, sent by a host
and reported back by the pods; our captures show the pods report most of
them in every startup burst (C, Nov 14: Conversation Awareness `02`,
Personalized Volume `01`, adaptive level 50, one-bud ANC `02`, volume
swipe `01`, its length `00`, press speed and hold `00`, sleep detection
`02`). Toggles are `01` on, `02` off.

| Setting | Id | Values | Source |
|---|---|---|---|
| Conversation Awareness | 0x28 | toggle | R; reported C |
| Personalized Volume | 0x26 (`AdaptiveVolume`) | toggle | R; reported C |
| Adaptive level | 0x2E | 0, 50, 100 (25 and 75 are reported to other hosts as 50, 100; C) | R; reported C |
| Microphone | 0x01 | 00 auto, 01 right, 02 left | R |
| Optimized charge limit | 0x3B (`DynamicEndOfCharge`) | toggle | R |
| Case sounds | 0x31 (`InCaseTone`; volume 0x40) | toggle | R |
| Noise control with one bud | 0x1B | toggle | R; reported C |
| Volume swipe | 0x25 | toggle | R; reported C |
| Swipe length | 0x23 | 00 default, 01 longer, 02 longest | R; reported C |
| Press speed | 0x17 | 00 default, 01 slower, 02 slowest | R; reported C |
| Hold duration | 0x18 | 00 default, 01 slower, 02 slowest | R; reported C |
| Sleep detection | 0x35 | toggle | R; reported C |

Also known but not offered: Allow Off 0x34 (calvados never offers Off),
automatic ear detection 0x0A, call controls 0x24 (reported `00 03`), tone
volume 0x1F (reported `80 80`), "Hey Siri" 0x12, per-side press and hold
0x16 (values undocumented), raw stem gestures 0x39 (the pods then report
presses as 0x19 and the host acts).

Rename: AACP 0x1A `01 [length] 00` + UTF-8 (R); the pods re-report the
name in 0x1D.

## Conversation Awareness

With the setting on (and our feature flags 0x4D = 0xD7, which LibrePods
says it needs while audio plays), the pods switch to transparency by
themselves while you speak and send 0x4B `02 00 01 [level]`: 1-2 speaking
started, 6, 8, 9 back to normal (R). Seen on 9B47 (Nov 14, the phone,
ANC on): `1` `2` as you start; `3` `5` pairs every so often while the
conversation lasts (unnamed; the pods stay open); `11` `4` `8` `9` at its
end, ~5 min later in that test; `7` alone at a mode switch with it off
(C). The host lowers its own media: `Engine::conversation`
emits `Effect::Duck` once while media plays here, `Effect::Unduck` on
6/8/9, on losing the audio, or when the setting goes off. Linux sets the
pods' output node to 40 % of its volume with `wpctl` (AVRCP absolute
volume); Android fades the music stream to 40 % in 50 ms steps.

## Not built (yet)

- Loud Sound Reduction (ATT handle 0x1B, 1 byte), transparency
  customization (ATT 0x18, floats), hearing aid (0x2C/0x33 plus ATT 0x2A,
  104 bytes): raw ATT on PSM 0x1F (R; Linux through BlueZ's GATT).
- Head tracking: 0x17 class 0x100000, start/stop frames in LibrePods,
  orientation at fixed offsets (R). Head gestures are host-side detection
  on that stream; spatial audio needs a renderer.
- The hearing test (calibrated tones, undocumented), Live Translation,
  high-quality microphone audio: not feasible.

## Find My

Built: on Linux, `calvados find`, the signal as you move.

- Playing a sound on the buds: only as ordinary audio over A2DP, at the
  host's volume; no AACP "play sound" is known (R/C). A chime that way was
  built and removed (Nov 14): in the case it's too quiet to hear, and the
  case's own speaker is what finding needs. That speaker and alarm need the Find
  My owner's keys (its BLE chip; docs/case.md); the "non-owner sound"
  (FMNS `01 00 03` to 4F860003, or DULT `00 03` to 8E0C0001) answers only
  while the AirPods are in an Apple ID's Find My and separated from its
  devices (R, DULT draft; 9B47 NERF logs `non-owner command cannot be
  issued, near_owner`, C).
- The case's sound from a non-Apple host (Nov 14, static analysis of
  9B47 NERF and its UWB chip's `R1FW`, C unless marked):
  - NERF's state machine (transitions at 0x503f8) plays sound only from
    `CONNECTED`, reached only from the paired states (`WILD`,
    `NEAR_OWNER`); unpaired states handle no sound event. The owner's
    control point (0x3ca5c, Sound Start 0x200) answers unpaired with
    "not paired"; the non-owner one (0x3d040, 0x300) is refused near the
    owner and acted on only paired.
  - Pairing can't be done from Linux or Android: the pairing crypto runs
    on the UWB chip (`crypto_set_s3`, `crypto_val_s3_cert`), which checks
    S3 and a "BAA" certificate chain against Apple roots in its firmware
    (P-384 key at `R1FW` 0x3fb24). Whether Apple's servers or the owner
    device's Secure Enclave signs it is open; no open-source project
    pairs a genuine accessory.
  - Possible only after a one-time pairing with an Apple device: the
    non-owner sound from Linux once the owner device has been away past
    the separated timeout (no keys needed), or the owner's Sound Start
    with keys exported from a Mac and the link-key derivation
    reverse-engineered in `R1FW` (KDF labels at 0x42e69; G).
  - Removing the AirPods from an account may not unpair the case if the
    removal never reached it over BLE; a factory reset (case button,
    ~13.5 s, B2P 0x154 → `findMySmUnpair`) does. An unpaired case should
    advertise service data 0xFD44 when the lid opens (G; none seen
    Nov 13).
- Hot/cold: the buds' proximity advertisement resolves with the IRK
  calvados keeps, with its RSSI; it covers the case only while a bud is in
  it. An empty case advertises only Find My frames from the owner's
  rotating keys: unrecognisable without them.
- Precision Finding (UWB) uses owner-keyed sessions Android's UWB API
  can't speak; locating through Apple's network needs the owner keys and
  unofficial account APIs. Neither is pursued.
- A "left behind" alert is host logic: the session dropping while not
  stowed, or the advertisement unseen for a while.
