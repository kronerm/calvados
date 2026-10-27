# AGENTS.md

Notes for developers and coding agents. End-user docs live in `README.md`;
the reasoning behind individual changes lives in commit messages.

## Crates

| Crate | Role | Boundary |
|-------|------|----------|
| `protocol` | AACP decoding, framing, commands, state, settings, the engine | Pure Rust, sans-IO, fully unit-tested. No platform code. |
| `linux` (binary `calvados`) | BlueZ daemon (`bluer`, tokio, D-Bus via `zbus`) | Transport + effects only; all decisions come from `Engine`. |
| `android-app` (`libcalvados.so`) | Android foreground service (cdylib) + thin Java shell | Transport + effects + notification; decisions from `Engine`. |
| `android-hook` (`libcalvados_hook.so`) | LSPosed native hook in the system Bluetooth process | `#![cfg(target_os = "android")]`; empty on the host. |
| `android-xposed` | Host build tool packaging the hook as an LSPosed APK | Shells out to SDK tools. |

## Building

The root is a virtual manifest over host and Android crates: always name
the crate with `-p`. Host is the default target.

```sh
cargo build -p linux --release              # the daemon (or `makepkg -si`)
cargo xposed                                # -> target/release/calvados-xposed.apk
cargo app-build                             # = cargo apk2 build -p android-app
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
`PATH` for the xposed APK. Any JDK works: `.cargo/config.toml` sets
`JDK_JAVAC_OPTIONS=--release 17` for cargo-apk2's `javac` (the xposed build
passes the flag itself), since `d8` rejects class files from newer JDKs
(Java 27: "Unsupported class file major version 71").

Release signing: cargo-apk2 takes the keystore from the environment; for
the committed one:

```sh
export CARGO_APK_RELEASE_KEYSTORE=crates/android-app/release.keystore
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=asdflmao
```

A new keystore needs the same password for store and key (cargo-apk2 passes
one to `apksigner`): `keytool -genkeypair -v -storetype PKCS12 -keystore …
-alias … -keyalg RSA -keysize 4096 -validity 10000 -storepass … -dname
"CN=Calvados"`. A new key changes the app's signature: uninstall the old
build first.

## Protocol crate layout

- `aacp/` — one module per opcode: `OPCODE` constant + nom parser. `aacp::Packet`
  is the single decoded-packet enum; unknown or undecoded payloads become
  `Packet::Opaque`. Byte enums derive `num_enum::TryFromPrimitive`.
- `protocol.rs` — framing (`Decoder` for byte streams, `parse_datagram` for
  message-preserving reads; both share `step()`), and command builders.
- `state.rs` — `DeviceState`, display-only state accumulated from packets.
- `engine.rs` — `Engine`: every decision, returning `Effect`s. Inputs: the
  session's lifecycle (`connected(pods, now)`, `disconnected(now)`),
  packets, local media/calls starting, nearby advertisements (`nearby`, with
  the stored keys), settings and noise-mode requests, and `tick(now)` at
  `next_wake()` for its timers (startup check, stream check, redials). The
  time comes in as `Instant`s, so the platforms keep no timers of their own
  but the notification throttle and attach rate limit.
- `settings.rs` — `Setting`/`Settings`: AutoSwitch and AutoConnect, with
  the storage key both platforms use; stored by the platforms, obeyed by
  the engine.
- `display.rs` — what both platforms show, so they read the same: `Buds`
  (per component: level, charging, last known, place, primary), the
  ownership label, and the noise modes' names (D-Bus `NoiseMode`, the Android actions).
- `ble.rs` — the proximity pairing advertisement: IRK resolution, the lid
  and in-case bits (the lid bit holds only from a bud in the case), and the case's level from its encrypted last 16 bytes
  (AES-128 decrypted with the pods' encryption key, as sent; byte 3, the
  top bit charging, LibrePods). `Engine::nearby` writes that level into the
  state for our pods, also during a session; the status stays the pods'
  (last known while no bud is inside, when the advertisement knows better).
- `firmware.rs` — the running version (`Version::of_running`: the first
  field of 0x1D's `71.2256000021000000.6663` is major then a one-digit
  minor, so 7.1; `display::firmware` shows it, or the raw string), and
  Apple's catalog: `catalog_url`, `catalog_models` (a product's models,
  served first: AirPods Pro 2 USB-C asks A3048, the case A2968, then
  A3047, which gets a 403; an unknown model only itself) and, behind the
  `catalog` feature (the `plist` crate; only Linux enables it),
  `parse_catalog` (the newest of `Assets`: `FirmwareVersionMajor`/`Minor`/
  `Release`, `Build`). The running string carries no release number (9A348
  is `90.…`), so `Release::is_newer_than` compares major.minor only.
  `DeviceState` keeps the model and firmware from 0x1D, carried over a
  reconnect like the name.
- `connection.rs` — L2CAP PSM and service UUID.

### How sure we are

- *Known*: from LibrePods and confirmed by our captures or by commands
  working (battery, ear detection, control commands, smart routing, device
  information, audio source, connected devices).
- *Observed*: our own, the structure holding across captures (framing
  lengths, `DeviceConnection` roles against known MACs, `PrimaryBud` at
  switches, the `0x2B` layout).
- *Inferred*: a meaning guessed from patterns; doc comments say
  "Inferred:" (history kinds and clock, `PrimaryBud::active`, everything
  about the opaque packets below beyond their bytes).

### Opaque packets (decoded no further)

From captures; meanings beyond the structure are guesses.

- `0x02`, after Request Notifications: a count (`1d` = 29), then that many
  `(id, value)` entries, mostly one byte each (`0a 01`, `50 02`), but ids
  `03 04 06 07 30` carry a 4-byte LE value. Length not encoded, so the frame
  ends at the next magic. One capture only.
- `0x17`: `[00] [kind] [2] [len 2 LE]` + body. Kind `04`: the pods' firmware
  endpoints, `AP AOP RTP BTM DSP1 DSP2`, each with `00 80 00 00`. Kind `00`:
  protobuf (`08` sequence number, `10` 1/3, `2a` nested message) wrapping an
  Apple binary-serialized dictionary (OSSerializeBinary, signature `d3 00 00
  00`): `VendorID` = `0x05ac`, `AccessoryService`, `MaxReportSize`, `devmot`…
  Sent unrequested: kind `04` also right after connect, kind `00` in the
  burst after a primary switch.
- `0x4E`: 8 zero bytes, once per session in the startup burst.
- `0x52`: `03 00 02 01 [00|01]`. `00` right before a bud leaving an ear
  (three times) and right after this host started playing; `01` alone,
  with nothing else around it.
- Case charging sounds are a *command* with a non-AACP header,
  `12 3A 00 01 00 08 [00 on / 01 off]` (LibrePods), not a packet.

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
- `service.rs` — the event loop: one channel of `Event`s (intents, link
  events, stop) driving the `Engine`; effects out.
- `link.rs` — the AACP L2CAP socket (connect / send / read thread).
- `bluetooth.rs` — paired AirPods, local MAC, A2DP routing, and the
  presence scan: a `BluetoothLeScanner` scan (Apple manufacturer data `07`
  with the lid open, masked: pods charging in the closed case advertise
  too; low power, controller-filtered, `CALLBACK_TYPE_FIRST_MATCH`) whose
  results start the service as `NEARBY` intents through a `PendingIntent`
  (no Java; the system keeps scanning without the app). Not across
  Bluetooth turning off, though: the scan is gone after, so the service
  starts it again on `BluetoothAdapter.ACTION_STATE_CHANGED` to `STATE_ON`
  (the receiver's filter in `PersistentService.java` lists that action).
  Without it, after a toggle on Oct 18 the phone never noticed the pods
  again: the laptop connected them first, they didn't page the phone, and
  it took a manual connect. While the pods'
  ACL is down, the engine connects the pods an advertiser resolves to with
  their stored IRK (preferences `identity_key:<MAC>`, `encryption_key:<MAC>`), as on Linux;
  `Effect::ConnectPods` is an A2DP connect. The scan permission is
  `neverForLocation`. Android builds first-match results from the
  controller's tracking event (vendor 0xFF, subevent 0x56) without the
  advertisement's data, though the event carries it, so the phone gets no
  case level from it. A second, batched scan of every match from a bud in
  the case (Oct 8-18) got every worn AirPods nearby regardless of its
  in-case mask, waking the phone every ~5.5 s nearly all day; it's gone
  (a `PendingIntent` scan outlives the app, so it needed stopping by its
  request code once).
- `intents.rs` — action names, intents to the service, parsing incoming ones.
- `notification.rs` — channel, `startForeground`, and the notification: a
  `DecoratedCustomViewStyle` with two `RemoteViews` layouts from `res/`
  (collapsed: title, status and levels like the Linux bar; expanded: the
  Linux panel's batteries, noise tiles and settings rows), filled by view
  id. There's no generated `R` class, so ids come from
  `Resources.getIdentifier`. Tiles and rows fire service intents
  (`NOISE:<mode>`, `AUTO_SWITCH`, `AUTO_CONNECT`). Bold text is a `Spanned`
  from `Html.fromHtml`. RemoteViews allow only some views (no `Space`), and
  an id must be a valid Java name (no `case`).
- `res/` (crate root) — the layouts and the icons: Tabler SVGs converted to
  VectorDrawables. Every color (text included) is one of five
  `notification_*` colors: the system's dynamic palette (`system_neutral1_*`,
  `system_accent1_*`…, hence Android 12 as the minimum), each with a
  `-night` variant. Theme attributes resolved against the app's light
  theme, unreadable in a dark shade. `ic_stat_airpods` is
  the status bar icon.
- `media.rs` — pause/resume as media button presses.
- `jni/` — typed wrappers mirroring Java packages, plus `with_env`.

## Linux crate layout (`crates/linux/src`)

- `main.rs` — setup, the shared `Daemon` context, and one loop owning the
  `Engine`: attaching (kept across iterations, so no L2CAP connect is cut),
  the session's datagrams, the engine's `next_wake`, MPRIS, and `Input`s
  from D-Bus and presence. No shared mutable state besides the published
  `Status`.
- `bluez.rs` — Apple DID record, finding the pods, L2CAP, A2DP.
- `dbus.rs` — the `dev.kroner.Calvados1` object and its `Status`, published
  through a `watch` channel; a task emits `PropertiesChanged` per changed
  property. Writes go to the loop as `Input`s; a setting's setter waits
  until the status shows it, so zbus' own changed signal is current.
- `mpris.rs` — pause/resume of MPRIS players on one ordered worker task.
- `presence.rs` — reports Apple advertisers to the loop (`Input::Nearby`;
  `Engine::nearby` decides), since the pods page only the host that last
  had the audio. BlueZ's advertisement monitor
  (`Experimental = true` in `main.conf`; without it one info line, no
  monitor) reports Apple proximity pairing advertisements (manufacturer data
  `4c 00 07`, filtered by the controller); one whose rotating address
  resolves with a stored IRK (`protocol::ble::resolves`) is a pair of ours,
  connected unless a session is up or they're put away
  (`ble::Proximity`: lid closed and the advertising bud in the case; byte 8
  bit 3 and byte 5 bit 6, from LibrePods), once per 3 s (both buds
  advertise). The monitor reports a device once, so each one's
  `ManufacturerData` is followed (property changes) until BlueZ forgets it:
  the lid opening changes it. Keys: every session requests both (`0x30`,
  mask `0x05`: IRK `0x01`, encryption key `0x04`; reply `0x31`, an
  `Effect::LearnedKey` each), stored as `$XDG_STATE_HOME/calvados/<address>.irk`
  and `.enc`, one pair each. A fresh link
  starts with the profile `off`, so connecting never takes the audio.
  The monitor starts after the D-Bus name is claimed (single instance).
- `firmware.rs` — the catalog check:
  `https://mesu.apple.com/assets/com_apple_MobileAsset_UARP_<MODEL>/com_apple_MobileAsset_UARP_<MODEL>.xml`
  for the pods' model (0x1D), trying `catalog_models` in order until one
  answers; once a day per product, in memory. reqwest (rustls, ring) in a
  spawned task; the result returns to the loop as `Input::Catalog`, which
  keeps it (a failed check keeps the last answer; logged at debug/info).
  On D-Bus: `Firmware` (`7.1`), `LatestFirmware` (`9.0.348 (9A348)`, empty
  until checked) and `FirmwareUpdate`. Android doesn't check (no INTERNET
  permission): it shows the running version only.
