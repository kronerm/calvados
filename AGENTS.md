# AGENTS.md

Notes for developers and coding agents. End-user docs live in `README.md`;
the reasoning behind individual changes lives in commit messages, the
research behind the protocol in `docs/*.md`.

## Crates

| Crate | Role | Boundary |
|-------|------|----------|
| `protocol` | AACP decoding, framing, commands, state, settings, the engine | Pure Rust, sans-IO, fully unit-tested. No platform code. |
| `firmware-fetch` | Apple's firmware catalog and the update's package, for both platforms | Blocking I/O (ureq, zip); the platforms pick the thread and the cache dir. |
| `linux` (binary `calvados`) | BlueZ daemon (`bluer`, tokio, D-Bus via `zbus`) | Transport + effects only; all decisions come from `Engine`. |
| `android-app` (`libcalvados.so`) | Android foreground service (cdylib) + thin Java shell | Transport + effects + notification; decisions from `Engine`. |
| `android-hook` (`libcalvados_hook.so`) | Native hook in the system Bluetooth process: Apple's Device ID (`BTA_DmSetLocalDiRecord`), L2CAP channel modes (`l2c_fcr_chk_chan_modes`, forced to pass; Android 17's stack accepts them itself, it logs which), the remote suspend after a take (below). Symbols from `libbluetooth_jni.so`'s `.gnu_debugdata` (`Library`: `hook` and `resolve` take the function's pointer type; a missing symbol is a warning; only Vector's entry points, `native_init` and `on_library_loaded`, are exported) | `#![cfg(target_os = "android")]`; empty on the host. |
| `android-xposed` | Host build tool: the app via cargo-apk2, then the same APK made the Xposed module (the hook, the entry class as `classes2.dex`, `META-INF/xposed`; libxposed API 101+, targeting 102: Vector 2.1+; the compile-only API in `assets/libxposed-api.jar`), aligned and signed again with cargo-apk2's key | Shells out to cargo-apk2 and SDK tools. |

## Building

The root is a virtual manifest over host and Android crates: always name
the crate with `-p`. Host is the default target.

```sh
cargo build -p linux --release              # the daemon (or `makepkg -si`)
cargo app-build                             # app + module -> target/debug/apk/calvados.apk
cargo app-run                               # build, install, run on a device
cargo app-build --release                   # needs the signing env below
```

The app is built by [`cargo-apk2`](https://crates.io/crates/cargo-apk2)
(≥ 1.4.2, for 16 KB page-aligned APKs) from `[package.metadata.android]`,
both ABIs. It never removes stale output from `target/*/apk/`: delete it
after renaming or removing a Java class, or on `classes.dex … already
exists in archive`.

Prerequisites: `rust-toolchain.toml` pins stable and the Android targets;
the Android SDK and NDK (`ANDROID_HOME`, `ANDROID_NDK_ROOT`, the latter
defaulting to `/opt/android-ndk`, which both Android builds use for the
linker); `aapt2`, `javac`, `d8`, `zipalign`, `apksigner` and `keytool` on
`PATH` (and `adb` for `app-run`). Any JDK works: `.cargo/config.toml` sets
`JDK_JAVAC_OPTIONS=--release 17` for cargo-apk2's `javac` (the entry's build
passes the flag itself), since `d8` rejects class files from newer JDKs
(Java 27: "Unsupported class file major version 71"). It also names the
NDK's clang and `llvm-ar` (at `/opt/android-ndk`) for ring's C, which plain
`cargo clippy --target …-android` needs; cargo-apk2 sets its own.

Release signing: the key is private, never in the repo: the maintainer
keeps it outside the checkout (backup in a password manager), CI has it as
the secrets `ANDROID_KEYSTORE` (base64) and `ANDROID_KEYSTORE_PASSWORD`.
cargo-apk2 reads `CARGO_APK_RELEASE_KEYSTORE` /
`CARGO_APK_RELEASE_KEYSTORE_PASSWORD`, and the module step signs again with
the same (dev builds: cargo-apk2's `debug.keystore` in the Android user
home). Store and key share the password (cargo-apk2 passes one to
`apksigner`): `keytool -genkeypair -storetype PKCS12 -keystore … -alias
calvados -keyalg EC -groupname secp256r1 -sigalg SHA256withECDSA -validity
10000 -storepass … -keypass … -dname "CN=Calvados"` (EC: APK signing takes
it since API 24, the app needs 31). A new key changes the apps' signature:
uninstall the old builds first.

Versions come only from git tags (`tools/version.sh`): tag `v0.1.1` is
release `0.1.1`, `v0.1.1-alpha.2` pre-release `0.1.1alpha2`, other commits
`<last>.r<n>.g<hash>`. pacman's vercmp and Android's `versionCode` (an
integer: slot = major·10⁴ + minor·10² + patch; a release and the builds
after it slot·10⁴ + 5000 + n, its alpha k slot·10⁴ + k·100 + n) order them
the same: 0.1.0 < 0.1.0.r43.g… < 0.1.1alpha1 < 0.1.1. The PKGBUILD computes
`pkgver` when read (no `pkgver()`, which would rewrite the file); CI writes
it into the app's manifest, which keeps `versionCode 1` / `dev` for local
builds. `.github/workflows/release.yml` publishes a tag as a release
(alphas as pre-releases) and every push to main as the rolling pre-release
`testing`; a release is `git tag -a v0.2.0 && git push --tags`. In it: the
key reaches only the signing step (`apksigner` alone; the build signs with a
throwaway key); only the release job may write to the repo; GitHub's own
actions are pinned by commit; the runner's SDK, NDK and JDK are used;
Cargo's downloads, `target` and cargo-apk2 are cached per job, keyed on
`Cargo.lock` and the compiler.

## Protocol crate layout

- `aacp/` — one module per opcode: `OPCODE` constant + nom parser. `aacp::Packet`
  is the single decoded-packet enum; unknown or undecoded payloads become
  `Packet::Opaque`. Byte enums derive `num_enum::TryFromPrimitive`.
