# mirqab

A system monitor for Linux desktops: one window with a tab for each part
of the machine, a tray icon, bar items, a command line and an MCP server
for coding agents.

It is modelled on the behaviour of the macOS app Stat Space and shares no
code, text or artwork with it. Everything is free: there are no licences
and no paid tier.

## Status

Phases A to C of the foundation exist (plan 001): the workspace, the
development gates, the daemon with its socket protocol and the CLI, and
history. Phase D part 1 is in as well: the view crate
(`crates/mirqab-view`), with number formatting, the theme token table, the
icon glyphs, graph geometry and the sidebar model. Its tests need no
display. Nothing is on screen yet; the window comes in part 2.

- The daemon (`mirqab daemon`) serves a private Unix socket at
  `$XDG_RUNTIME_DIR/mirqab/daemon.sock`, or at `$MIRQAB_SOCKET` or
  `--socket PATH` when given. Only one daemon runs per socket. It streams
  its own CPU and memory cost on the `self` stream, and the machine's CPU
  percent and memory on the `system` stream.
- History: once a minute the daemon stores one value for each of four
  series: `cpu.total` (percent, the average over the minute), `mem.used`,
  `mem.available` and `swap.used` (bytes). It keeps 30 days in
  `~/.local/state/mirqab/history.db`, or `$XDG_STATE_HOME/mirqab/history.db`
  when that is set. The budget is 40 MiB for the file and its write-ahead
  log together, and the file is compacted once a day. Minutes while the
  daemon is not running are not stored.
- The CLI has `daemon`, `status [--json]`, `open [TAB] [--window]`,
  `toggle [TAB]`, `close`, `history SERIES... [--range 1h|24h|7d|30d]
  [--json]`, `history --list`, `help [COMMAND]`, `--help` and `--version`.
  Every command takes `--socket PATH`.
- Theme tokens can be overridden in `~/.config/mirqab/theme.json`, one
  key per CSS variable name. The view crate checks the file; no program
  applies it until the window exists.
- There is no window yet. `mirqab-ui` only prints its version, so `open`,
  `toggle` and `close` fail with `unavailable: no interface is running`
  and exit 1. They take `--dry-run`, which prints what the action would do.
- Not built yet: the System tab and the other tabs, the window, the MCP
  server and the root helper. `mirqab-fand` only prints its version.

The config is `~/.config/mirqab/config.json` (under `$XDG_CONFIG_HOME` when
set), its keys are listed in `share/mirqab/config.schema.json`, the daemon
is its only writer, and a file you broke by hand is never overwritten.

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

Run the daemon in one shell. It stays in the foreground and stops on
Ctrl-C. It needs `XDG_RUNTIME_DIR`, which a login session sets, and it
prints `ready <socket>` once it listens:

```sh
mirqab daemon
```

In another shell, `mirqab status` reads from it. With no daemon running,
`status` exits 2 and says how to start one.

To run the daemon as a user service, copy the units in `dist/` to
`~/.config/systemd/user/`. No package installs them yet. Enable both
units: the service starts the daemon at login, so history is kept even
when no client is connected.

```sh
mkdir -p ~/.config/systemd/user
cp dist/mirqab.socket dist/mirqab.service ~/.config/systemd/user/
systemctl --user enable --now mirqab.socket mirqab.service
mirqab status
```

Stop a daemon you started by hand first; only one can hold the socket.
The service runs `/usr/bin/mirqab daemon`, so install the binary there or
change `ExecStart=` in your copy. `mirqab-ui.service` is for the window and
is not needed yet.

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

Exit codes: 0 done, 1 refused or failed, 2 no daemon, 64 usage error.

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
