# Calvados

AirPods Pro 2 on Android and Linux the way they work across Apple devices:
the audio follows whichever device starts playing, the AirPods connect when
they come out of the case, and their battery, noise control and settings
are a tap away.

- **Automatic switching:** starting media or a call on one device takes the
  AirPods from the other, which pauses.
- **Automatic connection:** the AirPods connect when they come out of the
  case nearby, and again if they drop while you wear them.
- **Ear detection:** taking a bud out pauses, putting it back resumes.
  Putting a bud on while something plays takes the AirPods; with both in
  the case, playing stays on the device's own speakers.
- **Status and controls:** battery, where each bud is, noise control and
  the firmware, in the phone's notification and a Linux status bar panel.
- **Firmware updates** (experimental), from either device.

## Requirements

| | Minimum | Tested with |
|---|---|---|
| AirPods | AirPods Pro 2 (other AACP AirPods may work) | AirPods Pro 2 (USB-C) |
| Android | 12, with [LSPosed] | 16 (Pixel 9) |
| BlueZ | 5.60 | 5.87 |
| PipeWire | 0.3 | 1.6.8 |
| Noctalia (optional) | plugin API 24 | 5.0.1 |

AirPods keep a second device connected only if it identifies as an Apple
device: on Android the LSPosed module does that, on Linux a line in BlueZ's
config (below). Calvados itself needs no root.

[LSPosed]: https://github.com/LSPosed/LSPosed

## Android

1. Install `calvados.apk` and `calvados-xposed.apk` (building them:
   [`AGENTS.md`](AGENTS.md)).
2. Enable the Calvados module in LSPosed, then turn Bluetooth off and on.
   Again after updating the module.
3. Open Calvados once and grant the permissions. It has no window, only a
   notification.

The notification shows the AirPods' status and battery; expanded, the
noise control and two settings:

- **Automatic switching:** media or an answered call on the phone takes the
  AirPods (unless the other device is on a call).
- **Automatic connection:** connect when the AirPods come out of the case
  nearby, even if another device had them last.

It checks Apple's catalog for new AirPods firmware once a day. On
LineageOS, allow the app network access (Settings → Apps → Calvados →
Mobile data & Wi-Fi).

## Linux

Add to `/etc/bluetooth/main.conf` under `[General]`, then
`sudo systemctl restart bluetooth`:

```ini
# Required: identify as an Apple device.
DeviceID = bluetooth:004C:0000:0000
# Optional: connect when the AirPods come out of the case.
Experimental = true
```

Install (Arch Linux, from a checkout):

```sh
makepkg -si
systemctl --user enable --now calvados
```

Elsewhere, with Rust:

```sh
cargo install --path crates/linux --root ~/.local
sudo setcap cap_net_raw+ep ~/.local/bin/calvados      # optional: traffic logging
mkdir -p ~/.config/systemd/user
cp crates/linux/dist/calvados.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now calvados
```

After an update, install again and `systemctl --user restart calvados`.

The laptop stays connected but silent while the phone has the audio, and
takes it when an MPRIS player starts playing.

**Noctalia:** a bar widget and a panel with the same controls as the
phone. Link the plugin, enable it, and add the widget
`kroner/calvados:bar` to the bar:

```sh
mkdir -p ~/.local/share/noctalia/plugins
ln -s /usr/share/calvados/noctalia ~/.local/share/noctalia/plugins/calvados
noctalia msg plugins enable kroner/calvados
```

**Command line** and D-Bus (`dev.kroner.Calvados`), for scripts and other
bars:

```sh
calvados watch                              # the state as JSON, a line per change
calvados set auto-switch|auto-connect on|off|toggle
calvados set noise anc|transparency|adaptive|next
calvados firmware status|update|cancel
```

## Updating the AirPods' firmware (experimental)

When Apple has newer firmware, the panel and the notification say so and
offer **Update**. It has worked once (7.1.21 to 9.0.348, both buds, with
the research tools in [`tools/firmware`](tools/firmware)); the case's own
firmware isn't updated. The AirPods only accept Apple-signed, newer
firmware, but if something goes wrong, only Apple can fix it.

1. Put both buds in the case, plug it in, keep the lid open. Turn off
   Bluetooth on any iPhone or Mac nearby.
2. Tap **Update** (or `calvados firmware update`). It downloads the
   firmware from Apple and sends it: about 8 minutes, until it says
   "Staged: close the lid".
3. Close the lid and leave the case plugged in for about 30 minutes: the
   case installs it on both buds.
4. Open the lid; the new version shows once they're connected.

If it fails, the line says why; close and reopen the case and try again.
Your other calvados device shows the progress meanwhile.

**The proven way, without calvados** (the daemon stopped; needs curl,
unzip, b3sum and Python with dbus-python and PyGObject):

```sh
tools/firmware/fetch.sh A3048                       # the right bud's model
python3 tools/firmware/uarp_probe.py <address>      # what the AirPods run
python3 tools/firmware/uarp_offer.py <address> \
    firmware/A3048/asset/AssetData/Firmware.acsw/firmware.uarp full 2700
```

The last command is step 2; then steps 3 and 4.

## Logs

- **Android:** logcat, and daily files in the app's storage (root needed):
  `adb shell "su -c 'cat /data/data/dev.kroner.calvados/files/logs/calvados.$(date -u +%F).log'"`.
- **Linux:** `journalctl --user -u calvados -f`.

## Known issues

Seen on firmware 7.1.21; newer firmware may behave differently.

- **Switching back and forth quickly** occasionally reboots the AirPods.
  Both devices reconnect by themselves.
- **A switch can stall** for a few seconds right after the other device
  started playing; the laptop retries.
- **Putting the primary bud in the case** drops the link for a few seconds.

## Credits

- [LibrePods]: most of what's known about the AirPods protocol, the BLE
  advertisement, and the Android approach (both Bluetooth stack hooks).
- [openulla]: the firmware's AAP framing and command table.
- [uarp-dump], [ftab-dump], [linden/uarp]: the firmware's container formats.
- [Tabler Icons] (MIT): the icons.

[LibrePods]: https://github.com/librepods-org/librepods
[openulla]: https://github.com/mstampfli/openulla
[uarp-dump]: https://github.com/19h/uarp-dump
[ftab-dump]: https://github.com/19h/ftab-dump
[linden/uarp]: https://github.com/linden/uarp
[Tabler Icons]: https://tabler.io/icons

Developer notes: [`AGENTS.md`](AGENTS.md); the firmware research:
[`docs/firmware.md`](docs/firmware.md).
