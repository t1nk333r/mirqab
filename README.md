# mirqab

A system monitor for Linux desktops: one window with a tab for each part
of the machine, a tray icon, bar items, a command line and an MCP server
for coding agents.

It is modelled on the behaviour of the macOS app Stat Space and shares no
code, text or artwork with it. Everything is free: there are no licences
and no paid tier.

## Status

Plan 001 (the foundation) is complete, phases A to E: the workspace, the
development gates, the daemon with its socket protocol and the CLI,
history, the view crate (`crates/mirqab-view`, with number formatting, the
theme token table, the icon glyphs, graph geometry and the sidebar model;
its tests need no display) and the window (`crates/mirqab-ui`). Plans 002
and 003 are complete too, and milestone M1 (a running app with Home and
System) is done.

- Measured cost (2026-11-13, on the development machine, release build):
  idle, with no subscriber, 0.000 % of one core over 60 s and 5.0 MiB.
  With a live 1 s subscription to `system`, 0.000 % and 5.0 MiB. The open
  window uses 50 MiB with the Cairo renderer. Budgets: under 0.3 % and
  15 MiB idle, under 1 % live. To measure it yourself, run
  `tools/bench-idle.sh`, or `tools/bench-idle.sh --subscribe system --live`
  (it needs `socat`).
- The daemon (`mirqab daemon`) serves a private Unix socket at
  `$XDG_RUNTIME_DIR/mirqab/daemon.sock`, or at `$MIRQAB_SOCKET` or
  `--socket PATH` when given. Only one daemon runs per socket. It streams
  its own CPU and memory cost on the `self` stream, and the machine's CPU,
  load, memory, pressure, temperatures, GPUs and power on the `system`
  stream.
- History: once a minute the daemon stores one value for each of 25
  series. `mirqab history --list` names them. The first four are
  `cpu.total` (percent, the average over the minute), `mem.used`,
  `mem.available` and `swap.used` (bytes). The rest cover CPU split, load,
  cache, pressure, paging, swap traffic, temperatures, GPU use and VRAM,
  power and CPU clock. It keeps 30 days in
  `~/.local/state/mirqab/history.db`, or `$XDG_STATE_HOME/mirqab/history.db`
  when that is set. The budget is 40 MiB for the file and its write-ahead
  log together, and the file is compacted once a day. Minutes while the
  daemon is not running are not stored. With `--root DIR` (fixture trees,
  used by the tests) the daemon keeps `history.db` and `config.json` beside
  its socket instead, so fixture numbers never reach your files, and only
  one daemon may use a history file (`history.db.lock`).
- The CLI has `daemon`, `status [--json]`, `open [TAB] [--window]`,
  `toggle [TAB]`, `close`, `history SERIES... [--range 1h|24h|7d|30d]
  [--json]`, `history --list`, `load [--json]` (CPU, load, memory, pressure
  and the thermal word), `sensors [--json]` (temperatures, GPUs and power),
  `bar ITEM [--watch] [--interval SEC] [--format waybar|plain|json]` and
  `bar --list` (readings for a status bar),
  `help [COMMAND]`, `--help` and `--version`.
  Every command takes `--socket PATH`. `open`, `toggle` and `close` take
  `--dry-run`, which prints what the action would do.
- Theme tokens can be overridden in `~/.config/mirqab/theme.json`, one
  key per CSS variable name. The view crate checks the file, and the
  window applies it and reloads it when the file changes.
- The window is `mirqab-ui`. `mirqab open` starts it through
  `mirqab-ui.service`. On a compositor with wlr-layer-shell, such as
  Hyprland, it is a popover anchored to a screen corner (the top right by
  default). Elsewhere, or with `--window`, it is a normal window. The
  sidebar has sixteen tabs. A tab whose plan has not landed shows a
  placeholder. The footer shows mirqab's own CPU and memory, the machine
  line, and a Settings button. Escape or `mirqab close` hides the window,
  and the process quits 60 s after it was last hidden. The `.desktop`
  entry opens a window.