- `cli.rs` — `calvados watch` / `calvados set …`: a zbus proxy client of
  the daemon's own D-Bus object (no daemon logic), for shells without
  D-Bus. `watch` prints JSON lines on `PropertiesChanged` and on the bus
  name's owner changes: the D-Bus properties, as data (the plugin does its
  own labels).
- `dist/noctalia/` — Noctalia (v5, Luau) plugin, installed to
  `/usr/share/calvados/noctalia`: a `[[service]]` holds one `runStream` of
  `calvados watch` and republishes it via `noctalia.state`; the bar widget
  and the attached panel only watch that, and act through
  `noctalia.runAsync({"calvados", "set", …})`. Noctalia plugins have no
  D-Bus API. Noctalia scans only configured sources and
  `~/.local/share/noctalia/plugins` (no system dir; `plugin_manager.cpp`),
  so users symlink the installed copy there (README). Scripts hot-reload; a panel's
  `width`/`height` only apply when Noctalia rebuilds its panels, on a
  `[plugins]` config change (disable + enable the plugin) or a restart, not
  on `config-reload`. `noctalia plugins lint crates/linux/dist/noctalia` checks it,
  but not `plugin_api` against the installed Noctalia: 5.0.1 supports 3–30
  (its log names the range when it refuses a plugin), while the published API
  definitions run ahead (container tooltips are API 32). The plugin needs 24
  (`runAsync` with an argv table).
