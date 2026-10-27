# Calvados

AirPods Pro 2 on Android and Linux the way they work across Apple devices:
the audio follows whichever device starts playing, the AirPods connect when
they come out of the case, and their battery, noise control and settings
are a tap away.

- **Automatic switching:** starting media (or a call) on the phone or the
  laptop takes the AirPods from the other; the one losing them pauses.
- **Automatic connection:** the AirPods connect when they come out of the
  case nearby, to the phone and the laptop alike, and again if they drop
  while you wear them.
- **Ear detection:** taking a bud out pauses, putting it back resumes.
- **Putting them on:** a bud going into an ear while something plays takes
  the AirPods (a call over the other device's media; with the same on
  both, the device that had them last).
- **In the case:** with both buds in the case, playing doesn't take the
  AirPods, and putting both in hands the audio back (on Linux to the
  laptop's speakers; Android keeps them as its audio device by itself).
- **Status and controls:** battery per bud and case, where each bud is,
  which bud is the primary (it holds the link), noise control (ANC,
  Transparency, Adaptive), both settings above, and the firmware version,
  in the phone's notification and in a Linux status bar panel.

## Requirements

| | Minimum | Tested with |
|---|---|---|
| AirPods | AirPods Pro 2 (other AACP AirPods may work) | AirPods Pro 2 |
| Android | 12, with [LSPosed] | 16 (Pixel 9) |
| BlueZ | 5.60 | 5.87 |
| PipeWire | 0.3 | 1.6.8 |
| Noctalia (optional) | plugin API 24 | 5.0.1 (plugin API 30) |

- **Android:** the AirPods keep a phone connected alongside another device
  only if it identifies as an Apple device; the LSPosed module makes it do
  so. Calvados itself needs no root (LSPosed usually does). The Android
  version that matters most is the Bluetooth stack the
  module hooks, tried only on Android 16.
- **Linux:** MPRIS players for pausing and automatic switching. BlueZ
  below 5.87 and PipeWire below 1.6 are untested.

[LSPosed]: https://github.com/LSPosed/LSPosed

## Android

1. Install `calvados.apk` and `calvados-xposed.apk` (see
   [`AGENTS.md`](AGENTS.md) for building them).
2. Enable the Calvados module in LSPosed (its scope, the Bluetooth app, is
   preset), then turn Bluetooth off and on again so the Bluetooth app
   restarts with it. The same after updating the module.
3. Open Calvados once and grant the permissions. It has no window: it runs
   as a persistent notification.

The notification shows the AirPods' name and whether the phone has the
audio (Active, Standby), the battery levels, the primary bud's in bold,
and in its header the firmware the AirPods run ("Firmware 7.1 (6663)": the
version and its build). Expanded, it shows each bud's battery and where it
is (a dot marks the primary bud), the noise control modes to tap, and two
settings, both remembered:

- **Automatic switching:** media starting on the phone takes the AirPods
  (unless the other device is on a call), and so does a call when answered.
- **Automatic connection:** connect the AirPods when they come out of the
  case nearby, even if another device had them last.

"No handoff: this phone's MAC is unknown" shows until the AirPods have told
the phone its own Bluetooth address, normally right at connect.

## Linux

**Required:** the AirPods keep the phone connected alongside the laptop
only if the laptop identifies as an Apple device. Add this to
`/etc/bluetooth/main.conf` under `[General]`, then
`sudo systemctl restart bluetooth`:

```ini
DeviceID = bluetooth:004C:0000:0000
```

**Optional, for automatic connection:** the AirPods page only the device
that last had the audio when they come out of the case. For the laptop to
spot them and connect itself, also set this under `[General]`:

```ini
Experimental = true
```

It recognises your AirPods once it has been connected to them.

### Install

On Arch Linux, from a checkout of this repository:

```sh
makepkg -si
systemctl --user enable --now calvados
```

Elsewhere (Rust needed):

```sh
cargo install --path crates/linux --root ~/.local     # -> ~/.local/bin/calvados
sudo setcap cap_net_raw+ep ~/.local/bin/calvados      # optional: traffic logging
mkdir -p ~/.config/systemd/user
cp crates/linux/dist/calvados.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now calvados
```

After an update, install again and `systemctl --user restart calvados`.

### Use

The daemon never turns Bluetooth on. While the phone has the audio, the
laptop stays connected but silent (its AirPods profile is off), so
connecting never steals the audio. Starting playback in any MPRIS player
takes it, unless the phone is on a call; losing it pauses, like a Mac. With
no other device connected, the laptop takes the audio by itself.

**Noctalia:** the package ships a plugin: a bar widget (the buds' levels,
left then right, the primary's in bold, then the case's; highlighted while
the laptop has the audio) and a panel with the same controls as the phone.
The panel also says when Apple has newer AirPods firmware ("Firmware
7.1.21 · 9.0.348 available"): the AirPods tell their exact version over
UARP, Apple's update protocol, and the daemon checks Apple's public
update catalog once a day while the AirPods are connected. It can also
update them (below); while it does, that line shows the progress. Both
platforms remember the last version seen, so it still shows when the
AirPods don't report it again.
Link it where Noctalia looks for plugins, enable it, then add the widget
`kroner/calvados:bar` to the bar (Settings → Bar):

```sh
mkdir -p ~/.local/share/noctalia/plugins
ln -s /usr/share/calvados/noctalia ~/.local/share/noctalia/plugins/calvados
noctalia msg plugins enable kroner/calvados
```

**Command line**, for scripts and other status bars:

```sh
calvados watch                                    # the state as JSON, a line per change
calvados set auto-switch on|off|toggle
calvados set auto-connect on|off|toggle
calvados set noise anc|transparency|adaptive|next
calvados firmware status|update|cancel            # see below
```

The same is on the session bus as `dev.kroner.Calvados`
(`busctl --user introspect dev.kroner.Calvados /dev/kroner/Calvados`);
its properties signal changes.

## Updating the AirPods' firmware (experimental)

AirPods update only from an Apple device, but the update protocol can be
spoken from Linux. This has worked once, with the research tools below:
AirPods Pro 2 (USB-C) from 7.1.21 to 9.0.348, both buds. The daemon does
the same, inside its connection to the AirPods; that route hasn't run a
whole update yet. The AirPods check Apple's signature and refuse older
firmware, but if something goes wrong, only Apple can fix it. The case's
own firmware isn't updated this way.

1. `calvados firmware status` shows what the AirPods run and Apple's
   latest release for them.
2. Put both buds in the case and plug the case in; keep the lid open and
   the case still. If an iPhone or Mac is around, turn its Bluetooth off:
   it could start an update of its own.
3. `calvados firmware update` downloads the release from Apple (cached in
   `~/.cache/calvados`), offers it and shows the progress: about 8
   minutes, until it says the firmware is staged. Stopping the command
   doesn't stop the update; `calvados firmware cancel` does.
4. Close the lid, keep the case plugged in and leave it for about 30
   minutes: the case copies the firmware to the other bud and installs it
   on both.
5. Open the lid; once the AirPods are connected, `calvados firmware
   status` shows the new version.

The daemon refuses unless both buds are in the charging case and the
release is newer. If the AirPods decline or never start, close and reopen
the case and try again. A transfer that stalls for a minute is
withdrawn; one cut by a dropped connection carries on where it was once
they're back. Another calvados host (the phone) shows the progress, and
won't start an update of its own meanwhile.

The research tools (curl, unzip, b3sum, Python with dbus-python and
PyGObject, calvados stopped, the AirPods' address from `bluetoothctl
devices`): `tools/firmware/fetch.sh A3048` downloads the release for a
model (the right bud's), `python3 tools/firmware/uarp_probe.py <address>`
shows the running and staged firmware, and `python3
tools/firmware/uarp_offer.py <address>
firmware/A3048/asset/AssetData/Firmware.acsw/firmware.uarp full 2700`
stages it (step 3 above; then steps 4 and 5).

## Logs

- **Android:** logcat, and daily files with every frame's raw bytes (the
  last 14 kept, UTC), in the app's private storage, so reading them needs
  root:
  `adb shell "su -c 'cat /data/data/dev.kroner.calvados/files/logs/calvados.$(date -u +%F).log'"`.
- **Linux:** `journalctl --user -u calvados -f`. With the `cap_net_raw`
  capability (the Arch package sets it), it also logs its Bluetooth
  traffic with the AirPods (`hci:` lines).

## Known issues

Seen with AirPods Pro 2 (USB-C) on firmware 7.1.21 (7B21); newer firmware
may behave differently.

- **Switching back and forth quickly** (a laptop take within a few seconds
  of a phone take) occasionally reboots the AirPods mid-switch: a moment
  with one bud in noise cancelling and the other in transparency. Both
  devices reconnect by themselves; a music app that resumes when a
  Bluetooth device connects may then take the audio back.
- **A take can stall** for a few seconds right after the other device
  started playing: the AirPods refuse to switch for a while. The laptop
  retries until they accept.
- **Putting the primary bud in the case** drops the link for a few seconds
  while the other bud takes over; both devices reconnect, and what's shown
  stays meanwhile.

## Credits

Calvados builds on others' work:

- [LibrePods]: most of what's known about the AirPods protocol (battery,
  ear detection, noise control, conversational awareness, smart routing
  and the handoff, device information, the proximity keys), the BLE
  advertisement (lid and in-case bits, the encrypted case level), the case
  charging sounds command, and the Android approach: the two hooks into the
  system Bluetooth stack (Apple's Device ID, the L2CAP mode check).
- [openulla]: the AirPods firmware's AAP framing and AACP command table
  ([`docs/firmware.md`](docs/firmware.md) continues from it).
- [uarp-dump], [ftab-dump] and [linden/uarp]: the firmware's container
  formats.
- [Tabler Icons] (MIT): the icons on Android and in the Noctalia plugin.

[LibrePods]: https://github.com/librepods-org/librepods
[openulla]: https://github.com/mstampfli/openulla
[uarp-dump]: https://github.com/19h/uarp-dump
[ftab-dump]: https://github.com/19h/ftab-dump
[linden/uarp]: https://github.com/linden/uarp
[Tabler Icons]: https://tabler.io/icons

Developer notes live in [`AGENTS.md`](AGENTS.md).