- The System tab shows these cards: the overview (the thermal word,
  uptime, load and restart advice), CPU with its cores, memory, pressure,
  graphics (one section per GPU, at most two, as history keeps),
  temperatures and power. Each card with one graph, and the graphics
  card, has a range picker. `2m` is live and updates every second while
  the page is on screen (the default `live.intervalMs`). `1h`, `24h`,
  `7d` and `30d` come from history: each is read when chosen, then once a
  minute. The numbers update every 2 s. Pressure has three small live
  graphs and no picker. Every card has "Pin to Home".
- Home shows the pinned cards on two columns, compact and live. The first
  run pins the overview, CPU and memory, once; an emptied Home stays empty
  and offers a button that opens System. Reorder by dragging a card's
  handle, which shows on hover, or with Alt+Up and Alt+Down; a card's pin
  control unpins it. Home subscribes to its cards' streams only while it
  is on screen. The window opens on Home; `window.rememberTab` reopens the
  last tab instead.
- The tray icon and `mirqab bar` for status bars (plan 004).
- Not built yet: the content of the other tabs (each one is a placeholder
  until its plan lands) and the root helper.
  `mirqab-fand` only prints its version.

The config is `~/.config/mirqab/config.json` (under `$XDG_CONFIG_HOME` when
set), its keys are listed in `share/mirqab/config.schema.json`, the daemon
is its only writer, and a file you broke by hand is never overwritten.
`window.rememberTab` (default false) reopens the last tab; that tab is kept
in `window.lastTab`, which is written only while `rememberTab` is true.

The design is in `docs/architecture.md` and the plans are in `plans/`.
The sections below describe the planned product. It is developed and
tested on an Arch-based system; other distributions are untested.

## What it will do

| Tab | Shows |
|-----|-------|
| Home | The cards you pinned from any tab |
| Apps | Processes grouped by app, with an inspector |
| System | CPU, memory, GPU, sensors, power |
| Fans | Fan speeds; curves if you enable the root helper |
| Disk | Space, activity, drive health, reclaimable caches |
| Network | Throughput, Wi-Fi, ping, traceroute, DNS, and a speed test against an endpoint you set |
| Devices | Bluetooth, USB, Thunderbolt, displays, audio |
| Battery | Level, health and history, on machines that have one |
| Volume | A volume slider per app, through PipeWire |
| Ports | What is listening, and which project owns it |
| Agents | Coding-agent sessions and what they left running |
| Awake | Keep the machine awake; see what blocks sleep |
| Time | World clocks with a scrubber |
| Weather | Forecast for your cities |
| Trends | 30 days of history, spike rewind, "why is it slow?" |
| Support | A shareable health report with no identifiers in it |

## How it is built

- A Rust daemon keeps 30 days of per-minute history in SQLite and serves
  a local socket. Idle, it is budgeted at under 0.3 % of one core and
  15 MiB.
- The window is a separate GTK4/libadwaita program, also in Rust. It
  starts when opened and exits a minute after it closes.
- A graph you are looking at updates every second. Nothing else samples
  that fast, and with the window closed only the once-a-minute history
  writer runs.
- Nothing runs as root unless you run `mirqab helper enable`, which
  starts a small service that can only set fan duties.
- No telemetry and no update check. The only request made without a
  button press is the weather forecast, while Weather is in use.
- No speed-test server and no "what is my IP" service is built in. Both
  are empty settings; the buttons do nothing until you set one.
- English only for now.

## Try it

Build from source on an Arch-based system. The interface draws its icons
with Symbols Nerd Font (`ttf-nerd-fonts-symbols`):

```sh
sudo pacman -S --needed base-devel rust pkgconf gtk4 libadwaita gtk4-layer-shell sqlite ttf-nerd-fonts-symbols
git clone https://github.com/t1nk333r/mirqab && cd mirqab
cargo build --release --locked
sudo install -m755 target/release/mirqab target/release/mirqab-ui /usr/bin/
```

Run the daemon in one shell. It stays in the foreground and stops on
Ctrl-C. It needs `XDG_RUNTIME_DIR`, which a login session sets, and it
prints `ready <socket>` once it listens:

```sh
mirqab daemon
```

In another shell, `mirqab status` reads from it. With no daemon running,
`status` exits 2 and says how to start one.

To run the daemon as a user service, copy the units in `dist/` to
`~/.config/systemd/user/`. No package installs them yet. Enable the socket
and the service: the service starts the daemon at login, so history is
kept even when no client is connected. Copy `mirqab-ui.service` too, but
do not enable it. `mirqab open` starts it when needed.

