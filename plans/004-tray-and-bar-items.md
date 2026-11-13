# Plan 004: tray icon and bar items

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MEDIUM. The first D-Bus code, and the first thing that can keep
  a sampler alive with the window closed, so the first real test of the
  idle budget.
- **Depends on**: 001, 002
- **Category**: integration
- **Planned at**: 2026-11-13, from the owner
- **State**: IN PROGRESS. Part 1 (`mirqab bar`, the haseen widget, the
  Waybar example) implemented 2026-11-13; part 2 (the tray) implemented
  2026-11-14. Acceptance 1 and 4 are owner steps on the desktop.
- **Owner decisions** (2026-11-13): the tray uses `ksni` with `zbus` held
  on 5.x; Unlicense goes on `cargo deny`'s allow-list for `ksni` only.

## Problem

Stat Space lives in the macOS menu bar: one status item per module, each
opening the popover on its tab. A Linux desktop has no single menu bar.
The owner's brief names the options: a tray icon (StatusNotifierItem), a
layer-shell popover, a normal window, and integration with haseen and
Waybar through the daemon. Plan 001 built the popover and the window. This
plan adds the tray icon and bar items.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Tray host present | Session bus name `org.kde.StatusNotifierWatcher`, present **[machine]**; property `IsStatusNotifierHostRegistered` | no |
| Register an item | `org.kde.StatusNotifierWatcher.RegisterStatusNotifierItem(service)`; the item serves `org.kde.StatusNotifierItem` at `/StatusNotifierItem` (https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/) | no |
| Tray menu | `com.canonical.dbusmenu` at the path in the item's `Menu` property | no |
| Bar item values | The daemon's own streams | no |
| Waybar's format | A custom module with `"return-type": "json"` reads lines of `{"text","alt","tooltip","class","percentage"}` (https://github.com/Alexays/Waybar/wiki/Module:-Custom). Waybar is not installed on the development machine **[machine]** | no |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| One status item per module, drawn as text, chart, ring, pie, tachometer | Replaced. A tray icon is an icon, not a drawing surface. Text and small charts go to the bar through `mirqab bar`, where the bar draws them |
| Twelve "looks", fonts, colours, backgrounds per item | Dropped. The bar's own theme styles its modules; a second styling system is clutter |
| Share a look as a link | Dropped with the looks |
| Combined item | Kept in spirit: `mirqab bar combo` prints several readings in one line |
| Global shortcuts | Dropped: bind `mirqab toggle <tab>` in the compositor (README has examples) |
| Desktop widgets | Dropped (plan 003) |

## Design

### Tray icon

- Served by the daemon, off the main thread, with the `ksni` crate
  (0.3.7 **[crates.io]**, `default-features = false`, features `async-io`
  and `blocking`), which implements StatusNotifierItem and the
  `com.canonical.dbusmenu` menu over `zbus`. The GTK4 interface cannot
  serve it: GTK4 has no status-icon API, and the interface is not running
  while the window is closed. `ksni` needs `zbus` 5, so the workspace
  stays on `zbus` 5.19.0 instead of 6.x (architecture §5.6). Enabled
  by `config.tray.enabled` (default true). If no watcher is on the bus the
  daemon waits for the name to appear (a `NameOwnerChanged` match, no
  polling) and registers then; it re-registers when the watcher restarts.
- Item properties: `Category=SystemServices`, `Id=mirqab`,
  `Title=mirqab`, `Status=Active`, `IconName=mirqab` (a symbolic SVG
  installed to the hicolor theme; our own drawing), `ToolTip`, `Menu`.
- `Activate` (left click) is `toggle`. `SecondaryActivate` (middle click)
  opens Home. `Scroll` does nothing.
- Menu: Open; one entry per visible tab; Pin window; Settings; Quit daemon.
- Tooltip: one line, `CPU 12% · Mem 41% · 52°`. It is computed **only when
  the host asks** (a property read), from one `get system`; the tray holds
  no subscription. Hosts that read the tooltip on a timer would defeat
  this, so the value is cached for 10 s.