- `aacp/control_command.rs` — also `Cycle`, the modes press and hold
  cycles through: control `0x1A`, a bitmask (`ListeningModes`: off `01`,
  ANC `02`, transparency `04`, adaptive `08`; value `[mask, 0, 0, 0]`), two
  modes at least (the pods require it). The pods keep it but never send it
  (docs/firmware.md), so it's known only once a host sets it
  (`Engine::set_cycle`): shown as set, told the other hosts as
  `News::Cycle` (also hosts that join), carried over a reconnect like the
  noise mode, unknown after a restart, and forgotten when the firmware
  version changes (an update resets it). Off is never offered
  (`AllowOffOption`, `0x34`, isn't set).
- `aacp/crash_log.rs` — the pods' saved crash logs (docs/firmware.md, "Crash
  logs"): opcode 0x17 is `[class u32 LE] [len u16 LE]` + body, class 0x400
  from the pods, 0x200 a host's requests. A reboot leaves each crashed
  core's RTKit log in flash, announced ~10 s into every session
  (`CrashLog::Saved`, `06 00 00 [n]` + per core `[name 4] [size 4]`: `AP
  AOP RTP BTM DSP1 DSP2`); fetching doesn't clear them. The engine asks once
  per run for the same pods (`crash_logs_kept`, in memory), when UARP could
  be sent and no update runs (`crash_log::request`: type 5, both buds, sub
  1, byte for byte what the firmware builds to push them); keeps the parts
  per bud (`01 [n | 0x80 more] [bud | 0x80]` + data) until `ff` (`fc 01`:
  aborted); logs each `CLHE` record's exception and `Cstr` message; and
  hands the bytes out as `Effect::CrashLog`, saved as
  `crash-logs/<pods>-<unix time>-bud<n>.bin` in the state dir (Linux) or the
  app's files dir (Android): one 32 KB file per core, ~22 s from the bud
  holding the link, ~47 s relayed (Nov 6). A lone `ff` also ends a transfer
  the pods gave up on (a chunk that doesn't fit the channel, retried 6 ×
  500 ms); their chunk frames are 1002 bytes (2565 with the larger chunk),
  so Linux asks for a 4096-byte receive MTU (kernel default 672; Android's
  sockets get 8087, the pods 2582). **Never send another request type**
  (8/9 and 0x10-0x12 change the pods' NVRAM), **nor bud mask `00`** (it
  clears the announcement even with no logs sent).
- `protocol.rs` — framing (`Decoder` for byte streams, `parse_datagram` for
  message-preserving reads; both share `step()`), and command builders.
- `state.rs` — `DeviceState`, display-only state accumulated from packets.
- `engine.rs` — `Engine`: every decision, returning `Effect`s. Inputs: the
  session's lifecycle (`connected(pods, now)`, `disconnected(now)`),
  packets, local media/calls starting, nearby advertisements (`nearby`, with
  the stored keys), settings and noise-mode requests, the update's
  requests, and `tick(now)` at `next_wake()` for its timers (startup check,
  claim wait, stream check, redials, UARP settling, the update's timers). Time comes
  in as `Instant`s, so the platforms keep no timers of their own but the
  notification throttle and the attach rate limit. UARP and updates in it:
  below.
- `settings.rs` — `Setting`/`Settings`: AutoSwitch and AutoConnect, with
  the storage key both platforms use; stored by the platforms, obeyed by
  the engine.
- `display.rs` — what both platforms show, so they read the same: `Buds`
  (per component: level, charging, last known, place, primary), the
  ownership label, the noise modes' names (D-Bus `NoiseMode`, the Android
  actions), the cycle's names (D-Bus `Cycle`, the CLI), a tile's
  `CycleMark` and `toggle_cycle` (from all three while unknown, never below
  two), and the firmware and update lines.
- `ble.rs` — the proximity pairing advertisement: IRK resolution
  (`identify`, `resolves`), `Proximity` (lid and in-case bits, the lid bit
  holding only from a bud in the case; Inferred: the host count, byte 10
  bits 2-3), the scan filter `Pattern`, and `case_level` from the encrypted
  last 16 bytes (AES-128 with the pods' encryption key; byte 3, top bit
  charging; LibrePods). `Engine::nearby` writes that level into the state
  for our pods, also during a session; the status stays the pods' (last
  known while no bud is inside, when the advertisement knows better).
