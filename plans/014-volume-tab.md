# Plan 014: Volume tab

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MEDIUM since the toolkit changed (owner, 2026-11-13).
  PipeWire does per-application volume natively, with no tap, no driver
  and no permission, but GTK has no audio module, so the page brings its
  own client library (below).
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

A volume slider for every app that plays sound, 0 to 150 %, remembered;
mute, "mute all but this", route an app to another output; which app is
using the microphone; output and input pickers with master volume; level
meters; and a list of recent sounds
(https://statspace.app/features/volume-mixer, read 2026-11-13). On macOS
this needs process taps and a recording permission. On Linux, PipeWire
gives every stream its own volume. The owner asked for "PipeWire per-app".

## Linux data sources

No root.

| Data | Source |
|------|--------|
| Streams, sinks, sources, their volumes and mutes | In the interface: the PulseAudio client API, served by `pipewire-pulse` (`pipewire-pulse 1:1.6.9-1`, `libpulse 17.0` installed **[machine]**), through the crates `libpulse-binding` 2.30.1 and `libpulse-glib-binding` 2.29.0 **[crates.io]**. Introspection: sink inputs (one per playing stream), sinks, sources, source outputs, server info (default sink and source); `subscribe` for change events |
| The app behind a stream | The sink input's property list: `application.name`, `application.process.binary`, `application.process.id`, `media.name`, `application.icon-name` |
| Set a stream's volume or mute | `set_sink_input_volume`, `set_sink_input_mute` |
| Level meters | A record stream on the sink's monitor (or on one sink input with `set_monitor_stream`) with the `PEAK_DETECT` flag, at a low rate: what `pavucontrol` does |
| Remembering per-app volume | WirePlumber's `restore-stream` does this already (0.5.18 **[machine]**), keyed by application |
| Route a stream to another sink | `move_sink_input_by_index` |
| Microphone in use | A source output that is not a peak-detect monitor and is not corked |
| Outside the interface (CLI, bar item, the "mic in use" attention state, plan 009's audio rule) | `pw-dump` one-shot through `exec.rs`, and `pw-dump --monitor` only while such a consumer is active; `wpctl set-volume` / `set-mute` for the CLI |
| Balance | Per-channel volumes of the sink's `ChannelVolumes` |

At planning time no stream was playing, so stream properties were read
from PipeWire's documentation, not from a live object
(https://docs.pipewire.org/page_man_pipewire-props_7.html).

## Not possible on Linux

Nothing is missing; several things are simpler:

| Stat Space item | Decision |
|-----------------|----------|
| Permission prompt before the first slider move | Not needed |
| The 50 ms of full volume before a tap takes over | Does not happen |
| "Recent sounds: which app made that beep" | Kept, but only while the tab is open or "Remember" is on (it needs the daemon's `pw-dump --monitor`); short notification sounds that start and end between events may be missed, and the card says so |
| Set an app's level before it plays | Limited: WirePlumber restores what it stored for apps it has seen; an app never seen has nothing to set. The row for a remembered-but-silent app is shown from the page's own list of the last 20 apps |

## Design

### The one exception to "all data comes from the daemon"

The Volume page talks to the sound server itself, in the interface
process, through the PulseAudio client API on GLib's main loop
(`libpulse-glib-binding` gives libpulse a main-loop implementation that
runs on GTK's own loop, so there is no thread and no second loop).
Reasons: the API is event-driven; sliders need feedback inside one frame;
proxying volumes through the daemon would re-implement the same library
and add a round trip to every slider tick. Architecture §7.2 names this
exception. Everything that must work with the window closed goes through
the daemon's `audio` stream instead.

The page code does not call the binding directly. `tabs/volume/server.rs`
defines a small trait (`list`, `set_volume`, `set_mute`, `move_to`,
`open_meter`, `close_meter`, and a change callback) with two
implementations: `pulse.rs` over the binding, and a fake in the tests.
The connection is opened when the page is mapped and closed when it is
unmapped; with the page closed the interface holds no connection to the
sound server.

Why the PulseAudio API on a PipeWire system: `pipewire-pulse` is
PipeWire's own server for it, it is what the desktop's volume tools use,
and it exposes exactly the per-stream operations this page needs as
plain calls. PipeWire's native API would mean building and parsing SPA
parameter objects for each volume change.

If the binding cannot be used (see Unverified), the fallback is recorded
now so the plan does not stall: the page reads the daemon's `audio`
stream and sends `audio.setVolume` / `audio.setMute` / `audio.move`
actions, which run `wpctl` or `pw-metadata` through `exec.rs`,
rate-limited to ten a second while a slider is dragged; level meters are
dropped in that case.

### Stream `audio` (daemon, light)

Only for the CLI, the bar item, the tray's attention state and rules.
Subscribed: one `pw-dump --monitor` child, parsed incrementally; unsubscribed:
no child.

```json
{"sink":{"name":"…","volumePercent":62,"muted":false},
 "source":{"name":"…","volumePercent":80,"muted":false},
 "playing":[{"app":"…","pid":1234,"volumePercent":100,"muted":false}],
 "micInUseBy":[{"app":"…","pid":1300}],
 "recent":[{"app":"…","at":0,"media":"…"}]}
```

### Page

1. A "Mic in use by …" banner when a capture stream runs.
2. Output: a device picker, the master slider (0 to 150 %, with a notch at
   100 it snaps to), mute, a balance control under a disclosure.
3. Input: a device picker, input volume, mute, an opt-in level meter (the
   meter opens the microphone, so it is a switch, off by default, and off
   again when the page closes).
4. Apps: one row per application (streams of one app are grouped by
   `application.process.binary`, falling back to `application.name`;
   browser tabs show `media.name` as the second line): icon, name,
   "Playing" or "Idle", slider 0 to 150 %, mute. A context menu: mute all
   but this, play through another output, reset to 100 %.
5. "Reset all to 100 %".
6. Recent sounds: the last ten, with the app.
7. Opt-in per-row level meters (a peak-detect record stream per row,
   drawn by the shared `Meter` widget), open only while the page is
   visible. A meter is the only thing on this page that redraws without
   the user acting; its stream is asked for about 15 updates a second and
   the widget redraws only when the level changed by a visible step.

Volumes above 100 % are amplification and can clip; the slider changes
colour past the notch. The cubic volume scale PipeWire uses is handled by
libpulse (`Volume` to and from a linear percent is the view model's job,
with the cubic mapping `wpctl` uses); the page works in the same percent the desktop's own OSD
shows, and a test pins that 50 % here equals 50 % in `wpctl`.

Pinnable card `volume.master` (master slider and the playing apps). Bar
item `volume`. CLI: `mirqab volume [N%|+N|-N|mute|unmute] [--app NAME]
[--json]`.

## Rejected

- **Driving everything through `wpctl` or `pactl` from the daemon.** A
  process per slider tick.
- **A PipeWire client in the Rust daemon (`pipewire-rs`).** A heavy
  binding and a main loop, for data the interface already has. `pw-dump`
  is enough for the daemon's light needs.
- **The native PipeWire crate in the interface (`pipewire` 0.10.1
  **[crates.io]**).** It generates its bindings with `bindgen`, so the
  build needs `clang`; it runs its own loop, which has to be joined to
  GLib's by hand; and a volume change is an SPA `Props` object to build.
  Three costs for the same result the PulseAudio API gives directly.
- **WirePlumber's library through a `wireplumber` crate.** The crate on
  the registry is 0.0.1 from 2023, a placeholder **[crates.io]**.
- **Quickshell's PipeWire module.** It went with the toolkit (owner,
  2026-11-13).
- **`wpctl` for everything, with no library.** Kept only as the fallback
  above: a process per slider tick and no meters.
- **Our own per-app volume store.** WirePlumber has one; two would fight.
- **EQ or effects.** Out of scope, as in the reference.
- **PulseAudio-only systems.** Not supported; the tab is hidden without
  PipeWire.

## Tests and fixtures

`tests/fixtures/pw-dump/*.json`: synthetic dumps (two sinks, a source, a
music player stream, a browser with two streams, a capture stream; and an
empty one). The page is tested against the fake implementation of the
`server.rs` trait, which records every call; no sound server runs in the
tests.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/audio.rs` | Dump to the `audio` shape; grouping; mic in use; incremental `--monitor` updates; no child process without a subscriber |
| `crates/mirqab-view/tests/volume.rs` | grouping streams into apps, snap to 100, "mute all but", the percent mapping |
| `tests/test-ui-volume.sh` (`mirqab-ui --selftest volume`, plan 001) | The page against the fake server (`MIRQAB_TEST=1` selects it): rows, the banner, a slider change is recorded as one `set_volume` with the expected value; after the window hides the fake reports no open meter and a closed connection |
| `tests/test-cli.sh` | `volume` argument parsing; the `wpctl` argv recorded by the stub |

## Acceptance

1. Two apps playing get two sliders; moving one changes only that app,
   and the level is back after the app restarts.
2. "Play through another output" moves the stream.
3. Starting a recording shows the banner with the app's name.
4. Closing the page leaves no peak-detect stream, no microphone capture,
   no connection to the sound server (`pactl list clients` does not list
   mirqab) and no `pw-dump` child running.

## Checklist

1. Resolve Unverified with a stream playing; record the property names
   seen and a scrubbed `pw-dump` fragment as the fixture's basis.
2. `crates/mirqab-view/src/tabs/volume.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/volume/{mod,page,output_card,input_card,app_row,app_menu,recent_list,mic_banner,master_card}.rs`.
3. `crates/mirqab-ui/src/tabs/volume/{server,pulse}.rs` and the fake in
   `crates/mirqab-ui/tests/volume.rs`. Add `libpulse-binding` 2.30.1 and
   `libpulse-glib-binding` 2.29.0 to `mirqab-ui` only, with their
   licences checked by `cargo deny` and the binary-size cost recorded
   here.
4. `crates/mirqab-core/src/samplers/audio/{mod,pwdump}.rs`; the stream,
   golden file and tests.
5. CLI `volume`, bar item, tray attention for the microphone (off by
   default).
6. Update architecture §7.2 if the exception's wording needs it.

## Unverified

- **The binding builds and runs on GTK's loop.** `libpulse-glib-binding`
  2.29.0 declares `glib` as a dependency with an open version range;
  confirm it resolves to the same `glib` 0.22 that `gtk4` 0.11 uses (one
  `glib` in `cargo tree`), and that a context created from its main loop
  delivers callbacks with no thread of ours. If it does not, use
  libpulse's threaded main loop and hand results to GLib with
  `glib::idle_add_once`; if that also
  fails, take the `wpctl` fallback above.
- **Optional or required.** Decided by the owner (2026-11-13): `libpulse`
  is a hard dependency of the package, linked normally; no feature flag
  and no `dlopen`. The tab is hidden when no PulseAudio server answers.
- **The percent.** That 50 % set here reads as 50 % in `wpctl
  get-volume` and in the desktop's OSD (cubic mapping); pin it in a view
  model test from recorded pairs.
- **Restore.** That WirePlumber's `restore-stream` stores a volume set
  through the PulseAudio API, and whether it keys on `application.name`
  or the binary: set a level, restart the app, read it back.
- **Routing.** That `move_sink_input_by_index` survives the stream's
  next restart, or whether the target must also be written as PipeWire
  metadata.
- That grouping by `application.process.binary` puts a browser's tabs
  together and a Flatpak app's streams under the app, with real streams.
- **Meters' cost.** CPU of one peak-detect stream per row at 15 updates
  a second; if it is noticeable, meters stay opt-in per row.