- A "needs attention" state (`Status=NeedsAttention`, `AttentionIconName`)
  is set by later plans: an exposed port (007), an agent that needs the
  user (008), an alert (018).

### `mirqab bar`

```
mirqab bar <item> [--watch] [--interval SEC] [--format waybar|plain|json]
mirqab bar --list
```

Without `--watch` it prints once and exits. With `--watch` it holds one
subscription and prints a line **only when the text would change**, at most
once per interval (default 5 s, floor 2 s; a bar item is never `live`, so
the 1 s rule of plan 001 does not apply to it). Closing stdout ends it.

Items, each `text` / `tooltip` / `class`:

| Item | Text | Class | Stream | Plan |
|------|------|-------|--------|------|
| `cpu` | `12%` | `normal`, `high` (≥ 80), `critical` (≥ 95) | system | here |
| `mem` | `41%` | same | system | here |
| `temp` | `52°` | from the thermal word | system | here |
| `gpu` | `3%` | same | system | here |
| `load` | `1.2` | — | system | here |
| `combo` | `12% 41% 52°` (parts chosen in config) | worst of its parts | system | here |
| `net` | `↓2.4M ↑180K` | — | net | 011 |
| `ports` | `8` | `exposed` | ports | 007 |
| `agents` | `2` | `needs-you` | agents | 008 |
| `awake` | remaining time | `on` | awake | 009 |
| `battery` | `83%` | charge state | battery | 013 |
| `volume` | `62%` | `muted` | audio | 014 |
| `fans` | `1200` | — | fans | 015 |
| `clock` | `NY 2:15 · LON 7:15` | — | time | 016 |
| `weather` | `23° ↑30° ↓18°` | condition | weather | 017 |

`percentage` is set where it makes sense, so Waybar's `format-icons` work.
A click is the bar's business: the example config binds `on-click` to
`mirqab toggle <tab>`.

The text never contains a colour or markup; `class` carries state and the
bar's CSS colours it.

### haseen plugin

