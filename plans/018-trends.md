# Plan 018: Trends, spike rewind, alerts and "why is it slow"

## Status

- **Priority**: P2
- **Effort**: L, in three phases
- **Risk**: MEDIUM. Nothing here mutates the system, but two parts run
  with the window closed (the spike recorder and alerts), so they are the
  parts that can break the idle budget. Both are opt-in or measured.
- **Depends on**: 002, 006, 010, 011
- **Category**: tab, rules
- **Planned at**: 2026-11-13, from the owner; these are Pro items in the
  reference and free here (architecture §1.2)
- **State**: PLANNED

## Problem

The history is already kept (plan 001). This plan makes it useful:

- **Trends**: every stored series over 24 hours, 7 days and 30 days, with
  summaries a person can act on.
- **Spike rewind**: when CPU, memory or temperature spiked while nobody
  was looking, what was running.
- **Alerts**: a notification when a threshold holds for a while.
- **"Why is it slow?"**: one sentence and its evidence, from rules over
  the live snapshot and the last few minutes. Also `mirqab why` and the
  MCP tool `explain_slowness`.

## Linux data sources

No root. No new source: this plan reads what earlier plans produce.

| Data | Source |
|------|--------|
| Series | `history.db` tables `sample` and `daily` through `history.query` (architecture §6) |
| Events | `event` rows: `top-apps`, `blocker`, `device`, `charger`, `clean`, `agent-start`, `agent-end`, `mcp-action` |
| Live snapshot | Streams `system`, `apps`, `disk`, `net` (one `get` each, no subscription kept) |
| Pressure | `/proc/pressure/{cpu,memory,io}` `some avg10` and `full avg10` (https://docs.kernel.org/accounting/psi.html), already sampled by plan 002 |
| Throttling | `/sys/devices/system/cpu/cpu*/thermal_throttle/core_throttle_count` (Intel) **[unverified]**; `cpu.mhz` against `cpuinfo_max_freq` |
| Out-of-memory kills | `oom_kill` counter in `/proc/vmstat` (a delta means the kernel killed something); `systemd-oomd` kills from the user journal **[unverified]** |
| Notifications | `org.freedesktop.Notifications.Notify` on the session bus (https://specifications.freedesktop.org/notification-spec/) |
| Boot and suspend marks | `/proc/stat` `btime`; logind `PrepareForSleep` |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Energy impact per app in trends | Dropped (no per-process energy) |
| Spike rewind naming other users' processes | Names only what `/proc` shows the user: all process names are visible by default, but with `hidepid` mounted they are not; the row then says "a system process" |
| Per-app network history | Only if plan 011 phase 3 is on, and with its limits |

## Design

### Phase 1: Trends (read-only page)

The page is a list of graph cards, each a series or a small group, with
one range picker for the whole page (24 h, 7 d, 30 d) and a hover
read-out: CPU, memory, swap, pressure, temperatures, GPU, power, disk
I/O, root filesystem use, network, latency, fans, battery level. A card
appears only if its series exists in the `series` table.

Above the graphs, a "This period" strip computed in the daemon
(`history.summary {range}`), each line with its evidence:

| Line | Rule |
|------|------|
| Busiest hour of the day | The hour-of-day with the highest mean `cpu.total` |
| Time under memory pressure | Minutes with `psi.memory` some-avg above 10 % |
| Hottest | The maximum of `temp.cpu`, with its time |
| Disk growth | Linear fit of `fs.root.used` over the range, as GB per week, and "full in about N weeks" only when the fit's sign is positive and R² is above 0.8 |
| Data used | Sum of `net.*.rx` and `.tx` from `daily` |
| Uptime and gaps | Minutes with no sample (suspended or off), shown as breaks in every graph, never interpolated |

Graphs draw the stored average as a line and the stored maximum as a
faint band, so a one-minute spike inside a two-hour bucket is still
visible.

Export: `mirqab history export --range 30d --format csv|json [--series
NAME]` writes to stdout. No interface names, SSIDs or process command
lines are in it by construction (plans 006 and 011).

### Phase 2: Spike rewind

**Recording.** The minute writer already runs once a minute with no
subscribers. It gains one check: if this minute's `cpu.total` average is
above 80 %, or `mem.available` is under 10 % of total, or `temp.cpu` is
above 85 °C, or `psi.memory` full-avg10 is above 5 %, or `oom_kill`
moved, it takes **one** process snapshot (the same code as plan 006's
sampler, top five by CPU and top five by memory, app-grouped names only,
no command lines) and writes a `top-apps` event with the trigger. At most
one event per trigger per 10 minutes. Thresholds are settings
(`trends.spike.*`).

Cost: one `/proc` walk in a minute that is already busy; nothing in a
calm minute but four comparisons. `tools/bench-idle.sh` must not move;
`tools/bench-idle.sh --rule spike` forces the path and reports its cost.

Recording spikes is **on by default**: it is local, bounded, and it is
the feature. `trends.spike.enabled` turns it off.

**Reading.** Spikes are markers on every Trends graph. Selecting one
opens a card: when, what tripped, the top apps then, and "what else
happened within five minutes" (device connects, charger, an agent
session starting, a blocker). A "Spikes" list shows the last 30 days.
CLI: `mirqab spikes [--json]`.

### Phase 3: Alerts and "why is it slow"

**Alerts.** Off until the user adds one. A rule is
`{series or condition, comparison, threshold, for N minutes, cooldown}`;
templates: CPU high for 5 minutes, memory available low, temperature
high, root filesystem above 90 %, an app above N % CPU for 10 minutes
(needs the `apps` sampler: this rule keeps it on at a 10 s interval,
measured and stated beside the switch), a port opened by an unexpected
app is **dropped** (too noisy).

Series rules are evaluated by the minute writer from the values it
already has: no extra wakeups. Stream `alerts` carries active and recent
alerts. A notification is sent once on entry and once on recovery,
respecting the cooldown; quiet hours are a setting. Notifications carry
no action that mutates anything: clicking opens the relevant tab.

**"Why is it slow?"** A pure function in `rules/why.rs` over one
snapshot plus the last 10 minutes of series. Each rule yields a finding
with a severity, a sentence and the numbers behind it; the answer is the
findings sorted by severity, or "Nothing stands out" with the three
numbers checked.

| Finding | Rule |
|---------|------|
| One app is using the CPU | An app above 90 % of a core for 2 minutes; names it |
| CPU saturated | `psi.cpu` some-avg10 above 25 % and load above the core count |
| Memory pressure | `psi.memory` some-avg10 above 10 %, or available under 5 %; names the largest app |
| Swapping | `swap.in` plus `swap.out` above a floor for 2 minutes (zram counts, with a milder sentence) |
| Disk is the bottleneck | `psi.io` full-avg10 above 10 %; names the busiest device and the top I/O process of the user |
| Thermal throttling | Temperature within 5 °C of the sensor's `crit`, or the throttle count moved, or frequency under 60 % of maximum under load |
| Disk nearly full | Root or home filesystem above 95 % |
| The kernel killed something | `oom_kill` moved in the last 10 minutes |
| On battery in a power-saving profile | Profile `power-saver` (plan 013) |
| Many agents running | More than N agent sessions with orphans (plan 008); links to cleanup |

The output never suggests an action it then performs. Through MCP,
`explain_slowness` returns the findings as data; an agent that wants to
act must call the destructive tools, with their confirm flow (plan 005).

The page shows a "Why is it slow?" button at the top of Trends and on
Home; the result is a card with the findings and a link per finding to
the tab that shows it.

## Rejected

- **A language model writing the explanation.** Not needed for ten
  rules, and it would send system data out.
- **Recording a process snapshot every minute.** Thirty days of process
  lists is a privacy and size problem; spikes only.
- **Anomaly detection by statistics.** Opaque; fixed thresholds can be
  explained and changed.
- **Alert actions (kill on threshold).** An unattended destructive
  action.
- **Interpolating gaps.** A laptop that slept did not have 40 % CPU.
- **Forecasting anything but disk fill.** No basis.

## Tests and fixtures

`tests/fixtures/history/`: a script builds databases at test time from a
seeded generator: `calm.db`, `spiky.db` (three planted spikes with known
top apps), `growing-disk.db`, `gappy.db` (a suspend and a reboot),
`full-30d.db` (size check). Snapshots for `why` are JSON files in
`tests/fixtures/why/<case>.json`, one per finding plus `nothing.json`
and `two-findings.json`.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/summary.rs` | Each summary line on a planted database; the disk forecast is absent with a poor fit or a negative slope; gaps are not interpolated |
| `crates/mirqab-core/tests/spike.rs` | Each trigger writes one event with the right apps; the 10-minute limit; no process walk in a calm minute (a counter on the walk function); no command line in any event |
| `crates/mirqab-core/tests/alerts.rs` | Enter, hold, recover; cooldown; quiet hours; one notification each way (a fake notification service on the private bus); a series rule adds no wakeup |
| `crates/mirqab-core/tests/why.rs` | One case per finding; ordering; "nothing stands out"; every finding carries its numbers |
| `tests/test-ui-trends.sh` (`mirqab-ui --selftest trends`, plan 001) | Cards only for existing series; spike markers; the range picker |
| `tests/test-cli.sh` | `why`, `spikes`, `history export` formats |
| `tests/mcp/explain.sh` | `explain_slowness` returns findings and performs nothing |
| `tools/bench-idle.sh` | Unchanged idle numbers with spike recording on; `--rule spike` and `--rule alert-app` report their cost |

## Acceptance

1. After a day of use, Trends shows every series with real data and
   gaps where the machine slept.
2. A deliberate CPU burn of two minutes with the window closed appears
   as a spike naming the burning process.
3. An alert fires once and recovers once.
4. `mirqab why` during the burn names the process; on a calm machine it
   says nothing stands out.
5. The idle budget holds with spike recording on.

## Checklist

Phase 1:

1. Resolve Unverified.
2. `crates/mirqab-core/src/history/{summary,export}.rs`; method
   `history.summary`.
3. `crates/mirqab-view/src/tabs/trends.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/trends/{mod,page,trend_card,summary_strip}.rs`.
4. CLI `history export`.

Phase 2:

5. `crates/mirqab-core/src/rules/spike.rs`, called from the minute
   writer; settings.
6. `crates/mirqab-ui/src/tabs/trends/{spike_card,spike_list}.rs`; markers in
   `crates/mirqab-ui/src/widgets/graph.rs` and their positions in
   `mirqab_view::graph`.
7. CLI `spikes`.

Phase 3:

8. `crates/mirqab-core/src/rules/{alerts,why}.rs`;
   `dbus/notifications.rs`; stream `alerts`.
9. `crates/mirqab-ui/src/tabs/trends/{alerts_card,alert_editor,why_card}.rs`; the
   Home button.
10. CLI `why`; MCP `explain_slowness` (the tool is declared in plan 005
    and returns "not available yet" until this lands).

## Unverified

- A throttle signal on AMD: the development machine's CPU is AMD
  **[machine]** and the `thermal_throttle` counter is Intel's. If there
  is none, use frequency and temperature only.
- Reading `systemd-oomd` kills: whether the user journal carries them,
  or only the system journal (which may need the `systemd-journal`
  group). If the latter, use the `oom_kill` counter alone.
- The thresholds in the tables are starting values. Tune them on real
  history after M4 and record what changed and why.
- `hidepid` on the target distributions (default off on Arch).