```sh
mkdir -p ~/.config/systemd/user
cp dist/mirqab.socket dist/mirqab.service dist/mirqab-ui.service ~/.config/systemd/user/
systemctl --user enable --now mirqab.socket mirqab.service
mirqab status
mirqab open
```

Stop a daemon you started by hand first; only one can hold the socket.
The units run `/usr/bin/mirqab daemon` and `/usr/bin/mirqab-ui`, so
install the binaries there or change `ExecStart=` in your copies.

If `mirqab open` reports that it could not start `mirqab-ui.service`, the
unit cannot see the display. Run this, then try `mirqab open` again:

```sh
systemctl --user import-environment WAYLAND_DISPLAY
```

The setting lasts until the end of the login session.

Stored history is read with `mirqab history`. This prints the last hour of
`cpu.total`, one bucket a minute:

```sh
mirqab history cpu.total
```

`--range` takes `1h` (the default), `24h`, `7d` or `30d`; longer ranges
use wider buckets. Each line is a bucket's unix time, the series, its
average and its peak, and the unit. `mirqab history --list` names the
series. The first value appears after the first whole minute the daemon
runs: memory is stored at the first minute boundary, and `cpu.total`,
which needs two readings, a minute later. Until then the command prints
nothing.

`mirqab load` prints the CPU percent, load averages, memory, pressure and
the thermal word. `mirqab sensors` prints every temperature, each GPU, and
package and battery power. Both take `--json`:

```sh
mirqab load
mirqab sensors
```

Package power needs the root helper, which is a later plan. `sensors`
says so only where the CPU's RAPL counter exists but cannot be read. A
suspended GPU is reported as suspended, and nothing is read from it, so it
is not woken.

For a status bar, `mirqab bar ITEM` prints one reading as Waybar's JSON;
`--format plain` prints the text only, and `--format json` every field.
The items are `cpu` (busy percent), `mem` (percent of memory used), `temp`
(the hottest CPU temperature, in degrees), `gpu` (the busiest awake GPU's
percent), `load` (the one-minute load average) and `combo`, several items
on one line, whose parts are the setting `bar.combo` (default `cpu`, `mem`
and `temp`). A percentage is `normal`, `high` (80 % or more) or `critical`
(95 % or more); `temp` takes the thermal word (`cool`, `warm`, `hot` or
`critical`, and `normal` when there is none); `combo` takes the worst of
its parts (`hot` counts as `high`); `load` is `normal` once it has a value. A reading
with no value yet is `—`, class `unknown`. `--watch` prints a line when the
reading changes, at most once per interval (5 s by default, with 250 ms of
jitter allowed), even when another client has the daemon sampling faster.
The first real value after `—` is printed at once. `--interval SEC` takes
2 to 3600 seconds: a smaller value is raised to 2 and a larger one lowered
to 3600. `--watch` ends when its reader closes the pipe.
`contrib/waybar/README.md` has the Waybar modules and
`contrib/haseen/mirqab.bar/` a haseen bar widget.

```sh
mirqab bar combo --watch
```

The daemon shows a tray icon (a StatusNotifierItem) on desktops with a
tray host, such as Waybar's tray or KDE. A left click shows or hides the
window, and a middle click opens Home. The menu has Open, one entry for
each tab the sidebar shows (a tab in `tabs.hidden` is left out, and the
list is read each time the menu opens), Settings, and Quit daemon, which
stops the daemon. The tooltip shows CPU, memory and temperature. It is
computed only when the host reads it, so a host that reads it on a timer
costs at most one reading every 10 s. The tray holds no subscription and
no timer. Set `tray.enabled` to false to turn it off; it is read when the
daemon starts.

The tray starts with the daemon. With the units from `dist/` installed,
the socket unit starts the daemon again on the next client after Quit
daemon, such as `mirqab open` or a bar item, and the tray with it. If the
session bus is missing when the daemon starts, it logs
`tray: not shown (...)` and runs without a tray. It does not retry, so the
tray appears at the next start. Until a package installs the icons, copy
them into your icon theme:

```sh
install -Dm644 dist/icons/mirqab.svg ~/.local/share/icons/hicolor/scalable/apps/mirqab.svg
install -Dm644 dist/icons/mirqab-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/mirqab-symbolic.svg
```

The tray icon has two tones, a light watchtower with a dark outline, so it
reads on dark and light bars alike. A bar that was already running when
you installed the icon keeps showing a placeholder until you restart the
bar: bars look icons up once per process.

To toggle the window from a key in Hyprland:

```
bind = SUPER, M, exec, mirqab toggle
bind = SUPER SHIFT, M, exec, mirqab toggle system
```

Coding agents can ask mirqab through its MCP server, `mirqab mcp`. It
speaks the Model Context Protocol on stdin and stdout. Its tools only
read: `system_load` and `explain_slowness`.

`mirqab mcp-config CLIENT` prints how to register it. CLIENT is `claude`,
`codex`, `gemini`, `opencode`, `cursor` or `json`. With `--write`, mirqab
runs the client's own command after asking. That works for `claude`,
`codex`, `gemini` and `opencode`; `cursor` and `json` are edited by hand.

```sh
mirqab mcp-config claude
```

Every command is described in `docs/cli.md` (made from `mirqab help
--markdown`). Shell completion: `mirqab completions bash|zsh|fish`
prints a script. Install it where your shell looks:

```sh
mirqab completions bash > ~/.local/share/bash-completion/completions/mirqab
mirqab completions zsh > ~/.zfunc/_mirqab    # with fpath+=(~/.zfunc) before compinit
mirqab completions fish > ~/.config/fish/completions/mirqab.fish
```

Exit codes: 0 done, 1 refused or failed, 2 no daemon, 3 not confirmed (see `mcp-config --write`), 64 usage error.

## Planned usage

```sh
systemctl --user enable --now mirqab.socket mirqab.service
mirqab open            # the window; `mirqab toggle` for a keybinding
mirqab status          # one-screen summary
mirqab ports           # what is listening
mirqab why             # why is it slow?
mirqab bar cpu --watch # JSON for Waybar and other bars
mirqab --mcp           # MCP server on stdio
```

## Development

On Arch, install the toolchain and the gate tools:

```sh
pacman -S --needed base-devel git dbus rust pkgconf gtk4 libadwaita gtk4-layer-shell sqlite shellcheck jq cargo-deny gitleaks
```

Run the first three gates before every commit, and `tools/secrets.sh`
before every push:

```sh
tools/lint.sh             # fmt, clippy, cargo deny, no-GTK link check, shellcheck, jq, grep rules, test coverage
tests/run.sh --affected   # only the test files your change affects, compared with origin/main
tools/check-docs.sh       # plan index, relative links, a golden file per stream
tools/secrets.sh          # gitleaks over history, then privacy patterns (addresses, tokens, home paths)
```

Run one test file while you work with `tests/run.sh tests/test-<name>.sh`.

A test declares what it covers in its leading comments, with
`# mirqab:covers <glob>` lines. `tests/run.sh --affected` selects tests by
those globs, and lint fails on a file that no test covers unless it is
listed in `tools/coverage-exempt.txt`. `tools/affected-tests.sh` prints why
each test was selected.

`main` is protected. Every change lands through a pull request; direct
pushes and force pushes to `main` are refused. CI runs the same gates as
four required checks, `lint`, `test`, `docs` and `secrets`. The `test` job
runs `tests/run.sh --affected` on a pull request and the full suite on
`main`.

## Read next

- [docs/architecture.md](docs/architecture.md): the stack, the daemon,
  privileges, budgets, privacy.
- [plans/README.md](plans/README.md): the plans, the milestones and the
  owner's decisions.
- [CLAUDE.md](CLAUDE.md): rules for anyone, human or agent, changing
  this repository.

## Credits

Weather data will come from [Open-Meteo](https://open-meteo.com/) (CC BY
4.0) by default. Open-Meteo's free API is for **non-commercial use**
([terms](https://open-meteo.com/en/terms)). If you use mirqab
commercially, set `weather.endpoint` to a server you are entitled to
use, such as a self-hosted Open-Meteo or a paid plan's host.

City data from GeoNames (CC BY 4.0). Land outline from Natural Earth
(public domain). Adapted code is listed in `NOTICE.md` as it arrives.

## Licence

MIT.