- `firmware.rs` — the running version and the catalog's `Release`.
  `Version::of_running`: 0x1D's `71.2256000021000000.6663` is major and a
  one-digit minor (7.1), then `<SourceVersion>.<BuildVersion>`
  (`BuildId`), no release number (7B19–7B21 all read 7.1). The release's
  package carries the same pair in its `version.plist` (9B47:
  `3431046004000000.29`; also its `bver`), which `firmware-fetch` reads with
  range requests at each catalog check (`Release::id`, a few KB of the
  zip). So `Firmware::older_than` (`Release::is_newer_than`,
  `update::check`) compares exactly when the version is known (UARP, or
  the build is the latest release's: `Firmware::runs`); another build of
  the same or an older major.minor counts as older (the pods refuse what
  isn't newer); else major.minor only.
  `DeviceState::firmware` (`Firmware`: model, 0x1D string, exact version)
  is carried over a reconnect like the name, never stored; the exact
  version is dropped only when the reported string changes. A session
  joining while another host is connected often gets no 0x1D (usually the
  phone): then only the exact version (its own query or a peer's
  `News::Firmware`), no model, so no catalog check. `display::firmware`:
  `7.1.21` when exact, else `7.1 (6663)`, else the raw string. The catalog
  side is `firmware-fetch`'s `catalog`: `url`, `models` (a product's models,
  served one first: AirPods Pro 2 USB-C asks A3048, the case A2968, then
  A3047, which gets a 403; an unknown model only itself) and `parse` (the
  `plist` crate; the newest of `Assets`: `FirmwareVersionMajor`/`Minor`/
  `Release`, `Build`).
- `uarp.rs` — UARP as a controller (docs/uarp.md), carried in AACP as
  opcode 0x4F (`[len 2 LE]` + one whole message, ≤ 1007 bytes, no fragment
  header; `Packet::Uarp`; Apple hosts' route, the pods' transport 0xd).
  Messages are `[type 2] [len 2] [id 2]`, big-endian. Out (`Message`): Sync,
  version discovery proposing 6, the buds' and the case's cap (the case
  takes offers only at 6), the offer 0x05, data response 0x07, rescind
  0x0C, the acks 0x0F and 0x18, the downstream relay 0x1B (`[id 2]` + a
  whole message), and the running-version query (tag 5: `[status 2] [tag 4]
  [len 4]`, four u32). In (`Received`): 0x04 as `RunningVersion`, 0x06,
  0x09 with `Processing` flags (low byte 1 staged, 2 denied, 3 abandoned, 4
  corrupt; the reason above), 0x0D, 0x17; the rest only logged. Data
  responses carry at most `MAX_DATA`. **No apply (0x0A) message exists;
  keep it that way.** The Python tools use the GATT characteristic
  (`94110001-…`, one-byte fragment header); calvados doesn't.
- `superbinary.rs` — `Package`: a `firmware.uarp` checked as the pods
  check it (format ≤ 4, header length 0x2c, binary length within the
  file, every metadata/payload range within it) and `FTAB`'s data against
  its metadata TLV `0xCCD28107` (SHA-256, `sha2`), before it's offered.
  `Package::offer` builds the offer from the header (version, binary
  length, payload count).
- `update.rs` — the update, sans-IO; the pods' side is docs/update.md,
  what a host must know docs/update-host.md. `Updater` (one per update):
  `up` (Sync, version, the offer: asset 1, tag 0, flags 1), `received`,
  `down`, `tick`, `cancel`, `running`; outputs `Output::Write` (a message)
  and `Output::Status` on a change. It serves exactly what's asked, cut to
  `MAX_DATA` (the pods ask on from there; an unknown asset or an offset
  past the end gets status 0x11), and acks processing notifications.
  `UpdateStatus`: `Downloading` (the engine's, while the platform fetches),
  `Offered`, `Deferred` (offer acked, nothing asked yet),
  `Transferring`/`Interrupted { percent }` (of `FTAB`'s data, by the pods'
  last request), `Staged`, `Done(version)` (the running version reached
  it), `Failed(Failure)`. Timers, since the pods have none of their own:
  nothing back within 10 s of the offer → `NoAnswer`; nothing asked for
  `WATCHDOG` (60 s) → rescind, `NotStarted` or `Stalled`. `down` (session
  dropped, primary changing) → `Interrupted`; `up` again re-offers the
  identical package, which the pods continue at their offset within their
  boot; more than 3 drops without a request between them, or 5 minutes
  down → `Lost`. `Staged` is the end: never an apply; with both buds in the
  closed case, the case copies the image to the other bud and applies both
  (~30 min). `check`: both buds in the case and the release newer than the
  running firmware, exactly when known, else by major.minor (equal is not
  newer; the pods refuse equal and older anyway).
  `Goal`: `Install`, or `ProbeCase` (`calvados firmware try-probe-case`,
  D-Bus `TryProbeCase`, Linux only): an offer that only holds the pods
  across a lid close, so the case tunnels through the primary as downstream
  1 and is asked its version; at most 10 % of `FTAB` served, never staged,
  withdrawn once the case answers or after 3 minutes (`Failure::NoCase`).
  It can only work with a case on 9A348 or later: older cases never
  tunnel (switched off in their flash; docs/case.md), and only their Find
  My owner's Apple device can update them. When nothing newer than the
  running firmware exists, the probe offers the latest as the next release
  (`Package::bumped`: the header's release + 1, and `FTAB`'s own last
  version word + 1, since the buds compare both against what runs); it
  can't stage (completion requires the image's `bver` to match the offer)
  and leaves only the inactive slot invalid, as any partial transfer.
- `news.rs` — `News`, calvados to calvados: `calvados` + format 1 + `1`
  exact firmware (3 × u32 BE), `2` + update state (two arguments, a
  version), or `3` + the cycle's mask; carried by smart routing
  (`protocol::news`: 0x10 to a host, `[mac] [len 2 LE]` + news, arriving as
  0x11 `Routing::News`). Apple's frames start `01 e5`. Untested: whether
  Apple hosts ignore ours, and whether the pods relay it unchanged.
- `connection.rs` — L2CAP PSM and service UUID, and `is_airpods_name`
  (a paired device named `AirPods…`, any case), both platforms' test.

### UARP and updates in the engine

- **A 0x4F frame reaching a bud that isn't primary aborts it** [rkof
  0xbea70], so nothing UARP goes out until `uarp_ready`: startup
  `Answering`, a live battery, the primary known, and `UARP_SETTLE` (5 s)
  past the connect or the last primary change (the session's `uarp.hold`).
- The pods serve UARP over AACP to one host per link, the first to send
  any, until that host's link drops (docs/uarp.md, "One host at a time"):
  a second host gets no answer at all, hence `Failure::NoAnswer`. So a
  host that isn't updating leaves UARP alone: the exact version comes from
  0x1D's build matching the catalog's latest release
  (`Engine::release_known`, `identify`; see `firmware.rs`), and a settled
  session asks it over UARP (`VersionQuery`) only when no 0x1D came and
  it's unknown (`Engine::firmware_known`, in memory), or when its staged
  update waits for the new version; the answer goes through
  `running_firmware`.
- The pods announce the other bud as downstream (0x17, acked with 0x18),
  and the case as downstream 1 while it tunnels through the primary (only
  across a lid close with an offer held, and only from a case on 90.x;
  docs/case.md). The case is then asked its version through the relay;
  answers are logged (`UARP: the case's firmware`, `UARP: from
  downstream`). The pods' own offers (a dynamic `MTIC` asset, usage
  metrics) go unanswered; nothing is solicited. **Never offer the pods an
  asset tagged `MTIC`, `LOGS` or `CRSH`**: that path aborts them [rkof
  0xbcd82, 0xbcdca].