- `state.rs` — the state directory and the two remembered settings
  (`protocol::settings`; files `auto_switch`/`auto_connect`, `on`/`off`,
  default on), the same two as the Android notification's Switch and
  Connect buttons (preferences under the same keys).
- `hci.rs` — the pods' traffic from the kernel's HCI monitor channel (raw
  `AF_BLUETOOTH` socket, `HCI_CHANNEL_MONITOR`, as `btmon`; needs
  `CAP_NET_RAW`, else one info line and nothing), decoded on its own thread
  into `hci:` debug lines: links (paging, up, down with reason and how long
  the pods had been silent, which dates a crash: a timeout follows only the
  supervision timeout's silence), role, sniff,
  AVDTP signaling, AVRCP passthroughs, A2DP media starting to flow,
  delivery (packets sent against the controller's completed-packet credits:
  a gap of 0.5 s or more with packets outstanding is logged when it ends,
  what's still undelivered at a link's end, and flushes), and every AACP
  frame both ways, in full. The delivery lines are to see the pods going out of
  reach: on Oct 26 the laptop heard nothing for 2 min while the link held,
  the pods kept it as the source and wouldn't start the phone's stream. L2CAP channels are tracked from signaling; the pods'
  link is the one our AACP channel opens on, so channels opened before the
  daemon started (A2DP's) stay unknown until reopened.
- `pipewire.rs` — the pods' card profile, set natively (`pipewire` crate,
  on its own thread and main loop): `off` as soon as BlueZ reports them
  connected (no AACP needed) and on `Effect::ReleaseAudio`; `a2dp-sink` on
  `Effect::RouteAudio`. With `off` the laptop keeps the link but can't pull
  the audio from the phone. Profile params of `bluez_card.*` devices are
  subscribed (which reports the active profile but not the list), and the
  profile list (with each profile's availability) is enumerated whenever
  the card's params change; for 10 s after a request every change is
  corrected (the card appears after BlueZ connects, then WirePlumber
  restores a remembered profile), and after that the requested profile is
  selected again whenever it becomes available again. A stream start the
  pods never answered (a laptop take 2 s after a phone take, 20:09:36) got
  aborted by BlueZ, `a2dp-sink` became unavailable and the card fell to
  `off`; the pods offered A2DP again 45 s later, but nothing restored it. `off` is set with `save`, `a2dp-sink` without, so WirePlumber
  remembers `off` and later connects start that way.
  On `Effect::RestartStream` (see the stream check below) the
  `bluez_output.*` node gets a `Suspend` command and, streams being linked,
  starts again at once: a start sent while the pods were still switching
  was lost (sink `RUNNING`, BlueZ transport `idle`, players blocked). Sent
  right at the take, it rebooted the pods once.
- `PKGBUILD` (repo root) — Arch package built from the checkout (binary +
  the unit, `ExecStart` rewritten to `/usr/bin`); `!lto`, since makepkg's
  LTO turns libspa's C shim into bitcode rust-lld can't link. The install
  script sets `cap_net_raw+ep` on the binary (user units can't grant
  capabilities; the unit must not set `NoNewPrivileges=`).
- `dist/calvados.service` — systemd user unit. The daemon never powers the
  adapter (it pages the pods only for `AutoConnect`, see the ownership
  model), waits for BlueZ's `Connected` property signal
  (no polling; re-finds paired AirPods each cycle), and refuses
  to start twice (D-Bus name requested with `DoNotQueue`). The Apple DID comes
  from `DeviceID` in `/etc/bluetooth/main.conf` (required, from boot); the
  daemon only warns when it's missing.

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
- the firmware line ("Firmware 7.1", `format.firmware` and
  `notification.rs` `Content::firmware`; " · 9.0 available" on Linux only);
- the battery glyph thresholds 88/63/38/13 (`format.batteryGlyph`,
  `notification.rs` `battery_glyph`);
- the icons: Noctalia names Tabler glyphs, Android has them converted in
  `res/drawable` (SVG paths, stroke 2, round caps and joins).

## Ownership model

- One long-lived AACP connection per platform; it is only opened over an
  ACL link the OS already has. The pods are paged (an OS-level connect,
  never a bare AACP socket) only with `AutoConnect` on: when their BLE
  advertisement shows up (presence), and after a session drops unless both
  buds were put away (`DeviceState::is_stowed`: in the case, or gone
  without a live battery reading), 3 times 3 s apart (`Engine::disconnected`
  and `tick`). "Not in an ear" was too strict: a bud just taken out hasn't
  reported its ear yet when the primary goes into the case. The pods page back only the host that last had the audio; BlueZ
  retries only after a timeout (0x08), not after a primary handover (0x15),
  and the BLE monitor reports an advertiser only when first found, so a
  drop while they advertise needs this.
- Taking audio: only automatically, through media or a call starting here
  (auto-switch, below; there's no manual take), or when this host is the
  only one connected to the pods (`DeviceState::hosts`) and a bud is out of
  the case (`DeviceState::is_stowed`; pods connected while charging took the
  audio from other headphones): checked at connect, when a bud comes out, ~0.2 s after the AACP
  handshake (`ConnectedDevices` list), and when another host leaves. A
  session joining pods that already have hosts starts with
  `DeviceConnection` (0x0C; role 1 = host, 2 = bud) for every connected
  device, *before* the list, so hosts stay unknown until the list arrives;
  on a fresh connect the list can come first. On a fresh connect while
  another host is connected, the rest of the startup burst (our own
  `DeviceConnection`, the `0x02` reply, `DeviceInformation`, the settings,
  EQ) is held back: 24 s in one capture, until just before that host
  disconnected. Mid-session, a leaving host comes as `DeviceConnection`
  (`connected = false`), a joining host as a new list (no
  `DeviceConnection`); both confirmed by connecting and disconnecting the
  laptop by hand. A list also comes in the burst after a primary switch
  (`PrimaryBud`, `DeviceConnection` for the buds and this host,
  `AudioSource`, `ConnectedDevices`), and once right after a host left. The auto-take sends
  nothing to the firmware (no peer to hand off from); it sets `Owning` and
  emits `RouteAudio`. There is no media-triggered auto-take.
- `Ownership`: `Idle` → `Claiming(Handoff::{Pending, Sent})` on take → `Owning(_)` when the
  firmware confirms (`OwnsConnection = 1`). Media or a call starting again while the
  handoff is unconfirmed sends it again: once the pods didn't answer at all
  (Oct 9 18:22:52) though the other host got it and paused, and ignoring
  later plays left the laptop silent until that host's next yield. `AudioSource` packets also move the
  state: our MAC streaming → `Owning`; another device streaming while
  `Owning` → `Idle` plus `ReleaseAudio` (pause), sending nothing. The pods
  switch to any host that starts streaming A2DP, even while the other plays
  (no takeover frames needed), and that source change precedes the revoke
  (`OwnsConnection = 0`) by ~30 ms.
- What the firmware does with these (docs/ownership.md): `OwnsConnection`
  goes to an arbiter that keeps one record per host; the last host to write
  1 wins at once (it outranks a streaming host), writing 0 hands it to the
  other, and the pods send `OwnsConnection` only when the owner changes
  (1 to the new, 0 to the old). Nothing there touches A2DP. So after
  letting go without a write (another host streamed), the pods still count
  us as owner and a later claim gets no answer: the engine remembers what
  they last said (`PodsOwner`) and routes at once then, likely the Oct 9
  silence. The smart-routing frames (0x10) are relayed verbatim as 0x11,
  never parsed. `AudioSource` (0x0E) is the host whose audio changed last
  plus one activity mask over all hosts (`Activity`: 0x01 call, 0x02 media,
  other bits unknown), so one host stopping can report nothing playing while
  the other streams.
- A session can get only the settings half of the startup burst (the 0x02
  reply, `DeviceInformation`, settings, EQ) and not the state half (battery,
  ear detection, `PrimaryBud`, `DeviceConnection`s, `ConnectedDevices`):
  seen with two AACP hosts, on fresh connects, and right after the pods power
  on. Only the handshake goes out at connect; the engine sends the rest of
  the setup (feature flags `0x4D`, notifications, EQ) on its ack: sent
  together, a late ack (~0.6 s, fresh connects) lost them, and without the
  feature flags the pods reported Adaptive as Off (`0D 01`) to that host
  while the other got `0D 04`. `Engine::check_state_burst`, due 2 s after
  the handshake and again every 2 s while it asks for something, re-sends
  the handshake when no ack came (a session opened as the pods finish booting can stay silent)
  and after 3 tries asks for a new AACP connection (`Effect::Reconnect`);
  acked but without battery, it re-requests notifications, 3 times at most,
  then also reconnects if nothing at all came after the ack (`Startup::Acked`
  vs `Answering`): once the pods sent nothing more for 53 minutes, not even
  a noise mode's echo (Oct 18 13:36:54), and a new connection worked at once.
  Without that half the session still works: battery, ears and lists come
  with later changes; auto-take waits for a known `AudioSource`. The history names the
  other hosts regardless, so peers come from it too.
- Relayed routing frames (0x11) carry their length like 0x10 and complete
  at once; as unknown-length frames they waited for the next frame's magic
  on Android (11.5 s in one capture).
- Automatic switching (`Engine::local_audio`): media starting on this host
  while `Idle` takes the audio, like Apple devices: not while the other host
  is on a call (`AudioSource` kind `Call`), nor while no `AudioSource` is
  known, nor while both buds report being in the case
  (`DeviceState::in_case`; calls too). The platforms report what plays
  (`Engine::local_audio`: silent, media, a call over media) and the engine
  finds the starts. A bud going into an ear while something plays here and
  we don't hold the audio takes it too (`Engine::ear_take`): a call wins
  over media, and with the same on both hosts the one the pods count as
  owner (`PodsOwner`) takes, so only one does. Both buds going into the case while
  holding the audio releases it (Linux: profile `off`, so the laptop's
  speakers play). Android still routes to the pods by itself, and the pods
  follow any host that streams. A call starting takes regardless of what plays elsewhere. Linux: MPRIS
  `PlaybackStatus` → `Playing` (D-Bus signals). Android: the
  `AudioPlaybackCallback` (`USAGE_MEDIA` starts media, a
  `USAGE_VOICE_COMMUNICATION` playback is a VoIP call) and the phone-state
  broadcast (`OFFHOOK`: answered or placed; ringing doesn't take); rising
  edges only (`service.rs` `LocalAudio`). Hosts
  arbitrate, not the pods: a host's `media_information` is only relayed to the
  peer (`PeerMedia`, logged as "peer reports its playback"). A peer's takeover
  arrives as three relayed frames, `PeerMedia`, `ShowNearbyUi`,
  `YieldRequest`; only the last (and `OwnsConnection = 0`) releases.
- Ownership is asserted to the firmware (`set_ownership(true)`) only together
  with the targeted handoff sequence to a known peer, never as a bare claim.
- Yield requests and `OwnsConnection = 0` release once (`set_ownership(false)`
  + `Effect::ReleaseAudio`); repeats send nothing. A yield request always
  emits `ReleaseAudio` (pause), even when not holding the audio: a host
  playing without a take (app just restarted) otherwise plays on, its OS
  restarts the suspended stream ~4 s later, and the pods switch back to it
  (10:47:58; the laptop's take 1.7 s after that rebooted the pods). A declined handoff also emits
  `ReleaseAudio`. Both platforms then pause, like iOS/macOS: Linux turns the
  PipeWire profile off and pauses its MPRIS players (otherwise playback
  falls back to the speakers); Android sends a pause key, as the pods do too.
- Android after a take: the pods remotely suspended our A2DP stream when
  another host took over, and `btif_av` won't restart a remotely suspended
  stream; only rebuilding the audio output clears it (a seek, or Android
  after ~10 s of a stalled output). `RouteAudio` therefore toggles
  `AudioManager.setParameters("A2dpSuspended=true/false")`, 200 ms apart,
  right away: the output is torn down (to no device, not the speaker) and
  rebuilt; the source switches right after (~0.5 s after the take).
  Every take after another host had the audio stalls, so Android ignores
  the stream check (waiting first cost ~1 s). Verified on a Pixel 9
  (Android 16); it's HAL-dependent.
- A paused Android player leaves its A2DP stream started; the pods
  suspend it only when they switch, and they don't switch within ~5 s of
  switching to this host (the Linux stall: a laptop take right after a
  phone take). Toggling `A2dpSuspended` on release doesn't help: the
  `false` restarts the stream right away (the output stays open while
  paused), the pods refuse it (`START_EVT status=1`), and the HAL blocks
  later toggles for ~5 s, stalling the next phone take.
- The take toggle doesn't run during a call (phone or VoIP): Android keeps
  A2DP suspended itself while SCO is up, and our `false` would undo that.
- Stream check (Linux): 1 s after a confirmed take `Engine::check_stream`
  (through `tick`) emits `RestartStream` unless the pods name us
  the media source by then, and again every second, 10 times at most. The
  pods ignore a new source for some seconds after switching to one: with
  the phone started 0.5 s before the take, restarts 1.5 and 3.5 s after
  that switch were ignored, one at 7.5 s made the pods suspend the phone
  (`initiator=false` in its log) and switch. Healthy
  Linux takes stream within ~0.7 s and are left alone: restarting on every
  take cut the running stream.
- The engine touches media playback only through the iOS-like ear rule: a
  bud leaving an ear while this host is the media source (`AudioSource` =
  our MAC, `Media`) emits `Effect::Pause`; both back in emits
  `Effect::Resume` if we paused; both out forgets the pause. The pause lives
  in `Ownership::Owning(Playback::EarPaused)`, so leaving `Owning` (release,
  another device streaming) forgets it too. Calls are never paused. Android
  sends media keys (`media.rs`), Linux pauses/resumes MPRIS players
  (`mpris.rs`).
- The local MAC is needed for the peer handoff (`engine::LocalMac`):
  - *inferred* at connect, before the first device list: from the history's
    newest event when it's a host connecting at most 2 s ago (a fresh
    connect: our link came right before AACP; the list precedes any
    `DeviceConnection` then), or from the session's first `DeviceConnection`
    (host, connected), which is the new connection itself when joining pods
    already connected (every capture). Dropped if the device list doesn't
    contain it; used, not persisted;
  - *known* when saved (learned earlier; apps can't read the adapter's
    address, `LOCAL_MAC_ADDRESS` being privileged-only; Linux has it from
    BlueZ) or once
    it's the only host connected (`DeviceState::hosts` has one entry: a
    one-entry list, or the other hosts having disconnected); then persisted
    (`Effect::LearnedLocalMac`).
  Without it, Take only routes audio (no claim that could never complete).

## Debugging a reboot

What there is to read after one, and for how long:
- Linux: the daemon's journal (weeks): decisions, packets, `hci:` lines.
- Android: the app's daily logs (14 days), every AACP frame both ways
  (`adb shell "su -c 'cat /data/data/dev.kroner.calvados/files/logs/…'"`).
- Android: the HCI snoop log (Developer options, "Enabled", i.e. full), the
  most complete source, `/data/misc/bluetooth/logs/btsnoop_hci.log` and
  `.last`, read with `btmon -r`. It rotates every `persist.bluetooth.btsnoopsize`
  packets (default 65 536, ~1-2 days; set to 1 048 576 on the test phone,
  ~2-3 weeks) and at a Bluetooth restart.
- The pods keep nothing readable: only what happened around a crash shows.
  Their last packet to a host, plus that host's supervision timeout (5 s on
  the phone), dates it: Oct 18 14:57:27.6, 0.4 s after confirming the
  laptop's take and right after suspending the phone, before any restart
  by the stream check.

## Pod reboots

Rapid takeovers (a laptop take within ~6 s of a phone take) occasionally
reboot the pods mid-switch: after they pause the old host, before they
suspend its stream; nothing in AACP warns first (HCI traces of 13:20:40
and 13:34:01). Not prevented, recovered from:
- After a reboot the pods page only the host that last had the audio.
  BlueZ reconnects the laptop itself after a link loss with timeout
  (`ReconnectAttempts` in `main.conf`).
- Android broadcasts no disconnect reason (the HCI reason only reaches a
  system API), and Linux' 0x15 gets no BlueZ retry, so both redial after a
  drop unless the buds were stowed (see the ownership model). A manual
  disconnect while worn redials too.

## Device state notes

- The primary bud holds the link: putting it in the case drops it (HCI
  reason 0x15, remote power off) and the pods come back ~6 s later with the
  other bud as primary. A reconnected session's settings half (name, noise
  mode, EQ) can then take ~20 s. `Engine::connected` carries over what's
  only shown (`DeviceState::carried_over`: name, noise mode, battery as
  last-known). With a bud out the pods fall back to Transparency by
  themselves and return to the chosen mode once both are in, so the carried
  mode follows the ears by that rule (the modes last reported with both in
  and with one) until the pods report it so it doesn't blank meanwhile, for the same pods after at
  most `CARRY_OVER` (60 s) apart; routing state starts empty, and
  the startup check counts only live battery readings (`Battery::is_live`).

- Ear detection is reported by role (primary/secondary). The physical primary
  comes from the `PrimaryBud` (0x08) packet, sent in every session's startup
  burst and right at a primary switch (e.g. the primary going into the case);
  ear detection roles swap with it, and the battery list is ordered primary
  first.
- A `Disconnected` battery component keeps its last level, shown as `(60%)`;
  the case reports `Disconnected` (level 0 or 255) over AACP whenever no bud
  is inside it.
- The BLE advertisement is used only to notice the pods nearby (Linux
  `presence.rs`, Android `bluetooth.rs`), not for state: `PrimaryBud` covers the sides, and the case
  level barely changes while it's closed. (Its per-side in-ear bits and,
  decrypted with the encryption key, a live case level are in LibrePods'
  `BLEManager` if ever needed.)

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
- Commit messages from agents are prefixed with the model name.

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo clippy -p android-app -p android-hook --target aarch64-linux-android
```
