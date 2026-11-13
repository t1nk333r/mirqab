# mirqab — architecture

This is the contract between the parts. Plans in `plans/` change it only by
editing this file in the same pull request.

The name is `mirqab` (مرقاب, a watch-post). The owner confirmed it on
2026-11-13, together with the other decisions recorded in
`plans/README.md`, "Owner decisions". The repository is public at
`github.com/t1nk333r/mirqab`.

Evidence marked **[machine]** was read on the development machine on
2026-11-13. That machine is Arch-based: `/etc/os-release` reports
`ID=omarchy`, `ID_LIKE=arch`; kernel 7.2.7; Hyprland session. All testing
happens there, so this document says "Arch-based" and never names a
particular derivative as tested. Evidence marked **[crates.io]** is a
crate version read from the registry on the same day (`cargo search`,
`cargo info`). Evidence marked **[unverified]** could not be checked; the
plan that depends on it starts by checking it and recording the result.

## 1. What this is

A Linux counterpart of the macOS app Stat Space (https://statspace.app/): a
system monitor and developer toolkit in one window, with a tray icon, bar
items, a CLI and an MCP server. It is a standalone app in its own
repository. It is not part of haseen and does not need haseen to run.

Stat Space is proprietary. It is the reference for behaviour and layout
only. Nothing is copied from it: no code, no text, no icons, no artwork. Its
site also sells a build prompt; that prompt was not obtained and must not be
used.

### 1.1 Scope

Every Stat Space tab that Linux allows, with no paid tier:

| Tab | Plan | Notes |
|-----|------|-------|
| Home | 003 | Pinned cards from any tab |
| Apps | 006 | Process inspector mapped to cgroups and `/proc` |
| System | 002 | CPU, memory, pressure, GPU, temperatures, power |
| Fans | 015 | hwmon; control only through the opt-in root helper |
| Disk | 010 | Volumes, I/O, SMART, cache cleanup |
| Network | 011 | Throughput, Wi-Fi, tools |
| Devices | 012 | Bluetooth, USB, Thunderbolt, displays, audio, drives |
| Battery | 013 | Shown only when the machine has a system battery |
| Volume | 014 | PipeWire per-app volume |
| Ports | 007 | Listening sockets traced to project and launcher |
| Agents | 008 | Coding-agent sessions and leftovers |
| Awake | 009 | logind inhibitors |
| Time | 016 | World clock, map, sun and moon |
| Weather | 017 | Open-Meteo, coarse coordinates |
| Trends | 018 | Free here (Pro in Stat Space) |
| Support | 019 | Health report and diagnostics |

### 1.2 Paid items: free or dropped

There is no licence, no account, no "Unlock" anywhere. Each Stat Space Pro
item is either built free or dropped. The list of Pro items comes from
https://statspace.app/ (pricing section, read 2026-11-13).

| Stat Space Pro item | Decision | Why |
|---------------------|----------|-----|
| Trends | Free, plan 018 | It only reads the history the daemon already keeps |
| Spike rewind | Free, plan 018 | Same data; one `event` row per spike |
| "Why is it slow?" | Free, plan 018 | Rules over the live snapshot and history; also the MCP tool `explain_slowness` |
| Network by app | Free, later phase of plan 011 | TCP only, own processes only, through `sock_diag`; recorded limits |
| Keep-awake rules | Free, later phase of plan 009 | Charger, task and download rules are small |
| Docker port mapping | Free, plan 007 | `docker ps` already gives it |
| App alerts | Free, plan 018 | Notifications over D-Bus |
| Extra weather cities | Free, plan 017 | No cost per city beyond one request |
| Rain notifications | Free where data exists, plan 017 | `minutely_15` covers only some regions |
| Battery drain per app | Dropped | Linux has no per-process energy accounting without eBPF or root |
| "Who's phoning home" | Dropped | Needs packet capture or eBPF; out of scope |
| Severe weather alerts | Dropped | No keyless worldwide source |
| Team planner | Dropped | Owner's instruction |
| Today's meetings | Dropped | Needs calendar accounts |
| Pro tab, licences | Dropped | Owner's instruction |
| Support tab | Free, plan 019 | The health report is free in Stat Space too |

Also dropped, free in Stat Space but not sensible here: the twelve menu bar
"looks" and share links, desktop widgets, in-app global shortcuts (the
compositor binds keys), the Sparkle updater (pacman updates the package).
Each is recorded with its reason in the plan nearest to it.

## 2. Stack

**Chosen: Rust throughout. A daemon, CLI and MCP server in one binary, and
a GTK4/libadwaita interface (`gtk4-rs`) in a second binary that exists
only while it is open.** The owner chose the toolkit on 2026-11-13.

### 2.1 Why Rust for the daemon

- haseen is moving its own sidecar from Go to Rust (haseen plan 167, PR
  #128). Measured there on the same code: Go 11.7 MiB RSS and 8 threads
  idle, Rust 2.7 MiB and 3 threads idle, 3.2 MiB while sampling. The idle
  budget in §9 is easy for the second and tight for the first.
- One static-ish binary, no runtime, blocking I/O on a few threads. No
  async runtime: the daemon has at most a handful of clients.
- rustc 1.99.0 and cargo 1.99.0 are installed **[machine]**.

### 2.2 Why GTK4 and libadwaita for the interface

- **The owner chose it** (2026-11-13), over the Quickshell/QML design this
  document first proposed.
- One language. The interface shares the protocol types and the view
  logic with the daemon as Rust crates; there is no second toolchain, no
  JavaScript and no QML lint.
- No dependency on a desktop shell. The interface needs `gtk4`,
  `libadwaita` and `gtk4-layer-shell`, all in Arch's `extra`.
- libadwaita gives the settings rows, dialogs, toasts, the dark scheme
  and an accessibility tree for free.

Versions, system and crate side by side:

| System library **[machine]** | Crate **[crates.io]** | Feature flags |
|------------------------------|-----------------------|---------------|
| `gtk4` 4.24.1 | `gtk4` 0.11.5 (MSRV 1.92; rustc here is 1.99.0) | `v4_16`: the floor we build against (CSS variables, `GskPath` drawing). The crate has flags up to `v4_24`; a higher floor is taken only when a plan needs an API from it |
| `libadwaita` 1.10.0 | `libadwaita` 0.9.2 | `v1_6`: the floor (CSS variables for the named colours). Flags exist up to `v1_10` |
| `gtk4-layer-shell` 1.3.0 | `gtk4-layer-shell` 0.8.1 (needs `gtk4` ^0.11) | `v1_3` only if a plan needs it |
| `glib` 2.x, `cairo` | `glib` 0.22.10, `cairo-rs` 0.22.9 | Come with `gtk4` 0.11; never pinned separately |

The feature floor is lower than what the development machine has on
purpose: a feature flag is a minimum, and the package's `depends` states
the same minimum (`gtk4>=4.16`, `libadwaita>=1.6`).

What the choice costs is recorded in §2.4.

### 2.3 Rejected

| Option | Why rejected |
|--------|--------------|
| Go daemon | The owner is leaving Go for this job (haseen plan 167); four times the idle memory |
| Quickshell/QML interface (`qs -p`, Quickshell 0.3.2) | **The owner chose GTK4** (2026-11-13). It was this design's first proposal: it had a layer-shell popover, a socket type and a PipeWire module ready-made, and haseen's QML graph and QR code could have been adapted. It cost a 122 MiB process (haseen `docs/architecture.md` §6), a second language, and a dependency on a shell that most desktops do not have installed |
| Plain Qt Quick app with a C++ shim | A third language and a build system; rejected before the toolkit decision and still rejected |
| Qt Widgets | No layer-shell without `layer-shell-qt` glue in C++, dated look, every chart hand-painted in C++ |
| GTK4 without libadwaita | Every settings row, dialog and colour scheme switch written by hand; libadwaita is one more library that every GNOME app already loads |
| Relm4 on top of `gtk4-rs` | A second abstraction to learn and pin; the pages here are lists and graphs fed by one socket, which plain `gtk4-rs` handles |
| `.ui` files or Blueprint | A second language again, and Blueprint needs its compiler at build time. Widgets are built in Rust |
| Tauri | Needs webkit2gtk (a browser engine resident while open), layer-shell is awkward, a third toolchain (Node) |
| egui, iced | Repaint continuously by default or have a weak layer-shell story; no accessibility tree to speak of |
| The interface inside the daemon process | The history, the CLI and the MCP server must work with no window open, and a process with GTK loaded cannot idle under the budget (§3.1) |
| A haseen plugin instead of an app | The owner asked for a standalone app in a new repository |

### 2.4 What the toolkit choice made harder

Recorded so nobody rediscovers it. Each item names the plan that carries
the work.

| Item | Before (Quickshell) | Now (GTK4) | Plan |
|------|---------------------|------------|------|
| Tray icon | Not possible from the interface | Still not possible from the interface: GTK4 removed `GtkStatusIcon` and has no tray API. The daemon serves the StatusNotifierItem over D-Bus with the `ksni` crate. Unchanged in effect | 004 |
| Layer-shell popover | Built in (`PanelWindow`) | The `gtk4-layer-shell` library. It must be loaded before `libwayland-client`, which depends on link order; where the linker gets it wrong the unit sets `LD_PRELOAD`. **[unverified]** on this machine | 001 |
| Per-app volume | Built in (`Quickshell.Services.Pipewire`) | No ready binding. The Volume page uses the PulseAudio client API (`libpulse-binding`), which `pipewire-pulse` serves, on GLib's main loop: what `pavucontrol` does | 014 |
| Graphs, QR code, dotted map | haseen's QML could be adapted | Written new as custom widgets that draw with `GtkSnapshot`. Nothing is adapted from haseen's QML; its *arithmetic* (moon phase, WMO codes) still can be | 001, 007, 016 |
| haseen bar plugin sharing models | Shared QML | No sharing. The plugin stays QML, because haseen is Quickshell, and reads `mirqab bar <item> --watch` like any other bar | 004 |
| Theme following a desktop's token file | Tokens were plain QML properties | Tokens become CSS variables written into one generated stylesheet; custom-drawn widgets read the same token table | 001 |
| Headless interface tests | `qs` under Qt's offscreen platform | GTK has no offscreen backend for windows. Tests use the Broadway backend (`gtk4-broadwayd`, shipped inside `gtk4` **[machine]**); layer-shell cannot be exercised there | 001 |
| Fractional scale bugs | Canvas and layers were wrong at 1.25 on Qt's software renderer | Does not apply to GTK. The "no Canvas, no layers" rule is gone; "no blur" stays | — |

Nothing became impossible.

## 3. Processes

```
                      ┌──────────────────────────────────────────┐
  mirqab <command> ──▶│ mirqab daemon   (user service, always on) │
  mirqab --mcp     ──▶│  samplers · history.db · tray item · rules│
  mirqab bar …     ──▶│  socket: $XDG_RUNTIME_DIR/mirqab/daemon.sock
  bar plugins      ──▶└───────────────┬──────────────────────────┘
                                      │ same socket          │ /run/mirqab-fand.sock
                      ┌───────────────┴────────┐   ┌─────────┴───────────────┐
                      │ mirqab-ui (GTK4 binary)│   │ mirqab-fand (root, opt-in)│
                      │ exists only while open │   │ pwm writes, RAPL reads   │
                      └────────────────────────┘   └──────────────────────────┘
```

| Process | Runs as | Lifetime | Started by |
|---------|---------|----------|------------|
| `mirqab daemon` | the user | Session | `mirqab.socket` (socket activation) or `mirqab.service` |
| `mirqab-ui` (`/usr/bin/mirqab-ui`) | the user | While the window is open, plus 60 s | The daemon, through `systemctl --user start mirqab-ui.service` |
| `mirqab <command>`, `mirqab --mcp`, `mirqab bar` | the user | The command | The user, an agent or a bar |
| `mirqab-fand` | root | Boot, only if enabled | `mirqab-fand.service`, disabled by default |

Three binaries. `mirqab` holds the daemon, the CLI and the MCP server (the
first argument picks). `mirqab-ui` is the interface. `mirqab-fand` is the
root helper, with its own dependency list so the root process carries as
little code as possible.

### 3.1 Why the interface is its own binary and its own process

The daemon is headless: `mirqab` links no GTK, GLib, Pango or Wayland
library, and lint checks it (`cargo tree -p mirqab` must not name `gtk4`,
`glib` or `libadwaita`; `ldd` on the release binary must not list
`libgtk-4`).

- **The idle budget (§9).** A process that has loaded GTK maps dozens of
  shared libraries and starts font, icon and settings machinery. Loaded
  into the daemon, that is resident for the whole session whether or not
  the window is ever opened, and the 15 MiB budget is gone before any
  sampler runs. Loaded into `mirqab`, every `mirqab bar` and `mirqab
  ports` call would also pay the dynamic loader for libraries it never
  uses.
- **A window that is closed costs nothing.** `mirqab-ui` exits 60 s after
  the window was last hidden. With the window closed there is one process,
  the daemon, and the under 0.3 % CPU target is measured on exactly that
  (§9.1).
- **A crash in drawing code cannot stop the history.** The minute writer
  and the rules live in the daemon.

Rejected: one binary with a `ui` subcommand (the same file would then link
GTK, so the daemon would map it too); a long-lived interface process that
only hides its window (resident memory for nothing); GApplication D-Bus
activation instead of the systemd unit (a second way to start the same
thing, and the daemon already talks to systemd).

`mirqab-ui` is a single-instance `AdwApplication`. Started a second time
it hands its arguments to the running instance and exits.

## 4. Window model

Four ways in, one daemon behind them.

1. **Popover (default).** An `AdwApplicationWindow` turned into a
   layer-shell surface by `gtk4-layer-shell` before it is first shown:
   layer `top`, anchored to a screen corner or edge chosen in Settings
   (default: top right, with a margin that clears the bar), keyboard mode
   `on-demand`, namespace `mirqab`. It closes on Escape. A click outside
   closes it too: the compositor moves keyboard focus away, the window's
   `is-active` property turns false, and the window hides. A "Pinned"
   toggle keeps it open and stops that close.
2. **Window.** `mirqab open --window`, or a Settings switch, shows the same
   content in an ordinary `AdwApplicationWindow` (an xdg-toplevel) that
   the compositor tiles or floats like any app. The `.desktop` entry opens
   this mode, because a launcher has no anchor to put a popover under.
   Whether a window is a layer surface is fixed when it is first shown, so
   switching mode builds a new window around the same content widget.
3. **Tray icon.** One StatusNotifierItem, owned by the daemon and served
   over D-Bus (`org.kde.StatusNotifierWatcher` is on the session bus
   **[machine]**). Left click toggles the popover, the menu lists tabs and
   Quit. GTK4 has no tray API (`GtkStatusIcon` was removed in GTK 4), and
   the interface process is short-lived anyway, so the daemon serves it,
   with the `ksni` crate. Plan 004.
4. **Bar items.** `mirqab bar <item> --watch` prints one JSON line per
   change in Waybar's custom-module format, which other bars also read. A
   haseen bar-widget plugin lives in this repository under
   `contrib/haseen/` (owner, 2026-11-13) and is installed by the owner
   with `haseen plugin install`; haseen never bundles it. The plugin is
   QML because haseen is; it is the only QML in this repository, and it
   holds no logic beyond reading those JSON lines. Plan 004.

Opening is `mirqab open [tab]`, `mirqab toggle [tab]` and `mirqab close`.
The user binds them in the compositor. The daemon starts the interface unit
if it is not running, then publishes on the `ui` stream; the interface
subscribes to `ui` and shows, hides or switches tab.

The interface process quits 60 s after the window was last hidden, so an
open-close-open is instant and an idle session pays nothing. The 60 s timer
is a one-shot marked `// mirqab:ui-timeout` (§7.2).

Rejected: an always-visible strip of tiles (visual clutter with no reason),
global shortcuts grabbed by the app (the compositor's job on Wayland), an
X11 fallback (the owner's sessions are Wayland), a `GtkPopover` for the
popover (it needs a parent window to hang from).

**On Hyprland** (where the app is developed and tested): the popover is a
layer surface, so window rules do not apply to it and it needs none; the
compositor must not blur it, which is its default for a layer with no
`layerrule`. The window mode is a normal toplevel with app id
`io.github.t1nk333r.mirqab`; the docs give one optional rule to float it.
The interface never calls `hyprctl`. Anything else Hyprland-only is listed
as such in its plan.

**Other compositors.** The popover needs `wlr-layer-shell`.
`gtk4_layer_shell::is_supported()` answers before the first window is
built; where it is false (GNOME) the interface uses window mode and says
so once in Settings.

## 5. The daemon

### 5.1 Socket

- Path: `$XDG_RUNTIME_DIR/mirqab/daemon.sock`. Directory mode 0700, socket
  mode 0600. The daemon refuses to start if `XDG_RUNTIME_DIR` is unset or
  not owned by the user.
- `SO_PEERCRED` is checked on accept: a peer with another uid is closed.
- systemd units: `mirqab.socket` (`ListenStream=%t/mirqab/daemon.sock`,
  `SocketMode=0600`, `DirectoryMode=0700`) and `mirqab.service`
  (`Type=notify`). Run by hand, the daemon prints `ready <socket path>` on
  stdout once it listens. A client connects only after that line or after
  the unit is active, and retries with a backoff (250 ms doubling to 4 s)
  if the daemon restarts.
- The interface reads the socket on GLib's main loop (`gio`'s Unix socket
  client and a line reader), not on a thread of its own.

### 5.2 Protocol

JSON, one object per line, UTF-8, both directions. The frame shapes are
adapted from haseen's `core/internal/proto/proto.go`, which is adapted from
DankMaterialShell (MIT); `NOTICE.md` carries both notices.

```
← {"type":"hello","protocol":1,"version":"0.1.0","pid":123,"capabilities":["system","history"]}
→ {"id":1,"method":"subscribe","stream":"system","params":{"interval":1000,"live":true}}
← {"type":"reply","id":1,"result":{"stream":"system","interval":1000}}
← {"type":"event","stream":"system","data":{…}}
→ {"id":2,"method":"action","params":{"name":"ports.stop","args":{"port":3000},"dryRun":true}}
← {"type":"reply","id":2,"result":{"wouldDo":[{"signal":"TERM","pid":4242,"name":"vite"}]}}
← {"type":"reply","id":3,"error":{"code":"refused","message":"pid 1 belongs to another user"}}
```

| Method | Params | Result |
|--------|--------|--------|
| `subscribe` | `stream`, `params.interval` (ms, optional), `params.live` (bool, optional; §5.3). Subscribing again to the same stream replaces the params | The effective interval; then events |
| `unsubscribe` | `stream` | — |
| `get` | `stream`, `params` | One snapshot, no subscription |
| `history.query` | `series[]`, `range` (`1h`, `24h`, `7d`, `30d`) or `from`/`to` (unix seconds) | Per series its `unit` and `points`, each `[unixSeconds, avg, max]` unscaled; empty buckets omitted |
| `history.events` | `kind`, `from`, `to` | Event rows |
| `history.list` | — | The series history keeps: key, unit, scale |
| `action` | `name`, `args`, `dryRun` | What was done, or `wouldDo` |
| `config.get` / `config.set` | key paths | The config; the daemon is the only writer |
| `capabilities` | — | The list from `hello`: the streams this machine serves, plus `history` when the history file is open (a capability, not a stream) |
| `status` | — | Version, uptime, subscribers, self-cost |
| `shutdown` | — | — |

Rules:

- `protocol` changes only when a frame shape changes. New streams, fields
  and actions do not bump it; clients ignore unknown fields.
- A stream that this machine cannot serve is absent from `capabilities`,
  and the interface hides or greys the tab with the reason (`status`
  returns `unavailable: {stream: reason}`).
- Every stream's `data` shape is documented in the plan that adds it and
  pinned by a golden file in `tests/golden/<stream>.json`.
- Error codes are a closed list: `bad-request`, `unknown-method`,
  `unknown-stream`, `unavailable`, `refused`, `needs-confirm`,
  `needs-helper`, `failed`.

### 5.3 Streams and samplers

Streams: `system`, `apps`, `ports`, `agents`, `awake`, `disk`, `net`,
`devices`, `battery`, `audio`, `fans`, `time`, `weather`, `alerts`, `self`,
`ui`. One sampler per stream.

- **A sampler runs only while something subscribes to it**: an open tab, a
  pinned Home card, a bar item, the tray tooltip or an enabled rule. With no
  subscriber the sampler's thread sleeps on a condition variable and reads
  nothing.
- Several subscribers share one sampler at the shortest requested interval.
- **Live sampling is 1 s, and only while a live graph is on screen**
  (owner, 2026-11-13). A subscription with `live: true` may ask for
  **1000 ms**, the lowest interval anywhere. The interface sets `live`
  only from a graph widget that shows the live range and is mapped in a
  visible window, and it drops the flag (by subscribing again without it,
  or unsubscribing) when the graph is unmapped: the page changed, the
  range picker left "live", or the window hid. A dropped connection drops
  its flags.
- **Everything else keeps its own slower interval.** Without `live` the
  floor is **2000 ms** and a shorter request is raised to it. Bar items
  (default 5 s), the tray tooltip, `--watch` commands, rules, and cards
  that show a number but no live graph are never `live`. Each stream's
  plan states its interval: 2 s for rates with no graph on screen, 5 s
  for ports and agents, 30 s for disk space, 30 minutes for weather.
- Only streams whose plan draws a live graph accept `live`: `system`,
  `net`, `disk`, `battery`, `fans`, `self`, and the Apps inspector's
  single-process sample. On any other stream the flag is ignored.
- History is not affected: it stays one value a minute (§6).
- Event-driven sources do not poll: logind, UPower, BlueZ, UDisks2 and bolt
  signals over D-Bus, udev over netlink, pidfd for process exit.
- Every sampler takes a `Root`, the directory that stands for `/`. Tests
  pass a fixture tree; production passes `/`. No sampler builds a path that
  does not go through `Root` (lint checks for string literals starting
  `"/proc`, `"/sys`, `"/etc`, `"/run`, `"/dev` outside `root.rs`).
- External commands go through one module, `exec.rs`: fixed argv, no shell,
  a timeout, `LC_ALL=C`. Tests put stubs first on `PATH`.

### 5.4 Actions and the dry-run contract

Every state change is an `action`. This is haseen's dry-run contract moved
to a daemon:

- Every action accepts `dryRun: true` and returns `wouldDo` without
  changing anything. Every mutating CLI command takes `--dry-run`.
- All mutation goes through `crates/mirqab-core/src/act.rs` (signals,
  file removal, D-Bus calls that change state, helper requests). Lint
  greps for `kill(`, `remove_file`, `remove_dir`, `Command::new` outside
  `act.rs` and `exec.rs`.
- The strings `sudo` and `pkexec` appear in exactly one file,
  `crates/mirqab/src/elevate.rs`, which only ever runs the fixed commands
  listed in §8. Check: `grep -rn 'sudo\|pkexec' crates tools dist | grep -v elevate.rs`.
- Destructive actions (`*.stop`, `*.kill`, `*.cleanup`, `disk.clean`,
  `disk.eject`) need `confirm: true`. Without it the reply is
  `needs-confirm` plus the same `wouldDo`. The interface shows the list in
  a dialog; the CLI asks `[y/N]` unless `-y`; the MCP server returns the
  list and acts only when the tool call sets `confirm`.
- Refused always, whatever the caller says: signalling a process of another
  uid, pid 1, the daemon itself, the session's compositor, and anything
  outside the user's own cgroup subtree.

### 5.5 Configuration and state

| What | Where |
|------|-------|
| Config | `~/.config/mirqab/config.json`, schema `share/mirqab/config.schema.json` |
| Theme override | `~/.config/mirqab/theme.json` (token names to colours; §7.3) |
| History | `~/.local/state/mirqab/history.db` |
| Cache (weather, geocoding) | `~/.cache/mirqab/` |
| Shipped defaults | `/usr/share/mirqab/` |

XDG variables override each directory. The daemon is the only writer of
`config.json`; the interface and CLI change settings through `config.set`,
and every subscriber to `ui` gets a `config-changed` event. A config the
daemon cannot parse is left untouched, reported in `status`, and defaults
are used: the app never rewrites a file the user broke by hand.

### 5.6 D-Bus and HTTP

- D-Bus through the `zbus` crate's blocking API, **pinned to the 5.x line
  (5.19.0 [crates.io])**. 6.3.0 is the newest, but `ksni` 0.3.7, which
  serves the tray item (§4), requires `zbus ^5`, and two copies of a
  D-Bus stack in one daemon is worse than being one major version behind.
  `cargo deny` refuses a second `zbus`. Both move to 6 together when
  `ksni` does. The exact API used is **[unverified]**; plan 001 records
  it. Rejected: shelling out to `busctl` (a process per call, no
  signals), the `libdbus` binding (C dependency).
- HTTP through `curl`, spawned by `exec.rs` with a fixed argv,
  `--proto =https`, `--max-time`, and `--config -` on stdin so no header or
  token ever appears in a process list. Rejected: a Rust HTTP and TLS stack
  (a large dependency closure for three endpoints). Tests use a stub `curl`.
- The network is touched only by: Weather (Open-Meteo), city search
  (Open-Meteo geocoding), agent usage limits when the user turned them
  on, and two things the user starts by hand and must first configure
  (speed test, public IP). §10 lists every outbound request.

## 6. History

One SQLite file, written by the daemon only, opened read-only by nothing
else (clients ask the daemon). `rusqlite` against the system `libsqlite3`
(3.54.0 **[machine]**), `journal_mode=WAL`, `synchronous=NORMAL`.

### 6.1 What is kept

Per-minute values for 30 days of: CPU (total, user, system, iowait), load,
memory (used, available, swap used), pressure (cpu, memory, io `some
avg60`), paging (page-ins, page-outs, swap-ins, swap-outs per minute), GPU
busy and VRAM per card, the hottest CPU and GPU temperature, fan speeds,
power (package watts when readable, battery watts when present), CPU clock
(average MHz), network bytes per interface, disk bytes read and written per
device, battery level. Plan 001 fixes the series names; a tab's plan may add
series, and the cap is 64.

Also kept: bytes per interface per day for 90 days (`daily`), and events
for 30 days (`event`): sleep blockers seen, devices connecting, agent
sessions starting and ending, spikes, alerts fired.

### 6.2 Schema

```sql
CREATE TABLE series (id INTEGER PRIMARY KEY, key TEXT UNIQUE NOT NULL, unit TEXT NOT NULL, scale INTEGER NOT NULL);
CREATE TABLE sample (series INTEGER NOT NULL, ts INTEGER NOT NULL, v INTEGER NOT NULL,
                     PRIMARY KEY (series, ts)) WITHOUT ROWID;      -- ts = minutes since 2026-01-01 UTC
CREATE TABLE daily  (key TEXT NOT NULL, day INTEGER NOT NULL, v INTEGER NOT NULL,
                     PRIMARY KEY (key, day)) WITHOUT ROWID;        -- day = unix day, local
CREATE TABLE event  (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX event_kind_ts ON event (kind, ts);
CREATE TABLE meta   (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- schema version
```

Values are integers scaled by `series.scale` (percent ×100, °C ×10, bytes
×1). A row's `ts` counts minutes from 2026-01-01 00:00 UTC
(`history::EPOCH`), which fits SQLite's 3-byte integer for 15 years where a
unix minute needs 4; the epoch is on a 2-hour boundary, so range buckets
align as they would to unix time. A row is labelled with the minute its
values cover.

64 series × 43,200 minutes = 2.76 M rows. The budget is **40 MiB** (owner,
2026-11-13) for the file and its write-ahead log together. Measured
(plan 001, "Phase C as built"): 38.4 MiB with one series in four holding
byte counts, packed by the daily compaction (§6.3); 43.3 MB never
compacted. A test fills 30 days the way the writer does and checks the
size without a manual checkpoint.

Rejected: one wide row per minute (every new series is a migration), a
round-robin file format (a second storage engine for no gain), keeping
history in the interface (gone when the window closes).

### 6.3 The minute writer

One wakeup a minute, aligned to the minute, one transaction. This is the
only work the daemon does with no subscribers. Woken just after a minute
boundary, it labels the values with the minute that just ended, so the
minute running now is never half-written in a query.

- **Counters** (CPU jiffies, network and disk bytes, paging counts, energy)
  are read once a minute and differenced, so the stored value is the true
  average over that minute at the cost of one read.
- **Gauges** (memory, temperatures, fan speed, GPU busy, clock) are one
  point sample at the minute, unless a live sampler was running during it,
  in which case the minute's mean of the live samples is stored. Stat Space
  stores per-minute averages; a true average of a gauge needs sampling all
  minute, which the idle budget forbids. Recorded as a difference.
- Pressure uses the kernel's own `avg60`, which is already a minute's mean.
- A gap (suspend, daemon stopped) stores nothing; graphs draw a break.
- Pruning runs once an hour: `DELETE … WHERE ts < now − 30 d` for `sample`
  and `event`, 90 d for `daily`.
- Compaction runs once a day (`VACUUM`, then a truncating checkpoint):
  rows arrive series by series, so each series' run grows in the middle of
  the key and its pages split half full. The time of the last compaction is
  kept in the file's `meta`, so a machine shut down every night still
  compacts. The write-ahead log is checkpointed every 256 pages and cut
  back to 1 MiB, so it adds at most about a megabyte.

### 6.4 Ranges

| Range | Source | Bucket | Points |
|-------|--------|--------|--------|
| 2 min ("live") | In-memory ring of live samples, exists only while a live graph is on screen | 1 s | 120 |
| 1 h | `sample` | 1 min | 60 |
| 24 h | `sample` | 4 min | 360 |
| 7 d | `sample` | 30 min | 336 |
| 30 d | `sample` | 2 h | 360 |

`history.query` returns `avg` and `max` per bucket, so a short spike stays
visible at 30 d. The 2 min range arrives on the live stream itself (each
event carries the newest point; the interface keeps the ring). Choosing
any other range on a graph ends its `live` subscription (§5.3): a graph of
stored minutes is redrawn once a minute, not once a second.

## 7. The interface

### 7.1 Layout

Taken from the Stat Space screenshots the owner supplied, redrawn with our
own tokens, icons and words:

- **Sidebar**, left: one tile per tab, icon above a short label, the active
  tile filled with the accent. Order: Home, Apps, System, Fans, Disk,
  Network, Devices, Battery, Volume, Ports, Agents, Awake, Time, Weather;
  then, pushed to the bottom, Trends and Support. A tile can carry a count
  badge (Ports: listeners; Agents: sessions that need the user). Tabs the
  machine cannot serve are hidden (Battery, Fans without sensors).
- **Page**, right: a scrolling column of cards with rounded corners. A card
  has a title, an optional range picker (2m, 1h, 24h, 7d, 30d) and a
  "Pin to Home" control.
- **Header**: the page title and the "Pinned" toggle for the popover.
- **Footer**: `self 0.2% · 14 MB` (daemon plus interface; the tooltip
  splits them), the machine line (CPU model, uptime), a health shortcut to
  Support, Settings, and a menu (window mode, quit daemon).

Popover size: 58 em × 40 em by default, clamped to the screen. Sizes are in
em of the base font so the fractional-scale case works.

### 7.2 Rules

- **Two crates, split by whether GTK is needed.**
  `crates/mirqab-view` holds everything that can be decided without a
  widget: one view model per tab (`src/tabs/<tab>.rs`: stream data in,
  rows, labels and graph points out), graph and map geometry, the QR
  matrix, number formatting, the card registry and the theme token table.
  It depends on `mirqab-proto` and nothing graphical, and lint fails if a
  file in it names `gtk`, `gdk`, `gsk`, `adw`, `glib` or `cairo`.
  `crates/mirqab-ui` holds the widgets: `src/tabs/<tab>/page.rs` and one
  file per card or custom widget. A widget file contains layout, signal
  wiring and drawing calls; a branch on data belongs in the view model.
- **Widgets are built in Rust.** No `.ui` files, no Blueprint. One widget
  type per file. Custom widgets subclass `gtk::Widget`.
- **All data comes from the daemon.** The interface runs no command and
  reads no file under `/proc` or `/sys`. One exception: the Volume page
  talks to the sound server itself through the PulseAudio client API,
  because sliders need an event-driven client in the process that draws
  them and the daemon would only proxy the same library (plan 014).
- **A tab's page is built when it is first shown and dropped when the
  window hides.** The page stack holds a constructor per tab, not a
  widget. A page subscribes when it is mapped and unsubscribes when it is
  unmapped; nothing subscribes at start-up except `ui`, `self` and the
  streams of pinned Home cards.
- **Icons** are glyphs from `ttf-nerd-fonts-symbols` (in `extra`,
  installed **[machine]**), shown in a `GtkLabel` with the `icon` style
  class and named in `crates/mirqab-view/src/icons.rs` only. Rejected: a
  vendored symbolic SVG set (hundreds of files to keep in step), and the
  Adwaita icon theme alone (it has no icon for fans, ports or agents).
- **Strings live in one place.** The first release is English only
  (owner, 2026-11-13). Every string a person reads, in the interface, the
  CLI and the daemon's stream data, is a function or constant in
  `crates/mirqab-proto/src/strings.rs`, so a translation later replaces
  one module's body (with gettext or a message file) and touches no call
  site. Lint greps `crates/mirqab-ui/src` for a string literal passed
  straight to `Label::new`, `set_label`, `set_title`, `set_text` or
  `set_tooltip_text`.
- **Accessibility.** Every control that shows only an icon sets an
  accessible label; custom-drawn widgets set an accessible role and a
  text description of what they show (a graph: its series and its latest
  value).
- **Timers.** The interface does not sample; data arrives from the daemon.
  Every `glib::timeout_add*` call carries a marker comment on the line
  above: `// mirqab:ui-timeout` for one-shots (the 60 s exit, a debounce),
  `// mirqab:sample` for anything that repeats. A `mirqab:sample` source
  must have an interval of at least 1000 ms and must be removed when its
  widget is unmapped. `add_tick_callback` (a callback every frame) is
  allowed only for a transition that ends, with the `ui-timeout` marker.
  Lint fails on either call with no marker.

### 7.3 Theming

- **libadwaita's style, recoloured, not replaced.** Widgets use
  libadwaita's style classes (`card`, `title-1` to `title-4`, `heading`,
  `caption`, `dim-label`, `numeric`, `accent`, `success`, `warning`,
  `error`, `boxed-list`, `navigation-sidebar`, `pill`, `flat`) and its
  named colours through CSS variables (`--window-bg-color`,
  `--card-bg-color`, `--accent-bg-color`, `--accent-color`,
  `--success-color`, `--warning-color`, `--error-color`,
  `--dim-opacity`; libadwaita 1.6 and later,
  https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html).
- **Colours are variables, never hex in widgets.** There is one
  stylesheet, `crates/mirqab-ui/resources/style.css`, compiled into the
  binary and loaded with a `GtkCssProvider`. Rules in it refer to
  `var(--…)` only. The app's own tokens (`--mirqab-series-1` to `-6`,
  `--mirqab-grid`, `--mirqab-meter-track`, `--mirqab-map-land`,
  `--mirqab-map-night`) are declared once, at the top of that file.
- **The token table.** `crates/mirqab-view/src/theme.rs` lists every
  token with its default. It is the only Rust file with a colour literal.
  From it the interface generates a second, small stylesheet that sets the
  variables, and custom-drawn widgets, which cannot use CSS for a path's
  colour, read the same table. Lint greps `crates/mirqab-ui` for
  `#[0-9a-fA-F]{3,8}\b`, `rgb(`, `rgba(` and `RGBA::new` and fails on any;
  it allows hex in `style.css` only inside the token block.
- **Theme source.** Built-in dark tokens with an orange accent
  (`AdwStyleManager` set to prefer dark; `theme.scheme` in `config.json`
  can say `light` or `system`); overridden key by key from
  `~/.config/mirqab/theme.json`; optionally following a desktop's token
  file named in `config.json` (`theme.follow`, a path; a haseen user
  points it at haseen's token file, whose keys `mirqab-view` maps onto
  the variables above). The file is watched with a `GFileMonitor`, not
  polled, and a change regenerates the small stylesheet.
- **Sizes** follow the font: spacing and widget sizes in the stylesheet
  are in `em`, and custom widgets measure in multiples of the font size
  they get from Pango. No pixel size literal outside `style.css`.

Rejected: a stylesheet that restyles every widget from scratch (it breaks
with each libadwaita release); `@define-color` (deprecated in favour of
CSS variables since GTK 4.16); following the GTK theme of the desktop
(libadwaita does not support third-party themes); reading colours back
from CSS in drawing code (`gtk_style_context_lookup_color` is deprecated
since GTK 4.10).

### 7.4 Drawing

Graphs, meters, dials, the QR code and the dotted world map are custom
widgets that implement `snapshot()`:

- **`GtkSnapshot` with `GskPath`** (GTK 4.14 and later): a line graph is
  one stroked path and one filled path (`append_stroke`, `append_fill`);
  a QR code and the map's dots are each **one** filled path holding all
  their squares or circles, so the node count does not grow with the dot
  count; solid bars are `append_color`.
- **Cairo only where a path node cannot do it** (`append_cairo`), and the
  plan that needs it says why. `GtkDrawingArea` is not used: it is Cairo
  for the whole widget and is redrawn in software.
- **No blur.** No `GskBlurNode` (`push_blur`), no CSS `filter: blur()`,
  no `backdrop-filter`. Lint greps the crate and the stylesheet for all
  three. Also no `GskShaderNode` and no `GtkGLArea`.
- A widget is redrawn only when its data changed (`queue_draw` from the
  stream handler), never on a frame clock.
- The geometry (points, the dot list, the QR matrix) comes from
  `mirqab-view` and is tested there; the widget only turns it into nodes.
- GTK renders at the surface's real scale, fractional or not, so there is
  no separate fractional-scale test matrix. The owner checks one page by
  hand on a monitor at scale 1.25 in plan 001.

### 7.5 How the interface is tested

Three layers, cheapest first:

1. **View models, with no display at all.** `cargo test -p mirqab-view`:
   plain Rust tests in `crates/mirqab-view/tests/<tab>.rs` feed golden
   stream data and check rows, labels, graph points and geometry. Most of
   the interface's behaviour is here, and this layer needs no GTK
   installed.
2. **Widgets, on a headless display.** `crates/mirqab-ui/tests/*.rs` use
   `#[gtk::test]`, which initialises GTK once and runs every test on the
   thread that owns it. `tests/lib.sh` starts `gtk4-broadwayd` (inside the
   `gtk4` package **[machine]**) on a private display number and exports
   `GDK_BACKEND=broadway`, `BROADWAY_DISPLAY`, `GSK_RENDERER=cairo` and
   `GTK_A11Y=none`. A widget test builds the widget, hands it data, and
   inspects the widget tree or the render node tree (`gtk::Snapshot` to
   `gsk::RenderNode`): for example "the graph produced one stroke node"
   and "no blur node anywhere". No pixels are compared.
3. **Each page once, against a real daemon.** `mirqab-ui --selftest <tab>`
   builds that page in window mode against the socket of a daemon serving
   a fixture root, waits for the first event, prints the widget tree as
   JSON (type, style classes, text, accessible label) and exits.
   `tests/test-ui-<tab>.sh` asserts on that JSON with `jq`.

Not covered headless: the layer-shell popover (Broadway is not Wayland).
Plan 001 has an **[unverified]** step to run the popover tests under a
headless wlroots compositor in CI; until that works the popover is
checked by hand on Hyprland, and the tests cover window mode.

## 8. Privileges

The daemon and the interface run as the user and never ask for more. Three
things need root, and each is opt-in, off by default and behind one
PolicyKit prompt:

| Need | Why root | How |
|------|----------|-----|
| Fan control | `pwmN` and `pwmN_enable` are writable by root only | `mirqab-fand` |
| CPU package power | `/sys/class/powercap/intel-rapl:0/energy_uj` is mode 0400 root **[machine]** | `mirqab-fand` reads it |
| System ports | `/proc/<pid>/fd` of other users is unreadable; `ss -Hltnp` names only the caller's processes **[machine]** | One `pkexec ss -Hltnp` snapshot when the user presses Load |

### 8.1 `mirqab-fand`

- A system service, **disabled by default**, shipped in the package but
  never enabled by it. `mirqab helper enable [--dry-run]` shows what it
  will do and runs one `pkexec systemctl enable --now mirqab-fand.service`
  plus the allow-list write; `mirqab helper disable` undoes both.
- Socket `/run/mirqab-fand.sock`, mode 0666, authenticated by
  `SO_PEERCRED` against `/etc/mirqab/fand.allow` (uids, written by
  `helper enable`). Rejected: a group-owned socket (logging out and in to
  pick up a group).
- It accepts five requests: `list`, `read-energy`, `set-pwm`, `set-auto`,
  `ping`. `set-pwm` writes only to `pwm[0-9]+` and `pwm[0-9]+_enable`
  files directly under a `hwmonN` directory it discovered itself under
  `/sys/class/hwmon`, after `realpath` confirms the target stays under
  `/sys/devices`. The client names a hwmon by index and a channel by
  number; it never sends a path.
- **Deadman.** If no `ping` arrives for 30 s, or the client disconnects, or
  the helper gets SIGTERM, every channel it touched goes back to automatic
  (`pwmN_enable` restored to the value read before the first write).
- A floor: it refuses a duty below the per-channel minimum from
  `/etc/mirqab/fand.conf` (default 20 %), and refuses everything while any
  temperature it can read is above 90 °C except `set-auto`.
- Its own crate, depending on `libc` and `serde_json` only. No D-Bus, no
  SQLite, no network.

Rejected: a udev rule that makes `pwm` files group-writable (no failsafe if
the user process dies with the fan at 0), a setuid binary, `pkexec` per
write (a prompt per slider move).

### 8.2 What is never done

No capability is added to the main binary. No setuid. Nothing is written
under `/etc` or `/usr` by the daemon. Root-owned processes are never
signalled; the interface shows them read-only.

## 9. Self-cost

### 9.1 Budgets

| Measure | Budget | How it is measured |
|---------|--------|--------------------|
| Daemon CPU, no subscribers | **< 0.3 % of one core** over 60 s | `tools/bench-idle.sh`: utime+stime delta from `/proc/<pid>/stat` |
| Daemon RSS, no subscribers | < 15 MiB | `VmRSS` after 60 s |
| Daemon CPU, System tab open with its live graph at 1 s | < 1 % of one core | Same script with `--subscribe system --live` |
| Interface process, window closed for 60 s | does not exist | `pgrep -x mirqab-ui` |
| Interface RSS, open | reported, not gated; plan 001 measures it with the default renderer and with `GSK_RENDERER=cairo` and records both | footer |
| `history.db` at 30 days, with its log | < 40 MiB | synthetic fill test |
| Wakeups, no subscribers | ≤ 2 per minute (minute writer, D-Bus keepalive) | `/proc/<pid>/status` context switches over 60 s |

**The idle target is unchanged by the 1 s decision**: under 0.3 % of one
core with the window closed. With the window closed no graph is on
screen, so nothing is `live`, and the only periodic work is the minute
writer. Stat Space's figures are < 0.3 % CPU and < 25 MB at 1 s sampling
with the popover closed; ours match on CPU and beat it on memory while
closed.

How plan 001 measures it, exactly: `tools/bench-idle.sh` starts
`mirqab daemon --root tests/fixtures/root-desktop` on a private socket
with a temporary `HOME`, waits 5 s, reads fields 14 and 15 (utime, stime)
of `/proc/<pid>/stat`, sleeps 60 s, reads them again, and divides the
difference by `60 × CLK_TCK` (from `getconf CLK_TCK`). It prints the
percentage, `VmRSS`, and the voluntary plus involuntary context switches
over the same minute, and exits non-zero when CPU is 0.3 % or more or RSS
is 15 MiB or more. It refuses to run while a `mirqab-ui` process using
that socket exists, so "window closed" is part of the measurement. With
`--subscribe <stream> [--live]` it holds one subscription for the minute
and applies that row's budget instead.

The script runs in CI against a fixture root on every pull request that
touches `crates/`, and fails the build over budget. A shared CI runner is
noisy, so CI takes the best of three runs; the number quoted in docs
comes from a run on the development machine, with the date and machine
class, never from a guess.

### 9.2 Resource rules

1. No polling under 1 s, anywhere. 1 s is for a stream whose live graph
   is on screen, and for nothing else (§5.3). Everything not on screen,
   and everything on screen that is not a live graph, runs at 2 s or
   slower.
2. Sample only what is visible, pinned, shown in a bar or needed by an
   enabled rule. Closing the window stops every sampler that only it used.
3. Prefer an event to a poll (D-Bus signal, netlink, inotify, pidfd).
4. No blur, no shaders, no frame-clock redraw loops (§7.4).
5. Pages load lazily and are destroyed on hide.
6. Expensive scans (disk caches, LAN devices, speed test, SMART refresh)
   run only when the user presses a button, show progress and can be
   cancelled.
7. Measure before claiming.

## 10. Privacy and the network

The repository is public. The app makes no request the user did not turn
on or start, except Weather when its tab, card or bar item is in use.

| Request | When | What leaves the machine |
|---------|------|-------------------------|
| `api.open-meteo.com/v1/forecast` | Weather in use, at most every 30 min per city | Coordinates rounded to 0.05° (about 5.5 km) |
| `geocoding-api.open-meteo.com/v1/search` | The user searches for a city | The typed name |
| Speed test endpoint | The user set one and presses Run | Random bytes |
| Public IP endpoint | The user set one and presses Show | Nothing but the request |
| Agent usage endpoints | Opt-in, Agents tab visible, at most every 5 min | The agent's own token to its own vendor |
| ICMP echo to a configured host | Opt-in latency, at most every 10 s | An echo |

Decisions of 2026-11-13 (owner):

- **No speed-test and no public-IP endpoint ships.** `net.speedtest.url`
  and `net.publicIp.url` are empty settings. While one is empty its
  button is disabled with the text "Set an endpoint in Settings"; the
  app never suggests a host.
- **Agent usage limits are built, opt-in and read-only** (plan 008). They
  use the token the agent itself stored. The token is read into memory
  for one request, handed to `curl` on stdin, and is never copied to
  another file, never refreshed and never logged.
- **Open-Meteo is the default weather provider**, and its base URL is a
  setting (`weather.endpoint`) so a self-hosted instance or another
  compatible server works. Open-Meteo's free API is for non-commercial
  use (https://open-meteo.com/en/terms); `README.md` states this, and a
  commercial user is told to set their own endpoint.

No telemetry, no update check, no crash upload. Process environments are read only to look for marker variable
names and are never shown, stored or sent. Serial numbers are off by
default in the health report.

Never commit: a real hostname, address, interface name, MAC, SSID, serial
number, coordinates more precise than a city, a token, or anything copied
from a live `~/.config`. Fixtures are synthetic or scrubbed, and
`tools/secrets.sh` checks patterns for all of these.

## 11. CLI and MCP

`mirqab` with no daemon running exits with code 2 and one line saying how
to start it (`systemctl --user start mirqab.socket`). Every listing command
takes `--json`. Every mutating command takes `--dry-run` and asks before
acting unless `-y`.

```
mirqab open [tab] [--window] | toggle [tab] | close
mirqab status | load | sensors | battery
mirqab apps [--sort cpu|mem] | what <pid>
mirqab ports [--system] | port <n> | kill <n> [-y] [--force]
mirqab agents | cleanup <session> | orphans [--kill]
mirqab awake <duration>|off [--display] | blockers
mirqab disk | net | ping <host> | speedtest | devices
mirqab volume [N%|+N|-N|mute|unmute] [--app NAME]
mirqab fans [set <id> <percent>|auto]
mirqab time [city] | weather [city]
mirqab why | spikes | history export | report
mirqab bar <item> [--watch]
mirqab helper enable|disable|status
mirqab --mcp | mcp-config <client>
mirqab daemon [--root DIR] [--socket PATH]
```

The MCP server speaks JSON-RPC over stdio and is a thin client of the same
socket. Tools: `list_ports`, `whats_on_port`, `kill_port`, `top_apps`,
`system_load`, `agent_sessions`, `orphans`, `cleanup_agent_leftovers`,
`keep_awake`, `sleep_blockers`, `explain_slowness`, `what_is_process`. A
destructive tool returns what it would stop and acts only when called again
with `confirm: true`; the refusals in §5.4 apply. Plan 005.

## 12. Repository layout

```
crates/mirqab-proto/    library: frames, stream data shapes, error codes, strings (serde only)
crates/mirqab-core/     library: root, exec, act, server, samplers, history, rules, dbus
crates/mirqab/          binary `mirqab`: daemon, cli, mcp, bar, tray, elevate. No GTK
crates/mirqab-fand/     binary `mirqab-fand`: the root helper (libc, serde_json)
crates/mirqab-view/     library: view models per tab, geometry, cards, icons, theme tokens. No GTK
crates/mirqab-ui/       binary `mirqab-ui`: GTK4/libadwaita
  src/{main,app,window,client,style}.rs
  src/widgets/          card, graph, meter, range_picker, sidebar, footer, …
  src/tabs/<tab>/       page.rs and one file per card
  resources/style.css   the one stylesheet
share/mirqab/           config.schema.json, cities, land mask
dist/                   systemd units, .desktop, PKGBUILD, deps.toml
contrib/haseen/         the haseen bar plugin, QML (installed by the owner)
contrib/waybar/         example module config
tests/                  run.sh, lib.sh, test-*.sh, fixtures/, golden/, stubs/
tools/                  lint.sh, affected-tests.sh, check-docs.sh, secrets.sh, bench-idle.sh
.github/workflows/      ci.yml (plan 001), release.yml (plan 020)
deny.toml               cargo-deny: licences, advisories, bans, sources
docs/                   architecture.md (this file)
plans/                  README.md and NNN-slug.md
```

`mirqab-proto` is its own crate so that the interface can share the
protocol types without compiling or linking SQLite and D-Bus: Cargo
unifies features across a workspace build, so a feature flag on
`mirqab-core` would not keep them out.

## 13. Dependencies

Rust crates, each pinned in `Cargo.lock`, each justified in the plan that
adds it. Versions are the newest on the registry on 2026-11-13
**[crates.io]** unless a reason is given.

| Crate | Version | Used by | Plan |
|-------|---------|---------|------|
| `libc` | 0.2 | all | 001 |
| `serde`, `serde_json` | 1, 1.0.154 | all | 001 |
| `rusqlite` | 0.42.0, system SQLite, no `bundled` feature | daemon | 001 |
| `zbus` | 5.19.0, blocking API (held on 5.x for `ksni`, §5.6) | daemon | the first plan that speaks D-Bus (002 or 004) |
| `ksni` | 0.3.7, `default-features = false`, features `async-io` and `blocking` (licence: Unlicense) | daemon | 004 |
| `jiff` | 0.2.43, time zones from the system `/usr/share/zoneinfo` | daemon | 016 |
| `gtk4` | 0.11.5, feature `v4_16` | interface | 001 |
| `libadwaita` | 0.9.2, feature `v1_6` | interface | 001 |
| `gtk4-layer-shell` | 0.8.1 | interface | 001 |
| `libpulse-binding`, `libpulse-glib-binding` | 2.30.1, 2.29.0 | interface, Volume page only | 014 |

Adding a crate needs a line in the plan that adds it saying what it
replaces and what it costs in binary size. `cargo deny` (0.20.2, in
Arch's `extra`) enforces the licence allow-list (MIT, Apache-2.0,
BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, Unlicense), known
advisories, one version of `zbus`, and crates.io as the only source.

No async runtime is written against. `zbus` and `ksni` run their own
small executor on a thread they own; our code calls their blocking APIs.
The interface's only loop is GLib's.

Build needs: `rust` (1.92 or later, the `gtk4` crate's minimum),
`pkgconf`, and the development files of the libraries below, which on
Arch are in the same packages.

Runtime packages (Arch names): `gtk4` (4.16 or later), `libadwaita` (1.6
or later), `gtk4-layer-shell`, `sqlite`, `curl`, `ttf-nerd-fonts-symbols`,
`polkit`, `tzdata`. Optional, each
unlocking a feature and reported by `mirqab status` when missing:
`pipewire-pulse` and `libpulse` with `wireplumber` (Volume), `bluez` (Bluetooth), `udisks2`
(SMART, eject), `upower` (battery), `networkmanager` or `iw` (Wi-Fi),
`bolt` (Thunderbolt), `docker` (container ports), `avahi` (LAN names),
`geoclue` (current location), `lm_sensors` (finding fan drivers), `fwupd`
(firmware updates in the health report), `qrencode` (QR codes). The one
list both the package and `mirqab status` read is `dist/deps.toml` (plan
020).

## 14. Packaging

Arch and Arch-based distributions first. `dist/PKGBUILD` builds `mirqab`
(and a `-git` variant) with `cargo build --release --locked`, installs the
three binaries, the user units under `/usr/lib/systemd/user`,
the system unit for the helper under `/usr/lib/systemd/system` (not
enabled), the `.desktop` file and the licence. Checked with `namcap` in CI
(`namcap` is not on the development machine **[machine]**). Publishing to
the AUR is an owner step (plan 020). Other distributions are out of scope
until the owner asks.

Release profile: `opt-level = "s"`, `lto = true`, `panic = "abort"`,
`strip = true`, as in haseen's sidecar.

## 15. Licence

MIT (owner, 2026-11-13), Copyright (c) 2026 t1nk33r. `gtk4-rs`,
`libadwaita-rs` and `gtk4-layer-shell` are MIT; GTK and libadwaita
themselves are LGPL-2.1-or-later and are linked dynamically. Reasons: haseen is MIT, so code moves
between the two freely; the parts adapted from DankMaterialShell and
Omarchy are MIT. `NOTICE.md` has one row per adapted file. GPL projects are
reference only and are never pasted. Rejected: GPL-3.0 (blocks moving code
back into haseen), Apache-2.0 (no benefit here, and a second licence text
beside haseen's).

## 16. Gates

```sh
tools/lint.sh             # cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo deny check, bash -n, shellcheck, jq, the greps in §3, §5 and §7, coverage
tests/run.sh --affected   # cargo test for the affected crates, and the shell tests whose "# mirqab:covers" globs match a changed path
tools/check-docs.sh       # plans/README.md in step with plans/, links resolve, every stream has a golden file
tools/secrets.sh          # gitleaks over history, plus the privacy patterns of §10
```

- `tools/lint.sh` in detail: `cargo fmt --all --check`; `cargo clippy
  --workspace --all-targets --locked -- -D warnings`; `cargo deny check
  licenses bans sources` (and `advisories` when `CI` is set, because that
  check downloads the advisory database); `bash -n` and `shellcheck -x
  --severity=warning` on every script; `jq empty` on every JSON file; the
  greps this document names (privilege strings, literal system paths,
  mutation calls, GTK in the headless crates, colour literals, blur,
  unmarked timers, string literals in widget calls); and the coverage
  check that every source file is matched by some test's
  `# mirqab:covers`. The one QML file set, `contrib/haseen/`, is checked
  with `qmllint` only when that tool is installed; it is not a gate.
- Tests never read the live machine. Rust tests take a `Root` fixture;
  shell tests run with a stub `PATH`, a temporary `HOME` and
  `XDG_RUNTIME_DIR`. The interface is tested as §7.5 describes: view
  models with no display, widgets and pages under Broadway.
- `tests/run.sh --affected [BASE]` selects by `# mirqab:covers` globs
  against paths changed since the merge base. A change to the test
  machinery (`tests/lib.sh`, `tests/run.sh`, `tools/affected-tests.sh`,
  `tools/coverage-exempt.txt`, `.github/workflows/`) runs every test; a
  change under `crates/mirqab-proto/`,
  `crates/mirqab-core/src/{root,exec,act,server}.rs`, `Cargo.toml`,
  `Cargo.lock`, `deny.toml` or `rust-toolchain.toml` is shared
  infrastructure and runs every test that declares
  `# mirqab:covers-shared`. The other gates under `tools/` select only
  the test that covers them (plan 001, "Phase A as built"). The full
  suite is CI's job on `main`.

### 16.1 The repository and CI

The repository is public: `github.com/t1nk333r/mirqab` (owner,
2026-11-13). The coordinator creates it; no plan does.

Plan 001 adds one workflow file, `.github/workflows/ci.yml`, with four
jobs whose names are exactly the required checks:

| Job | Runs |
|-----|------|
| `lint` | `tools/lint.sh` |
| `test` | On a pull request `tests/run.sh --affected origin/main`; on a push to `main` `tests/run.sh` (everything). Then `tools/bench-idle.sh` when `crates/` changed |
| `docs` | `tools/check-docs.sh` |
| `secrets` | `tools/secrets.sh` (full history: the checkout uses `fetch-depth: 0`) |

All four run on `pull_request` and on `push` to `main`, in an
`archlinux:latest` container as an unprivileged user, with every action
pinned by commit SHA and `permissions: contents: read`. The `test` job
installs `gtk4`, `libadwaita` and `gtk4-layer-shell` and starts Broadway
through `tests/lib.sh`. Plan 020 adds a second file,
`.github/workflows/release.yml` (job `package`, on tags and on changes
under `dist/`); it is not a required check.

`main` is protected the way haseen's is:

- the checks `lint`, `test`, `docs` and `secrets` must pass on a branch
  that is up to date with `main`;
- history is linear: squash merges only, no force pushes, no deletion;
- administrators are included;
- every change lands through a pull request
  (`gh pr create --fill && gh pr merge --squash --auto --delete-branch`).

**Protection is turned on once those checks exist**, because GitHub can
only require a check that has reported at least once. The order is fixed
in plan 001: the coordinator pushes the design as the first commit; plan
001's first pull request adds the gates and `ci.yml`; when its four jobs
have reported on that pull request the coordinator turns protection on;
that pull request is then the first to merge through it.