- `prepare_update(release)` refuses with a `Refusal` (not connected, no
  release, already running, another host's update running, or
  `update::check`'s). An actionable refusal (not connected, not in the
  case, version unknown) becomes the status, `Failed(Refused)`, unless an
  update runs, and isn't told to peers. Otherwise → `Downloading`; the
  platform fetches and hands `package(Option<Package>)`, checked again (the
  release's version, the preconditions), then offered once settled. A done
  or failed update is dropped at the next session, so the firmware line
  shows again.
- The update survives a session drop with the same pods (`disconnected`
  takes it down, the next settled session offers it again) and waits out a
  primary change the same way. `cancel_update` rescinds.
- Peers: every status change goes to the other hosts (`DeviceState::hosts`)
  as `News::Update`, progress in 5 % steps; the exact firmware from our own
  query as `News::Firmware`, also to hosts that join (a `ConnectedDevices`
  list). Received `News::Firmware` sets the exact version; `News::Update`
  is `DeviceState::peer_update` (cleared when that host leaves).
  `display::update` / `display::peer_update` are the lines.

### How sure we are

- *Known*: from LibrePods and confirmed by our captures or by commands
  working (battery, ear detection, control commands, smart routing, device
  information, audio source, connected devices).
- *Observed*: our own, the structure holding across captures (framing
  lengths, `DeviceConnection` roles against known MACs, `PrimaryBud` at
  switches, the `0x2B` layout).
- *Inferred*: a meaning guessed from patterns; doc comments say
  "Inferred:" (history kinds and clock, `PrimaryBud::active` and `peer`,
  everything about the opaque packets below beyond their bytes).

### Opaque packets (decoded no further)

From captures; meanings beyond the structure are guesses.

- `0x02`, after Request Notifications: a count (`1d` = 29), then that many
  `(id, value)` entries, mostly one byte each (`0a 01`, `50 02`), but ids
  `03 04 06 07 30` carry a 4-byte LE value. Length not encoded, so the frame
  ends at the next magic. A fixed capability table: identical in every
  session Nov 4-6, no state in it.
- `0x17`: `[class u32 LE] [len 2 LE]` + body. Class 0x400 is decoded
  (`aacp/crash_log.rs`); class 0x100000 (HID): protobuf (`08` sequence
  number, `10` 1/3, `2a` nested message) wrapping an Apple
  binary-serialized dictionary (OSSerializeBinary, signature `d3 00 00
  00`): `VendorID` = `0x05ac`, `AccessoryService`, `MaxReportSize`,
  `devmot`…, in the burst after a primary switch.
- `0x4E`: 8 zero bytes, once per session in the startup burst: the NVRAM
  value `fprox` [rkof 0xa628c, sent 0x85914].
- `0x52`: `03 00 02 01 [00|01]`, from 0xd16ec through an interface table.
  `01` next to `AudioSource` changes; `00` next to ear changes, before them
  11 times and after 26: no warning of a bud leaving an ear.
- Case charging sounds (LibrePods: `12 3A 00 01 00 08 [00 on / 01 off]`) are
  an ATT Write Request on PSM 0x1F, not AACP: handle 0x003A, value
  `01 00 08 [x]`, the Find My service's characteristic `4F860005` (handles
  inferred from rkof's GATT definitions; handler 0x68fa4). A fixed setting,
  not a relay to the case.

`0x2B` (connection history), used to infer the local MAC and as a source of
handoff peers (hosts whose latest event is a connect): `01 [len 2]
[now 8]`, then 17-byte events `[time 8] [mac 6, reversed] [kind] [value 2]`:
hosts and the other bud connecting and leaving, the pods paging a host after
power-on (`LinkChange`). Its length caps at 255, i.e. 15 events.

## Android crate layout (`crates/android-app/src`)

- `lib.rs` — tracing: logcat (debug) plus daily files in `files/logs`,
  14 kept (~0.5 MB a day; debug and the `aacp_raw` trace of every frame
  received and sent), to look back at a reboot.
- `exports.rs` — the JNI entry points of `PersistentService.java`.
- `service.rs` — the event loop: one channel of typed `Event`s (the ACL
  up or down, Bluetooth on, a notification `Request`, nearby advertisers,
  link events, stop) driving the `Engine`; effects out. MACs are
  `MacAddress` from the JNI boundary on. Every session that opens also
  connects A2DP (a no-op when up): Android 17 opens no profile over a link
  the pods paged, and the pods open none while another host is connected
  (Nov 13: 92 s with only SDP and AACP). Also the update's I/O through
  `firmware-fetch`, as on Linux: the catalog checked once a day per product
  (`Catalog::due`), the package fetched on `UPDATE_FIRMWARE`, each on a
  thread posting `Event::Catalog` / `Event::Package`; the package cached as
  `<build>.uarp` in `getCacheDir`. Hence `INTERNET`, the app's only network
  use.
- `link.rs` — the AACP L2CAP socket (connect / send / read thread),
  through a hidden `BluetoothSocket` constructor by reflection, trying each
  known argument order (Android 17 takes the `BluetoothAdapter` first).
- `bluetooth.rs` — paired AirPods, local MAC, A2DP routing, and the
  presence scan: a `BluetoothLeScanner` scan (`ble::Pattern`: Apple
  manufacturer data `07`, lid open, since pods charging in the closed case
  advertise too; balanced, controller-filtered, `CALLBACK_TYPE_FIRST_MATCH`:
  low power found the pods 19 s after the laptop, screen off) delivering
  `NEARBY` intents through a `PendingIntent` (no Java; the system scans
  without the app running). A Bluetooth toggle ends the scan, so the
  service starts it again on `BluetoothAdapter.ACTION_STATE_CHANGED` to
  `STATE_ON` (listed in the receiver's filter in `PersistentService.java`);
  without that the phone once never noticed the pods again (Oct 18). While
  the pods' ACL is down, the engine connects the pods an advertiser
  resolves to with their stored IRK (preferences `identity_key:<MAC>`,
  `encryption_key:<MAC>`), as on Linux; `Effect::ConnectPods` is an A2DP
  connect. The scan permission is `neverForLocation`. Android builds
  first-match results from the controller's tracking event (vendor 0xFF,
  subevent 0x56) without the advertisement's data, so the phone gets no
  case level from it. Don't add a batched scan of every in-case match: one
  (Oct 8-18) got every worn AirPods nearby and woke the phone every ~5.5 s
  (and a `PendingIntent` scan outlives the app: stop it by request code).
- `intents.rs` — action names, intents to the service, and `parse`, which
  turns an incoming one into an `Event`. The notification's buttons are a
  `Request` each (`action`, `pending_intent`, and parsed back), so the
  action strings live only here.
- `notification.rs` — channel, `startForeground`, and the notification: a
  `DecoratedCustomViewStyle` with two `RemoteViews` layouts from `res/`
  (collapsed: title, status and levels like the Linux bar; expanded: the
  Linux panel's batteries, noise tiles and settings rows), filled by view
  id. There's no generated `R` class, so ids come from
  `Resources.getIdentifier`. Tiles and rows fire service intents
  (`NOISE:<mode>`, `AUTO_SWITCH`, `AUTO_CONNECT`); a tile's cycle dot is
  its own view over the tile's corner (a `FrameLayout`, adding no height)
  with its own intent, `CYCLE:<mode>`. The header's subtext is one line
  (`header`): this phone's update, a refusal included (`display::update`),
  else a peer's (`display::peer_update`), else the firmware and the newer
  release. The update's start and cancel are a `Notification.Action`
  (`UPDATE_FIRMWARE`, `CANCEL_UPDATE`; no icon, as AndroidX builds them),
  shown under the custom view, outside its height limit, where a row in the
  layout would be cut off. Bold text is a `Spanned` from `Html.fromHtml`.
  RemoteViews allow only some views (no `Space`), and an id must be a valid
  Java name (no `case`).
- `res/` (crate root) — the layouts and the icons: Tabler SVGs converted to
  VectorDrawables. Every color (text included) is one of five
  `notification_*` colors: the system's dynamic palette (`system_neutral1_*`,
  `system_accent1_*`…, hence Android 12 as the minimum), each with a
  `-night` variant; theme attributes resolve against the app's light
  theme, unreadable in a dark shade. `ic_stat_airpods` is the status bar
  icon.
- `media.rs` — pause/resume as media button presses.
- `jni/` — typed wrappers mirroring Java packages, plus `with_env` and
  `string` (a call's `String` result, checked, null as `None`).

## Linux crate layout (`crates/linux/src`)

- `main.rs` — setup, the shared `Daemon` context, and one loop owning the
  `Engine`: the AACP connection (`Aacp`: attaching, kept across
  iterations so no L2CAP connect is cut, or up; `down` attaches again after
  a pause), the session's datagrams, the engine's `next_wake`, MPRIS, and
  `Input`s from D-Bus and presence. No shared mutable state besides the
  published `Status`.
- `bluez.rs` — Apple DID record, finding the pods, L2CAP, A2DP.
- `dbus.rs` — the `dev.kroner.Calvados1` object and its `Status`, published
  through a `watch` channel; a task emits `PropertiesChanged` for every
  changed property, the writable ones too (the cycle also changes from
  peers). Writes go to the loop as `Input`s; a setting's setter (and
  `Cycle`'s) waits until the status shows it, so zbus' own changed signal,
  sent after the setter, is current too.
- `mpris.rs` — pause/resume of MPRIS players on one ordered worker task.
- `presence.rs` — reports Apple advertisers to the loop (`Input::Nearby`;
  `Engine::nearby` decides), since the pods page out only while no host is
  connected, and then one host (docs/paging.md). BlueZ's advertisement
  monitor (`Experimental = true` in `main.conf`; without it one info line,
  no monitor) reports proximity pairing advertisements (manufacturer data
  `4c 00 07`, filtered by the controller); one whose rotating address
  resolves with a stored IRK is a pair of ours, connected unless a session
  is up or they're put away (`ble::Proximity`: lid closed and the
  advertising bud in the case), once per 3 s (both buds advertise). The
  monitor reports a device once, so each one's `ManufacturerData` is
  followed until BlueZ forgets it (the lid opening changes it). Our pods'
  advertisements are logged (`our pods advertising`) with their host
  count, to confirm it before presence relies on it. Keys: every session
  requests both (`0x30`, mask `0x05`: IRK `0x01`, encryption key `0x04`;
  reply `0x31`, an `Effect::LearnedKey` each), stored as
  `$XDG_STATE_HOME/calvados/<address>.irk` and `.enc`. A fresh link starts
  with the profile `off`, so connecting never takes the audio. The monitor
  starts after the D-Bus name is claimed (single instance).
- `firmware.rs` — runs `firmware-fetch` on blocking tasks. The catalog
  (`https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_<MODEL>/com_apple_MobileAsset_UARP_<MODEL>.xml`,
  trying `catalog::models` in order until one answers) is checked once a
  day per product, in memory (ureq: rustls, ring, bundled roots); the
  result returns as `Input::Catalog` (a failed check keeps the last
  answer). The package: the release's zip (`__BaseURL` + `__RelativePath`),
  `AssetData/Firmware.acsw/firmware.uarp` read with `zip` (pure Rust
  inflate), checked (`Package::parse`, the release's version) and cached as
  `$XDG_CACHE_HOME/calvados/<build>.uarp` (written aside, renamed), reused
  while it checks out; `Input::Package`. D-Bus: `Firmware` (`7.1.21`, or
  `7.1 (6663)` without UARP), `LatestFirmware` (`9.0.348 (9A348)`, empty
  until checked), `FirmwareUpdate`, `StartUpdate` (fails with the
  refusal's text; the package is fetched after it returns), `CancelUpdate`,
  `TryProbeCase`, `Update` (`(state, percent, detail)`) and `PeerUpdate`
  (the host first).
- `cli.rs` — `calvados watch` / `set …` / `firmware
  status|update|cancel|try-probe-case`: a zbus proxy client of the daemon's
  own D-Bus object (no daemon logic), for shells without D-Bus. `firmware
  update` follows `Update` until staged, then says to close the lid
  (Ctrl-C leaves the update running). `watch` prints JSON lines on
  `PropertiesChanged` and on the bus name's owner changes: the D-Bus
  properties, as data (the plugin does its own labels). `set cycle toggle
  <mode>` is `display::toggle_cycle` against the daemon's `Cycle`, for the
  panel's dots.
- `dist/noctalia/` — Noctalia (v5, Luau) plugin, installed to
  `/usr/share/calvados/noctalia`: a `[[service]]` holds one `runStream` of
  `calvados watch` and republishes it via `noctalia.state`; the bar widget
  and the attached panel only watch that, and act through
  `noctalia.runAsync({"calvados", "set", …})`. Noctalia plugins have no
  D-Bus API. Noctalia scans only configured sources and
  `~/.local/share/noctalia/plugins` (no system dir; `plugin_manager.cpp`),
  so users symlink the installed copy there (README). Scripts hot-reload; a
  panel's `width`/`height` apply only when Noctalia rebuilds its panels (a
  `[plugins]` config change, i.e. disable + enable, or a restart), not on
  `config-reload`. `noctalia plugins lint crates/linux/dist/noctalia`
  checks it, but not `plugin_api` against the installed Noctalia: 5.0.1
  supports 3–30 (its log names the range when it refuses a plugin), while
  the published API definitions run ahead (container tooltips are API 32).
  The plugin needs 24 (`runAsync` with an argv table).
- `state.rs` — the state directory, the pods' crash logs, and the two
  remembered settings (`protocol::settings`; files
  `auto_switch`/`auto_connect`, `on`/`off`, default on), the same two as
  the Android notification's Switch and Connect buttons (preferences under
  the same keys).
- `hci.rs` — the pods' traffic from the kernel's HCI monitor channel (raw
  `AF_BLUETOOTH` socket, `HCI_CHANNEL_MONITOR`, as `btmon`; needs
  `CAP_NET_RAW`, else one info line and nothing), decoded on its own thread
  into `hci:` debug lines: links (paging, up, down with reason and how long
  the pods had been silent, which dates a crash: a timeout follows only the
  supervision timeout's silence), role, sniff, AVDTP signaling, AVRCP
  passthroughs, A2DP media starting to flow, delivery (sent packets against
  the controller's completed-packet credits: gaps of 0.5 s or more with
  packets outstanding, what's undelivered at a link's end, flushes; to see
  the pods going out of reach, as on Oct 26), L2CAP channels opening,
  refused and closing (PSM, which side), and every AACP frame both ways, in
  full. Channels are tracked from signaling on the link our AACP channel
  opens on (and later links to that address from their start), so channels
  opened before the daemon started (A2DP's) stay unknown until reopened.
- `pipewire.rs` — the pods' card profile, set natively (`pipewire` crate,
  on its own thread and main loop): `off` as soon as BlueZ reports them
  connected (no AACP needed) and on `Effect::ReleaseAudio`; `a2dp-sink` on
  `Effect::RouteAudio`. With `off` the laptop keeps the link but can't pull
  the audio from the phone. `off` is set with `save`, `a2dp-sink` without,
  so WirePlumber remembers `off` and later connects start that way. The
  `bluez_card.*` devices' profile params are subscribed (the active profile,
  not the list), and the list (with availability) is enumerated whenever
  they change; for 10 s after a request every change is corrected (the
  card appears after BlueZ connects, then WirePlumber restores a remembered
  profile), and after that the requested profile is selected again whenever
  it becomes available again (a BlueZ-aborted start once left the card on
  `off` for good). On `Effect::RestartStream` (the stream check, below) the
  `bluez_output.*` node gets a `Suspend` command and, streams being linked,
  starts again at once.
- `PKGBUILD` (repo root) — Arch package built from the checkout (binary,
  the unit with `ExecStart` rewritten to `/usr/bin`, the Noctalia plugin);
  no `check()` (run the tests below yourself); `!lto`, since makepkg's LTO
  turns libspa's C shim into bitcode rust-lld can't link. The install
  script sets `cap_net_raw+ep` on the binary (user units can't grant
  capabilities; the unit must not set `NoNewPrivileges=`).
- `dist/calvados.service` — systemd user unit. The daemon never powers the
  adapter (it pages the pods only for `AutoConnect`, see the ownership
  model), waits for BlueZ's `Connected` property signal (no polling;
  re-finds paired AirPods each cycle), and refuses to start twice (D-Bus
  name requested with `DoNotQueue`). The Apple DID comes from `DeviceID` in
  `/etc/bluetooth/main.conf` (required, from boot); the daemon only warns
  when it's missing.

