# mirqab

A system monitor for Linux desktops: one window with a tab for each part
of the machine, a tray icon, bar items, a command line and an MCP server
for coding agents.

**Status: planned, not built.** This repository holds the design
(`docs/architecture.md`) and the plans (`plans/`). There is no code yet.
It is developed and tested on an Arch-based system; other distributions
are untested.

It is modelled on the behaviour of the macOS app Stat Space and shares no
code, text or artwork with it. Everything is free: there are no licences
and no paid tier.

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

## Planned usage

```sh
systemctl --user enable --now mirqab.socket
mirqab open            # the window; `mirqab toggle` for a keybinding
mirqab status          # one-screen summary
mirqab ports           # what is listening
mirqab why             # why is it slow?
mirqab bar cpu --watch # JSON for Waybar and other bars
mirqab --mcp           # MCP server on stdio
```

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