Kept in this repository (owner, 2026-11-13).
`contrib/haseen/mirqab.bar/`: a haseen bar-widget plugin (manifest and one
`Widget.qml`; QML because haseen's bar is Quickshell, and the only QML in
this repository; it shares no code with the GTK interface) that runs `mirqab bar combo --watch --format json` through
`Quickshell.Io.Process` and toggles the window on click. It is not bundled
with haseen and not enabled by installing it (haseen's rules). The owner
installs it with `haseen plugin install`. It validates with
`haseen plugin validate` against haseen's schema; that check runs only on a
machine with haseen and is not a CI gate here.

### Waybar example

`contrib/waybar/README.md` with a module snippet per item.

## Rejected

- **An always-visible strip of tiles drawn by mirqab itself.** A new
  on-screen element with no reason a bar module does not already cover.
- **The interface process serving the tray item.** GTK4 removed
  `GtkStatusIcon` and offers nothing in its place, and the interface is
  not running while closed.
- **Serving the three SNI interfaces by hand on `zbus`.** It would free
  the workspace to use `zbus` 6, but it means writing and maintaining
  the dbusmenu layout protocol that `ksni` already has. It stays the
  fallback: step 1 measures `ksni`'s cost, and if the idle bench moves
  or the stripped binary grows by more than about 500 KiB, write the
  interfaces directly and record why here.
- **`ksni` with its default `tokio` feature.** A runtime the daemon has
  no other use for; the `async-io` feature uses the executor `zbus`
  already brings.
- **`libappindicator`/`libayatana-appindicator`.** GTK3 in the daemon for
  one icon.
- **A push to bars over the bar's own IPC.** Every bar differs; a line on
  stdout works with all of them.

## Tests and fixtures

- `tests/stubs/dbus/`: tests start a private session bus with
  `dbus-daemon --session --print-address --nofork` in the sandbox, and a
  small watcher stub (a shell loop around `busctl` or a 40-line Rust test
  helper) that owns `org.kde.StatusNotifierWatcher` and records
  registrations. No test touches the real session bus.

| Test | Checks |
|------|--------|
| `crates/mirqab/tests/tray.rs` | Registers once the watcher appears; re-registers after the watcher restarts; `Activate` produces a `ui` toggle event; the tooltip read causes one `get`, and no subscription exists afterwards; `tray.enabled=false` registers nothing |
| `tests/test-bar.sh` | Each item's one-shot output in the three formats against `root-desktop`; `--watch` prints nothing while the text is unchanged between the base and `.t1` fixtures swapped under it; interval below 2 s is raised; a closed pipe ends the process; `--list` matches the table |
| `tests/test-contrib.sh` | `contrib/haseen/mirqab.bar/manifest.json` parses, `Widget.qml` passes `qmllint` when that tool is installed (skipped with a note otherwise; not installed in CI), the Waybar snippets are valid JSON |
| `tools/bench-idle.sh --tray` | With the tray registered and no window, the daemon stays in the idle budget |

## Acceptance

1. The icon appears in the development machine's tray; a click toggles the
   popover; the menu opens each tab.
2. `mirqab bar cpu --watch` in a terminal prints a line when the rounded
   percent changes and is silent otherwise.
3. With the tray on, the window closed and no bar item running,
   `mirqab status` shows no subscribers and the idle benchmark passes.
4. The haseen widget, installed by the owner, shows the combo and toggles
   the window (an owner step; recorded as such).

## Checklist

1. Check the Unverified items. Add `ksni` 0.3.7 and `zbus` 5.19.0
   (architecture §13); run `cargo tree -p mirqab` and record which
   executor crates arrive, the stripped binary size before and after, and
   `tools/bench-idle.sh --tray` against the 0.3 % budget. Confirm
   `cargo deny check` accepts `ksni`'s licence (Unlicense).
2. `crates/mirqab/src/tray/{mod,item,menu}.rs`; `config.tray.*`.
3. `dist/icons/mirqab-symbolic.svg` (original artwork: a simple
   watch-tower or scope glyph; no trademarked shapes).
4. `crates/mirqab/src/bar/{mod,items,format}.rs`: the item table as data,
   one function per item from a stream snapshot to text, tooltip, class.
5. `contrib/haseen/mirqab.bar/{manifest.json,Widget.qml,README.md}`,
   `contrib/waybar/README.md`.
6. Tests above; the `--tray` mode of `tools/bench-idle.sh`.
7. README: tray, bar and keybinding examples for Hyprland.

## As built

Part 1 (2026-11-13), where it differs from the plan:

- **Two pull requests.** Part 1 is `mirqab bar`, the haseen widget and the
  Waybar example (checklist steps 4, 5, the bar half of 6 and 7); part 2
  is the tray (steps 1 to 3 and the rest of 6).
- The items are `crates/mirqab/src/bar/items.rs`, one function per item
  from a `system` snapshot; `strings::BAR_ITEMS` is the table `--list`
  prints. `temp` is the hottest CPU sensor, classed by the thermal word
  (`cool`, `warm`, `hot`, `critical`); `gpu` the busiest awake GPU; a
  reading with no value yet is `—`, class `unknown`. `combo` takes the
  worst of its parts as `normal`, `high` or `critical` (`hot` counts as
  high).
- `bar.combo` is a config key (default `cpu`, `mem`, `temp`), validated
  against the items; the CLI reads it with `config.get`.
- `--watch` checks for a closed reader on every event (`poll` on stdout,
  in `mirqab-core::sys`, the one module that may use `unsafe`), so a
  reading that never changes cannot keep a dead bar's process alive.
- Temperatures print in Celsius; `units.temperature` reaches the CLI with
  plan 005.
- The haseen widget validates with `haseen plugin validate` against
  haseen's schema on the development machine; its one unmet requirement
  there was `mirqab` not on `PATH` (not installed). `Widget.qml` passes
  Qt 6's `qmllint` for syntax; haseen's own modules are not importable
  outside haseen.

Part 2 (2026-11-14), where it differs from the plan:

- **Step 1, measured** (release profile of the workspace, stripped):
  `mirqab` grew from 961 552 to 2 399 264 bytes. In a scratch crate with
  the same profile, an empty `main` is 281 296 bytes, a hand-written
  StatusNotifierItem on `zbus` 5.19.0 with one interface 1 367 032, and a
  `ksni` 0.3.7 tray 1 804 056: `zbus` itself is about 1.09 MB, which any
  tray carries, and `ksni` adds about 437 KB over one bare interface, less
  over a real hand-written item with the `dbusmenu` layout. Under the
  500 KiB line, so `ksni` stays. `cargo tree` shows `async-io`,
  `async-executor`, `blocking`, `polling` and `async-process` (zbus's), no
  tokio. `tools/bench-idle.sh --tray --seconds 60`: 0.000 % of one core,
  6.9 MiB, 121 context switches, against 0.000 %, 6.8 MiB and 121 without
  the tray: ksni's thread sleeps.
- **The `zbus` 5 advisories.** `cargo deny` reports `zvariant`,
  `zbus_names` and `zvariant_derive` unmaintained (RUSTSEC-2026-0370 to
  0372: merged into `zbus` 6). `ksni` 0.3.7 is the latest and needs `zbus`
  5; the owner chose to keep it and ignore exactly those three
  (2026-11-14, `deny.toml`), until `ksni` moves to `zbus` 6.
  `Unlicense` is allowed for `ksni` only.
- The tray is a client of the daemon's own socket: Activate sends
  `ui.toggle`, the middle click `ui.show home`, the menu `ui.show <tab>`,
  Quit `shutdown`, and the tooltip one `get system`, cached 10 s. No core
  API was added.
- It starts on a thread of its own: ksni builds the menu while it
  registers, the menu asks the daemon for its tabs, and the daemon answers
  only once it serves. The menu reads the tabs (the sidebar's, with
  `tabs.hidden`) each time the host builds it.
- `tray.enabled` is read when the daemon starts. "Pin window" is not in
  the menu: the protocol has no action for it.
- `IconName` is `mirqab-symbolic` (`dist/icons/mirqab-symbolic.svg`, an
  original watchtower drawing); until a package installs it, copy it to
  `~/.local/share/icons/hicolor/symbolic/apps/`.
- Tests: `crates/mirqab/tests/tray.rs` on a private `dbus-daemon` with a
  stand-in watcher (`tests/support/watcher.rs`, also
  `examples/sni_watcher.rs` for the bench). Fixture daemons in the other
  tests get a dead bus address, so no test shows a tray on the desktop.
  Not automated: the menu's layout.

## Unverified

- **`ksni` with `async-io` and `blocking`**: that it adds no executor
  beyond what `zbus` 5 already links, that its service thread sleeps
  with no wakeups while nothing changes, and how it behaves when no
  watcher is on the bus and when the watcher restarts (read
  https://docs.rs/ksni/0.3.7 and test with haseen's bar and with none).
- **The watcher's behaviour in Quickshell-based hosts** (haseen's bar):
  whether it reads `ToolTip` on a timer. Measure with `busctl monitor` for
  a minute and adjust the cache time.
- **haseen's bar-widget manifest fields**: read
  `share/haseen/shell/plugin.schema.json` in the haseen repository at the
  time of writing; it is that project's contract and may have moved.
- **Waybar's JSON fields**: confirm against the wiki page above; Waybar is
  not installed here.