## Keeping the two UIs alike

The data comes from `protocol::display` on both; the layouts and their
texts are written twice. A change to one side needs the other:

- the panel and bar (`crates/linux/dist/noctalia/panel.luau`, `bar.luau`,
  `lib/format.luau`) against the notification
  (`crates/android-app/res/layout/notification_*.xml`, `src/notification.rs`);
- the noise modes' labels and Tabler glyphs (`format.MODES`, the tiles in
  `notification_expanded.xml` and `notification.rs` `MODES`);
- the settings' titles and descriptions ("this computer" / "this phone");
- the noise heading ("· Off", "· unknown yet");
- the cycle dots (`format.cycleDot`/`cycleToggles`, `notification.rs`
  `cycle_dot`): Tabler `point-filled` when in, `point` when out, the
  filled one at 0.4 while unknown, in the tile's text color; the dot's
  click toggles the mode, the tile's sets it. Noctalia takes
  `onRightClick` only on buttons, so a tile has no right-click;
- the firmware ("Firmware " + `display::firmware`: the panel's last line,
  `format.firmware`; the notification's header subtext,
  `notification.rs` `header`, since the expanded layout has no
  height to spare; " · 9.0.348 available" once the catalog has a newer one);
- the update's line (`display::update`, `format.update`: "Updating
  firmware… 40 %", "Staged: close the lid", …, a peer's with " · <host>";
  in the panel it takes the firmware line's place);
