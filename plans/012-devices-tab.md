# Plan 012: Devices tab

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: LOW. Read-only, event-driven. The risk is breadth: five buses.
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

What is plugged in or paired, and what state is it in. Stat Space's
Devices tab lists Bluetooth devices with battery levels, USB and
Thunderbolt devices, displays, audio devices and drives, with a history of
connections. The owner listed: Bluetooth, USB, Thunderbolt, displays,
audio.

## Linux data sources

No root for any of it.

| Data | Source |
|------|--------|
| Bluetooth adapter and devices | BlueZ on the system bus (`org.bluez` present **[machine]**, 5.87): `ObjectManager.GetManagedObjects`; `org.bluez.Device1` `Name`, `Alias`, `Icon`, `Connected`, `Paired`, `RSSI`; `org.bluez.Battery1` `Percentage`; `InterfacesAdded`, `PropertiesChanged` signals (https://github.com/bluez/bluez/tree/master/doc) |
| Peripheral batteries (non-Bluetooth) | `/sys/class/power_supply/*` with `scope` = `Device`: `capacity`, `capacity_level`, `model_name`, `status`. One wireless-receiver battery (`hidpp_battery_0`) present **[machine]**. UPower (`org.freedesktop.UPower` present) unifies these: `EnumerateDevices`, each `Type`, `Model`, `Percentage`, `State` |
| USB devices | `/sys/bus/usb/devices/<bus>-<port>…/`: `idVendor`, `idProduct`, `manufacturer`, `product`, `speed`, `bMaxPower`, `bDeviceClass`, `busnum`, `devnum`; interfaces' `bInterfaceClass` for the kind. 19 devices **[machine]** |
| USB names when the device gives none | `/usr/share/hwdata/usb.ids` (package `hwdata`) **[unverified]** present |
| Thunderbolt / USB4 | `/sys/bus/thunderbolt/devices/*`: `device_name`, `vendor_name`, `authorized`, `generation`, `rx_speed`, `tx_speed`; a domain and a host router are present **[machine]**. `org.freedesktop.bolt` is on the bus **[machine]** for authorisation state |
| USB-C power and roles | `/sys/class/typec/port*/{power_role,data_role}`, `/sys/class/usb_power_delivery/`. Neither exists on the development machine **[machine]**; coded behind a probe, tested with a fixture only |
| Displays | `/sys/class/drm/card*-*/{status,enabled,modes,edid}`; the EDID gives maker, model and physical size. Resolution in use, refresh, scale and position from the compositor: Hyprland `hyprctl monitors -j` **[unverified]** |
| Audio devices | PipeWire: `pw-dump` one-shot, objects with `media.class` `Audio/Sink`, `Audio/Source`, `Audio/Device` (5 sinks and 3 sources **[machine]**) |
| Drives | Plan 010's device list (removable ones appear here too) |
| Input devices | `/proc/bus/input/devices` (name, handlers) for keyboards, mice, controllers |
| Cameras | `/sys/class/video4linux/video*/name` |
| Hot-plug events | A `NETLINK_KOBJECT_UEVENT` socket (kernel uevents are readable unprivileged) for USB, Thunderbolt, DRM, block, power_supply; D-Bus signals for BlueZ and UPower. No polling |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| AirPods case and per-bud levels | Dropped: BlueZ exposes one percentage, when the device reports it at all |
| Battery of every Bluetooth device | Only those implementing the battery service or reporting through HID; others show no level |
| Charger wattage of a USB-C port | Only where `typec`/`usb_power_delivery` exist; absent otherwise |
| Display brightness, HDR state | Brightness through `/sys/class/backlight` for built-in panels (shown, not controlled here). HDR: dropped |
| Connect and disconnect Bluetooth | Kept, minimal: `Device1.Connect` / `Disconnect`. Pairing new devices is the desktop's job: dropped |

## Design

### Stream `devices`

Built once on subscribe, then updated only on events; no timer. Battery
percentages of peripherals refresh on their own change signals.

```json
{"bluetooth":{"powered":true,"devices":[{"id":"…","name":"Headphones","kind":"audio",
              "connected":true,"paired":true,"batteryPercent":80,"rssi":-50}]},
 "usb":[{"id":"1-2","name":"…","vendor":"…","kind":"hub","speedMbps":480,"maxPowerMa":100,
         "children":["1-2.1"]}],
 "thunderbolt":[{"id":"0-1","name":"…","vendor":"…","authorized":true,"generation":4}],
 "displays":[{"id":"DP-1","name":"…","maker":"…","connected":true,"builtin":false,
              "mode":"2560x1440","refreshHz":144,"scale":1.25,"sizeMm":[597,336]}],
 "audio":[{"id":51,"name":"…","kind":"sink","default":true}],
 "input":[{"name":"…","kind":"keyboard"}],
 "cameras":[{"name":"…"}],
 "batteries":[{"name":"Mouse","percent":60,"state":"discharging","level":"normal"}],
 "typec":[{"port":0,"powerRole":"sink","dataRole":"host","watts":null}]}
```

`id` values are bus positions, never serial numbers or MAC addresses.
Bluetooth addresses are hashed with a per-install salt for the join key
and are not in the stream; serials are never read.

Events: connects and disconnects go to `event` (`kind: "device"`, the
kind, the display name, connected or not), kept 30 days. Bluetooth device
names are user-visible data: they are stored in the local database and
never leave it (the health report omits them unless the user ticks
"include device names").

### Actions

`devices.bluetooth.connect {id}`, `…disconnect {id}`: reversible, no
confirmation. Nothing else. USB "eject" is plan 010's.

### Page

Sections, each hidden when empty: Bluetooth (with a battery meter per
device and a connect toggle), Peripheral batteries, Displays (a small
arrangement drawing from positions when the compositor gives them),
USB (a tree: hubs with their children, speed and power draw; a "slower
than it could be" hint when a SuperSpeed-capable device sits at 480 Mbps
is **dropped** unless the capability is readable, see Unverified),
Thunderbolt, Audio, Cameras and input, and "Recently" (the event log).

A low-battery notification for peripherals (below 15 %) is off by default.
Pinnable cards: `devices.batteries`, `devices.bluetooth`.

CLI: `mirqab devices [--json]`.

## Rejected

- **`lsusb`, `bluetoothctl`, `boltctl` parsing.** A process per refresh,
  text formats that change; sysfs and D-Bus are stable interfaces.
- **`libudev`.** A C dependency for what one netlink socket and sysfs
  reads give. The kernel uevent group is used rather than udev's
  processed group, because it needs no library to parse.
- **Polling sysfs.** Events exist.
- **Showing serial numbers.** Identifying, and often pasted into bug
  reports.
- **A Bluetooth pairing wizard.** Scope creep; the desktop has one.

## Tests and fixtures

Add to `root-desktop`: `sys/bus/usb/devices/` (a root hub, a hub with two
children, a storage device, a HID receiver, a device with no strings),
`sys/bus/thunderbolt/devices/`, `sys/class/drm/card1-DP-1/` with a
synthetic EDID built by a script (maker "ABC", a model name, a size),
`sys/class/power_supply/hidpp_battery_0/`, `proc/bus/input/devices`,
`sys/class/video4linux/`, `usr/share/hwdata/usb.ids` (ten lines).
`root-laptop` adds `sys/class/typec/` and a built-in panel with a
backlight. Fake BlueZ and UPower on the private bus; a stub `pw-dump` and
`hyprctl`. Uevents come from a recorded byte stream fed to the parser.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/devices.rs` | The USB tree; names from strings, then from `usb.ids`, then "USB device"; kinds from interface classes; EDID parsing; displays without a compositor still list from DRM |
| same | BlueZ objects to devices; battery present and absent; a `PropertiesChanged` updates one field with no rescan |
| same | Uevent parsing: add and remove produce one event row each; a storm of 100 events coalesces into one stream update |
| same | No serial, MAC or Bluetooth address appears anywhere in the stream or the event table |
| `tests/test-ui-devices.sh` (`mirqab-ui --selftest devices`, plan 001) | Sections present and hidden as the fixture dictates |
| `tools/bench-idle.sh --subscribe devices` | Indistinguishable from idle: no timer runs |

## Acceptance

1. Plugging in and removing a USB device updates the page within a second
   and adds two log rows, with the page open or closed-then-reopened.
2. Connected Bluetooth devices show, with a level where they report one.
3. Displays show maker, model, mode and scale on Hyprland.
4. The subscribed sampler adds no measurable CPU while nothing changes.

## Checklist

1. Resolve Unverified.
2. `crates/mirqab-core/src/uevent.rs` (the netlink socket, a parser, a
   50 ms coalescing window).
3. `crates/mirqab-core/src/samplers/devices/{mod,usb,thunderbolt,displays,edid,input,cameras,batteries,typec,audio}.rs`.
4. `crates/mirqab-core/src/dbus/{bluez,upower,bolt}.rs`.
5. `crates/mirqab-core/src/desktop/hyprland.rs`: `monitors` (extends plan
   006's file).
6. `tests/golden/devices.json`; fixtures; tests.
7. `crates/mirqab-view/src/tabs/devices.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/devices/{mod,page,bluetooth_section,batteries_section,displays_section,usb_tree,thunderbolt_section,audio_section,input_section,recent_list}.rs`.
8. CLI `devices`; the optional low-battery notification.

## Unverified

- Reading `NETLINK_KOBJECT_UEVENT` group 1 as an unprivileged user on the
  target kernels (it works on mainline; confirm with a five-line probe).
- `hyprctl monitors -j` keys (`name`, `width`, `height`, `refreshRate`,
  `scale`, `x`, `y`, `make`, `model`): `hyprctl` could not reach the
  compositor from the planning shell. Record a scrubbed sample.
- Whether a device's best possible USB speed is readable (to offer the
  "plugged into a slow port" hint): the SuperSpeed capability sits in the
  BOS descriptor, which sysfs exposes only as raw `descriptors`. Parse it
  or drop the hint.
- `hwdata`'s path and whether it is a dependency worth adding, or optional.
- The `org.freedesktop.bolt` object layout: `busctl introspect`.