- the update's button ("Update" when a newer release is known and no host
  updates, "Cancel" while this host's runs: `format.updateAction` and the
  notification's action, `UpdateAction`, which says "Update firmware" and
  "Cancel update" since it stands apart from the firmware line); the UI
  stays the panel and the notification, no windows or activities;
- the battery glyph thresholds 88/63/38/13 (`format.batteryGlyph`,
  `notification.rs` `battery_glyph`);
- the icons: Noctalia names Tabler glyphs, Android has them converted in
  `res/drawable` (SVG paths, stroke 2, round caps and joins).

## Ownership model

The firmware's side (the arbiter, `AudioSource`, the A2DP switch):
docs/ownership.md; which host the pods page: docs/paging.md; the startup
burst: docs/session-start.md.

- **Connecting.** One long-lived AACP connection per platform, opened only
  over an ACL link the OS already has. The pods are paged (an OS-level
  connect, never a bare AACP socket) only with `AutoConnect` on: when their
  advertisement shows up (presence), and after a session drops unless both
  buds were put away (`DeviceState::is_stowed`: in the case, or gone
  without a live battery reading, or still charging; "not in an ear" was
  too strict, a bud just taken out hasn't reported its ear yet), 3 times 3
  s apart (`Engine::disconnected` and `tick`). Needed because the pods page
  only while no host is connected, and only one host (their stored
  preferred, `bvi05`, or the most recently connected, never one
  disconnected on purpose, 0x13-0x16, the lid-close drop included, until it
  connects again); BlueZ retries only after a timeout (0x08), not after a
  primary handover (0x15); and the BLE monitor reports an advertiser only
  when first found.
- **Taking audio** happens only automatically: media or a call starting
  here (automatic switching, below; there's no manual take), or when this
  host is the only one connected (`DeviceState::hosts`) and a bud is out of
  the case (not `is_stowed`; pods connected while charging once took the
  audio from other headphones). The latter is checked at connect, when a
  bud comes out, at the first `ConnectedDevices` list (~0.2 s after the
  handshake), and when another host leaves; it sends nothing to the
  firmware (no peer to hand off from), sets `Owning` and emits
  `RouteAudio`.
- **Who's connected.** A session joining pods that already have hosts gets
  `DeviceConnection` (0x0C; role 1 = host, 2 = bud) for every connected
  device *before* the list, so hosts stay unknown until the list; on a
  fresh connect the list can come first. Mid-session, a leaving host comes
  as `DeviceConnection` (`connected = false`), a joining one as a new list.
  A list also comes in the burst after a primary switch (`PrimaryBud`,
  `DeviceConnection` for the buds and this host, `AudioSource`,
  `ConnectedDevices`), and once right after a host left.
- **`Ownership`**: `Idle` → `Claiming(Handoff::{Pending, Sent})` on a take
  → `Owning(_)` when the firmware confirms (`OwnsConnection = 1`, or no
  answer within `CLAIM_WAIT`, 1 s; confirmations took 73-437 ms). Media or
  a call starting again while unconfirmed sends the handoff again (the pods
  once didn't answer at all, Oct 9). `AudioSource` moves it too: our MAC
  streaming → `Owning`; another device streaming while `Owning` → `Idle`
  plus `ReleaseAudio` (pause), sending nothing. The pods switch to any host
  that starts streaming A2DP, even while the other plays, ~30 ms before
  revoking (`OwnsConnection = 0`).
- **The firmware's arbiter**: one record per host; the last host to write 1
  wins at once (outranking a streaming host), writing 0 hands it to the
  other, and the pods send `OwnsConnection` only when the owner changes (1
  to the new, 0 to the old); nothing there touches A2DP. So after another
  host streamed us away the pods still count us as owner and a claim gets
  no answer: the engine remembers what they last said (`PodsOwner`) and
  routes at once then. The pods also pick an owner unannounced (adding or
  removing a host's record re-chooses [rkof 0xd8618, 0xd87a0], and `09 06
  01` goes only to hosts whose AACP session is open, so a host that
  connected first owns them untold), hence an unanswered claim counts as
  confirmed after `CLAIM_WAIT`. An `OwnsConnection = 1` we didn't ask for
  (a host joined or left, the buds put away) only sets `PodsOwner`.
- Smart-routing frames (0x10) are relayed verbatim as 0x11, never parsed;
  0x11 carries its length like 0x10 and completes at once (as an
  unknown-length frame it waited for the next magic, 11.5 s once on
  Android). `AudioSource` (0x0E) is the last host to start something plus
  one activity mask over all hosts (`Activity`: 0x01 call, any SCO; 0x02
  media, A2DP; other bits unnamed), so one host stopping can report nothing
  playing while the other streams.
- **Session start.** The state half of the startup burst (battery, ear
  detection, `PrimaryBud`, `DeviceConnection`s, `AudioSource`,
  `ConnectedDevices`) answers each notification request at once; the
  settings half (the 0x02 reply, `DeviceInformation`, settings, EQ) comes
  once the pods count the host connected, which nothing can solicit: with
  another host connected they wait for the profiles this one advertises,
  20 s, then up to 3 × 5 s (24 s in one capture). Only the handshake goes
  out at connect; the rest of the setup (feature flags `0x4D`,
  notifications, EQ) on its ack, since a late ack (~0.6 s) lost what was
  sent with it, and without the feature flags the pods reported Adaptive
  as Off to that host. A re-sent handshake on the same channel gets 0x85
  (already open). `Engine::check_startup` (2 s after the handshake, again
  every 2 s while it asks for something): no ack → a new AACP connection
  (`Effect::Reconnect`; a session opened as the pods finish booting can
  stay silent, a new channel was acked at once); acked but no battery →
  re-request notifications, 3 times at most, then reconnect if nothing at
  all came after the ack (`Phase::Acked` vs `Answering`; once the pods
  went silent for 53 min, Oct 18). Without the settings half the session
  still works: battery, ears and lists come with later changes; auto-take
  waits for a known `AudioSource`; the history names the other hosts.
- **Automatic switching** (`Engine::local_audio`): the platforms report
  what plays (silent, media, a call over media) and the engine finds the
  starts. Media starting while `Idle` takes the audio, like Apple devices,
  except while the other host is on a call (`AudioSource` kind `Call`),
  while no `AudioSource` is known, or while both buds are in the case
  (`DeviceState::in_case`; calls too). A call takes regardless of what
  plays elsewhere. A bud going into an ear while something plays here and
  we don't hold the audio takes it too (`Engine::ear_take`): a call wins
  over media; with the same on both hosts the `PodsOwner` takes, so only
  one does. Both buds going into the case while holding the audio
  releases it here only (Linux: profile `off`, so the speakers play),
  writing nothing: an `OwnsConnection = 0` made the other host the owner,
  and the pods then paged that host when they came out (Nov 6). Sources:
  Linux, MPRIS `PlaybackStatus` → `Playing`; Android, the
  `AudioPlaybackCallback` (`USAGE_MEDIA` media,
  `USAGE_VOICE_COMMUNICATION` a VoIP call) and the phone-state broadcast
  (`OFFHOOK`; ringing doesn't take); rising edges only (`service.rs`
  `LocalAudio`). Hosts arbitrate, not the pods: a host's
  `media_information` is only relayed to the peer (`PeerMedia`, logged as
  "peer reports its playback"). A peer's takeover arrives as `PeerMedia`,
  `ShowNearbyUi`, `YieldRequest`; only the last (and `OwnsConnection = 0`)
  releases.
- Ownership is asserted to the firmware (`set_ownership(true)`) only
  together with the targeted handoff sequence to a known peer, never as a
  bare claim.
- **Releasing.** Yield requests and `OwnsConnection = 0` release once
  (`Effect::ReleaseAudio`, nothing to the pods: the claimant's 1 lands
  first, so a 0 would change nothing, except handing an Apple host that
  took over with 0x44 the ownership for good); repeats do nothing. A yield
  request always emits `ReleaseAudio`, even when not holding the audio: a
  host playing without a take (app just restarted) otherwise has its
  stream restarted by the OS ~4 s later and the pods switch back to it. A
  declined handoff also emits `ReleaseAudio`. Both platforms then pause,
  like iOS/macOS: Linux turns the profile off and pauses its MPRIS players
  (otherwise playback falls back to the speakers); Android sends a pause
  key, as the pods do too.
- **Android after a take.** When another host took over, the pods remotely
  suspended our A2DP stream, and `btif_av` then refuses every start from
  the audio HAL (`kFlagRemoteSuspend`) until a local suspend or the
  remote's AVRCP PLAY, which the pods never send (AOSP `btif_av.cc`). The
  hook clears it: it watches L2CAP sends (`l2cap::Impl::L2CA_DataWrite` on
  Android 17, else the free `L2CA_DataWrite`) for our AACP `OwnsConnection
  = 1` claim and calls `btif_av_clear_remote_suspend_flag(kSource)` and
  `btif_av_stream_start_with_latency(false)` (the Pixel HAL doesn't ask
  again by itself); takes back to the phone are then immediate (Android 17,
  Pixel 9). Not covered: a take without a claim (`PodsOwner` already us).
  The older `AudioManager.setParameters("A2dpSuspended=true/false")` toggle
  on `RouteAudio` (200 ms apart; a local suspend on Android 16) is ignored
  by Android 17's Pixel HAL; it doesn't run during a call (Android keeps
  A2DP suspended while SCO is up, and our `false` would undo that), and
  toggling on release doesn't help (the pods refuse the restart and the
  HAL blocks toggles for ~5 s). A paused Android player leaves its stream
  started; the pods suspend it only when they switch, and not within ~5 s
  of switching to this host. Android ignores the stream check.
- **Stream check (Linux).** 1 s after a confirmed take
  `Engine::check_stream` (through `tick`) emits `RestartStream` unless the
  pods name us the media source, then every second, 10 times at most.
  Healthy takes stream within ~0.7 s and are left alone (restarting every
  take cut the running stream). The pods never refuse a second host's
  START (they defer it ~0.7 s while the other streams) and have no guard
  timer; the restarts that went unanswered on 9A348 put no AVDTP START on
  the air. A restart right at a take rebooted the pods once.
- **Ear rule**, the only way the engine touches media playback: a bud
  leaving an ear while this host is the media source (`AudioSource` = our
  MAC, `Media`) emits `Effect::Pause`; both back in emits `Effect::Resume`
  if we paused; both out forgets the pause. The pause lives in
  `Ownership::Owning(Playback::EarPaused)`, so leaving `Owning` forgets it
  too. Calls are never paused. Android sends media keys (`media.rs`), Linux
  pauses/resumes MPRIS players (`mpris.rs`).
- **The local MAC** is needed for the peer handoff (`engine::LocalMac`):
  - *inferred* at connect, before the first device list: from the history's
    newest event when it's a host connecting at most 2 s ago (a fresh
    connect), or from the session's first `DeviceConnection` (host,
    connected), which is the new connection itself when joining pods
    already connected. Dropped if the device list doesn't contain it; used,
    not persisted;
  - *known* from the pods' `AudioSource` reply while nothing plays: it
    names the host that asked (kind 0, our MAC; [rkof 0x875aa-0x875ce]; 107
    of 107 on the laptop, 3 of 3 on the phone), while a change to nothing playing names nobody (zero
    MAC);
  - *known* when saved (apps can't read the adapter's address,
    `LOCAL_MAC_ADDRESS` being privileged-only; Linux has it from BlueZ) or
    once it's the only host connected (`DeviceState::hosts` has one entry);
    then persisted (`Effect::LearnedLocalMac`).
  Without it, a take only routes audio (no claim that could never complete).

## Firmware updates

Where: `protocol::{superbinary, uarp, update}` and the engine (UARP over
the AACP session); `firmware-fetch` checks the catalog and fetches and
caches the package for both platforms; Linux exposes the update on D-Bus,
the CLI and the panel, Android in the notification (its header and an
action). The pods' side: docs/update.md, docs/update-host.md,
docs/sibling-and-case.md; the case: docs/case.md.

The flow: offer → the primary pulls the SuperBinary header, the payload
headers, `FTAB`'s metadata and `FTAB` (~7.8 MB) → "fully staged" → the lid
closes → the case copies to the other bud and applies (~30 min) → the
next session's version query shows it (`Done`). Done over AACP on Nov 14
(9A348 → 9B47, laptop): offer to staged in 5 minutes (8604 requests of
971 bytes, ~26 KB/s, no drop); applied by the case and running 30 minutes later (`Done`); with the
lid closed the primary let the link go 30 s in (probes held a first close 77 s and 2m20s; why it varies is open). Before that only over GATT with
a Python tool, since removed (Oct 27, ~8 minutes; docs/uarp.md).

Rules:
- never an apply (no 0x0A message exists), never a solicitation; only
  the offer, data responses, rescinds and acks;
- preconditions (`update::check`): both buds in the case, the release
  newer than what runs; power is the pods' call (they defer until the case
  charges);
- no UARP unless settled and the primary isn't changing (see the engine);
- the watchdog rescinds after 60 s without a request;
- a drop resumes by re-offering the identical package; a staged image
  stays (nothing rescinds it);
- other calvados hosts learn the progress through `News`, and an update
  isn't started while one of them reports one running;
- only the buds: the case's payload is never offered. A case on 7B21 is
  updated only by its Find My owner's Apple device (docs/case.md).

Untested: `News` through the pods' relay; `ProbeCase` against a case on
90.x.

## Debugging a reboot

What there is to read after one, and for how long:
- Linux: the daemon's journal (weeks): decisions, packets, `hci:` lines.
- Android: the app's daily logs (14 days), every AACP frame both ways
  (`adb shell "su -c 'cat /data/data/dev.kroner.calvados/files/logs/…'"`).
- Android: the HCI snoop log (Developer options, "Enabled", i.e. full), the
  most complete source, `/data/misc/bluetooth/logs/btsnoop_hci.log` and
  `.last`, read with `btmon -r`. It rotates every
  `persist.bluetooth.btsnoopsize` packets (default 65 536, ~1-2 days; set
  to 1 048 576 on the test phone, ~2-3 weeks) and at a Bluetooth restart.
- The pods' own crash logs, fetched at the next session that gets their
  announcement (`aacp/crash_log.rs`): "pod crash log" lines and
  `crash-logs/` files.
- Their last packet to a host, plus that host's supervision timeout (5 s on
  the phone), dates it.

## Pod reboots

Seen on 7.1.21 (7B21) and, once so far, on 9A348 (Nov 4): ~3 s into a
phone take's stream the pods sent the phone an AVRCP PAUSE, the phone
played again 0.24 s later, and the pods went silent within ~2 s and
rebooted. Nothing from calvados in between. Its crash log (fetched Nov 6)
says `ASSERTION FAILED: stream_sync_a2dp.cpp:429`, task `AudioMgr`
(docs/firmware.md): a started A2DP stream whose audio clock never
anchored; 3 s after the start the pods send the source a PAUSE, wait 2.5 s
more, then assert. A started but silent stream is the leading guess (the
hook had started it 3 ms after the claim); why the clock didn't anchor is
open. Earlier reboots' logs are overwritten, so whether they were the same
is unproven; the stream check's restart right at a take fits.

Rapid takeovers (a laptop take within ~6 s of a phone take) occasionally
reboot the pods mid-switch: after they pause the old host, before they
suspend its stream; nothing in AACP warns first. Not prevented, recovered
from:
- After a crash the pods page only the last audio owner (docs/paging.md).
  BlueZ reconnects the laptop itself after a link loss with timeout
  (`ReconnectAttempts` in `main.conf`).
- Android broadcasts no disconnect reason (the HCI reason only reaches a
  system API), and Linux' 0x15 gets no BlueZ retry, so both redial after a
  drop unless the buds were stowed (see the ownership model). A manual
  disconnect while worn redials too.

## Device state notes

- The primary bud holds the link: putting it in the case drops it (HCI
  reason 0x15, remote power off) and the pods come back ~6 s later with the
  other bud as primary; the new session's settings half (name, noise mode,
  EQ) can then take ~20 s. So `Engine::connected` carries over what's only
  shown (`DeviceState::carried_over`: name, noise mode, battery as
  last-known, firmware, the cycle) for the same pods at most `CARRY_OVER`
  (60 s) apart. With a bud out the pods fall back to Transparency by
  themselves and return to the chosen mode once both are in, so the
  carried mode follows the ears by that rule (the modes last reported with
  both in and with one) until the pods report it. Routing state starts
  empty, and the startup check counts only live battery readings
  (`Battery::is_live`).
- Ear detection is reported by role (primary/secondary). The physical
  primary comes from `PrimaryBud` (0x08, `[bud] 00 [peer] [active]`), sent
  in every session's startup burst and right at a primary switch; ear
  detection roles swap with it, and the battery list is ordered primary
  first.
- A `Disconnected` battery component keeps its last level, shown as `(60%)`;
  the case reports `Disconnected` (level 0 or 255) over AACP whenever no bud
  is inside it.
- The BLE advertisement is used to notice the pods nearby (Linux
  `presence.rs`, Android `bluetooth.rs`) and, on Linux, for the case's
  level (`ble::case_level`); not for sides or ears (`PrimaryBud` covers
  them; the per-side in-ear bits are in LibrePods' `BLEManager` if ever
  needed).

## Transport notes

- Android exposes L2CAP as an `InputStream`: reads can split or merge frames,
  hence `Decoder`. Variable-length frames complete only when the next frame's
  magic arrives.
- Linux uses `SOCK_SEQPACKET`; a read may still contain several frames.
- Both platforms advertise Apple's Device ID (vendor `0x004C`); the AirPods
  only allow multipoint handoff to Apple hosts. Linux sets it in BlueZ's
  config; Android needs the LSPosed hook.

## Firmware

`docs/firmware.md` is what the pods' firmware says, by build, image and
offset (fetched and unpacked by `tools/firmware/`, never committed). Add to
it whatever new RE shows, with how each landmark was found, and update the
README's credits when work from elsewhere is used.

## Conventions

- Keep comments short; put rationale in commit messages.
- The pods are the store: nothing they hold (firmware, the cycle, modes)
  is persisted on a host; what they don't report is shown only once set
  or told, in memory. Persisted are only the host's own (settings, local
  MAC) and the keys presence needs before any session.
- Prefer enums over flags; make invalid states unrepresentable where it's cheap.
- Prefer iterator/combinator style over mutable accumulation.
- Test enum variants with `matches!`/`match`/`if let`, not `==`. Derive
  `PartialEq` only for real comparisons (MACs, the diffed notification and
  D-Bus status); for test asserts use `#[cfg_attr(test, derive(PartialEq, Eq))]`.
- Java is limited to the Android entry points that must be Java subclasses
  (`PersistentService`, `LauncherActivity`, `StartReceiver`, and inside the
  service the broadcast receiver and the `AudioPlaybackCallback`, both
  abstract classes JNI can't create); they forward to Rust and hold no
  logic. Don't add Java; use JNI wrappers (and tricks like
  PendingIntent callbacks) instead.
- JNI wrappers under `crates/android-app/src/jni/` mirror Java packages and are
  generic over the reference type (`R: Reference + AsRef<JObject>`). Java's
  int constants become `#[repr(i32)]` enums (one value, passed with
  `.into()` from `IntoPrimitive`: `ScanMode`,
  `Visibility`; `TryFromPrimitive` when they come from Java, like `Usage`)
  or `bitflags` types (combinable: `PendingIntent` `Flags`, `CallbackType`),
  with only the values used. The
  module is private, so unused wrappers show up as dead-code warnings:
  remove them (git has them if needed again).

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo clippy -p android-app -p android-hook --target aarch64-linux-android
```
