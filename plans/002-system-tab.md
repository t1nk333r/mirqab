# Plan 002: System tab

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: LOW. Read-only files under `/proc` and `/sys`. The risk is
  hardware variety: sensors differ per machine, and every reading must be
  optional.
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: IN PROGRESS. Part 1 (the sampler, history series, `load`
  and `sensors`) implemented 2026-11-13. Part 2 (the page) implemented
  2026-11-13. Measured on the development
  machine (release build, real `/`): 0.116 % of one core subscribed at
  2 s, 0.233 % live at 1 s, 5.4–5.6 MiB RSS.

## Problem

The first tab with content, and the one that fills most of the history.
Stat Space's System tab shows total and per-core CPU with clocks, GPU use,
power in watts, memory with pressure, swap and paging, temperatures, a
thermal word (Cool, Warm, Hot, throttling) and a "restart recommended"
notice (https://statspace.app/features/system-monitor, read 2026-11-13).

## Linux data sources

No root for any, except package power.

| Data | Source | Root? |
|------|--------|-------|
| CPU total and per core | `/proc/stat` lines `cpu`, `cpuN`: user, nice, system, idle, iowait, irq, softirq, steal | no |
| Load averages, runnable tasks | `/proc/loadavg` | no |
| CPU model, core count | `/proc/cpuinfo`; topology in `/sys/devices/system/cpu/cpuN/topology/{core_id,physical_package_id}` | no |
| Core kinds (performance, efficiency) | `/sys/devices/system/cpu/cpuN/cpu_capacity` (ARM), `/sys/devices/cpu_core/cpus` and `/sys/devices/cpu_atom/cpus` (Intel hybrid). Absent on the development machine (one kind of core) **[unverified]** on hybrid hardware | no |
| Clock per core | `/sys/devices/system/cpu/cpuN/cpufreq/scaling_cur_freq` (kHz), `cpuinfo_max_freq`, `cpuinfo_min_freq` **[machine]** | no |
| Governor, energy preference | `…/cpufreq/scaling_governor`, `energy_performance_preference` **[machine]** | no |
| Memory | `/proc/meminfo`: `MemTotal`, `MemAvailable`, `MemFree`, `Buffers`, `Cached`, `Shmem`, `SReclaimable`, `SwapTotal`, `SwapFree`, `Zswap`, `Zswapped`, `Dirty` | no |
| Compressed swap | `/sys/block/zramN/mm_stat` (orig, compressed, used); zram swap is in use **[machine]** | no |
| Pressure | `/proc/pressure/{cpu,memory,io,irq}`: `some`/`full` with `avg10`, `avg60`, `avg300`, `total` **[machine]** | no |
| Paging | `/proc/vmstat`: `pgpgin`, `pgpgout`, `pswpin`, `pswpout`, `pgmajfault`, `oom_kill` | no |
| Temperatures | `/sys/class/hwmon/hwmonN/{name,tempM_input,tempM_label,tempM_crit,tempM_max}`. Seen: `k10temp`, `nvme`, `amdgpu`, `spd5118`, network PHYs **[machine]**. Intel: `coretemp`. `/sys/class/thermal` has only cooling devices here **[machine]**; elsewhere `thermal_zoneN/{type,temp}` | no |
| Throttling | Intel: `/sys/devices/system/cpu/cpuN/thermal_throttle/core_throttle_count`. AMD: none exposed; fall back to "clock held below 60 % of max while above the critical-minus-5 temperature" | no |
| GPU (AMD) | `/sys/class/drm/cardN/device/{vendor,gpu_busy_percent,mem_info_vram_used,mem_info_vram_total,mem_info_gtt_used}`; hwmon `power1_average` or `power1_input`, `temp1_input`, `freq1_input` **[machine]** | no |
| GPU (Intel) | `/sys/class/drm/cardN/gt_cur_freq_mhz`, `gt_max_freq_mhz`; busy percent is not in sysfs | no |
| GPU (NVIDIA) | `nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw --format=csv,noheader,nounits`, only if the binary exists; not installed here **[machine]** | no |
| Package power | `/sys/class/powercap/intel-rapl:0/energy_uj` (also present on AMD); mode 0400 root **[machine]** | **yes**: read through `mirqab-fand` when enabled (plan 015); otherwise absent |
| Battery power | `/sys/class/power_supply/BAT*/power_now` or `current_now × voltage_now` | no |
| Uptime, boot time | `/proc/uptime`, `/proc/stat` `btime` | no |

Hardware evidence comes from one AMD desktop. Plan step 1 collects scrubbed
fixtures for an Intel hybrid laptop and an NVIDIA machine from public
kernel documentation and synthetic values, and marks what no real machine
has confirmed.

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Neural Engine power | Dropped: no such unit |
| Voltages for the whole board | Shown only where hwmon has `inN_input`; no promise |
| "App, wired, compressed" memory split | Replaced by Linux's own split: used (total − available), cache (reclaimable), swap, zram compressed |
| Per-process energy | Dropped (architecture §1.2) |
| Intel GPU busy percent | Dropped: needs `perf_event_open` with privileges. Clock only |
| Package watts without the helper | Absent, with a line saying the helper enables it |

## Design

### Stream `system`

Sampled every 1 s while one of its live graphs is on screen (the `live` flag of plan 001; owner, 2026-11-13), every 2 s for any other subscriber (a pinned card with no graph, a bar item at its own interval, the CLI). Any object may be `null` or absent.

```json
{"cpu":{"percent":12.5,"user":8.1,"system":3.9,"iowait":0.5,
        "cores":[{"id":0,"percent":20.0,"mhz":4200,"kind":"p"}],
        "mhzAvg":3900,"mhzMax":5050,"governor":"powersave","epp":"balance_performance",
        "model":"…","physical":8,"logical":16},
 "load":{"one":1.2,"five":0.9,"fifteen":0.7,"runnable":2,"tasks":1400},
 "mem":{"totalBytes":0,"availableBytes":0,"usedBytes":0,"cacheBytes":0,"shmemBytes":0,
        "swapTotalBytes":0,"swapUsedBytes":0,
        "zram":{"origBytes":0,"comprBytes":0},
        "pageInPerSec":0,"pageOutPerSec":0,"swapInPerSec":0,"swapOutPerSec":0,"oomKills":0},
 "pressure":{"cpu":{"some10":0.0,"some60":0.0},
             "memory":{"some10":0.0,"some60":0.0,"full10":0.0,"full60":0.0},
             "io":{"some10":0.0,"some60":0.0,"full10":0.0,"full60":0.0}},
 "temps":[{"id":"k10temp/temp1","label":"CPU","celsius":52.0,"critical":95.0,"kind":"cpu"}],
 "thermal":{"state":"cool","hottest":"k10temp/temp1","throttling":false},
 "gpus":[{"id":"card1","name":"…","vendor":"amd","busyPercent":3,"vramUsedBytes":0,"vramTotalBytes":0,
          "celsius":45.0,"watts":18.0,"mhz":500}],
 "power":{"packageWatts":null,"batteryWatts":null,"source":"none"},
 "uptimeSec":0,"bootTime":0,
 "advice":[{"code":"restart-recommended","reason":"uptime-30d"}]}
```

Rules:

- **Temperature labels.** `kind` is `cpu` for `k10temp`, `coretemp`,
  `zenpower`, `cpu_thermal`; `gpu` for a hwmon under a DRM device; `disk`
  for `nvme`, `drivetemp`; `memory` for `spd5118`; `other` otherwise. The
  label is `tempM_label` when present, else the kind and index.
- **Thermal word.** From the hottest `cpu` or `gpu` reading against its own
  `critical` (default 95 °C when absent): `cool` below 60 % of critical,
  `warm` below 80 %, `hot` below 95 %, `critical` above. `throttling` as in
  the table above. The thresholds are constants in one file with a comment
  on why.
- **Advice.** `restart-recommended` when uptime is over 30 days, or over 7
  days with `swap.used` above half of swap or memory `some60` above 10 for
  the last hour (history). Shown as a quiet line, never a notification.
- **CPU percent** is `1 − (idle + iowait) delta / total delta`; `iowait`
  is reported separately so a stuck disk is visible.

### Series added to history

`cpu.user`, `cpu.system`, `cpu.iowait`, `load.one`, `mem.cache`,
`psi.cpu`, `psi.memory`, `psi.io`, `page.in`, `page.out`, `swap.in`,
`swap.out`, `temp.cpu`, `temp.gpu`, `gpu.<n>.busy`, `gpu.<n>.vram` (n = 0,
1), `power.package`, `power.battery`, `cpu.mhz`. With plan 001's four this
makes 24 of the 64.

### Page

Top to bottom, as cards, each with "Pin to Home":

1. **Overview strip**: the thermal pill, uptime, load, and the advice line
   when present.
2. **CPU**: big percent, a graph with the range picker, user/system/iowait
   legend, clock (average, max), governor. A "Cores" disclosure opens a
   grid of small bars, grouped by kind when there are two kinds.
3. **Memory**: used of total as a meter split into used, cache and free;
   pressure as a second graph line; swap and zram lines; paging rates.
4. **Pressure**: three small graphs (CPU, memory, I/O) of `some60` with a
   one-line explanation of what stall percentage means.
5. **GPU** (one card per GPU): busy percent graph, VRAM meter, temperature,
   watts, clock. Hidden with no GPU data.
6. **Temperatures**: a list grouped by kind, each with a small meter against
   its critical value.
7. **Power**: package watts and battery watts with a graph; when package
   power is unavailable, one line: "Needs the root helper (Fans tab)".

The range picker on a card switches its graph between the live ring and
`history.query`. A history graph is fetched once on selection and then once
a minute while visible (`// mirqab:sample`, 60 s).

### CLI

`mirqab load [--json]`: CPU percent, load, memory, pressure, thermal word.
`mirqab sensors [--json]`: temperatures, fans when plan 015 lands, watts.

## Rejected

- **`libsensors`.** A C dependency and a config file to interpret; reading
  hwmon files is fifty lines (haseen's sidecar does it:
  `core/internal/sysusage/system.go`, `hwmonInput`).
- **`sysinfo` or `procfs` crates.** They read the live `/` and cannot take a
  fixture root.
- **Reading NVIDIA through NVML bindings.** An unsafe FFI dependency on a
  proprietary library for one vendor; `nvidia-smi` every 2 s only while a
  GPU card is on screen is enough (it stays at 2 s when the stream runs
  at 1 s: the GPU fields of every second event repeat). Recorded cost: a process spawn per
  sample.
- **Per-core history.** 16 to 64 more series for a graph nobody reads at
  30 days.

## Tests and fixtures

Add to `tests/fixtures/root-desktop/` (and a `.t1` twin for deltas):
`proc/{loadavg,vmstat,pressure/*}`, `sys/devices/system/cpu/cpu{0..15}/cpufreq/*`,
`sys/class/hwmon/hwmon{0..5}/` (k10temp, nvme, amdgpu with fan and power,
spd5118, one with no temperature files), `sys/class/drm/card1/device/*`,
`sys/block/zram0/mm_stat`, `sys/class/powercap/intel-rapl:0/energy_uj`
readable (the helper path is tested in plan 015; here a readable file
proves the parser).

New trees: `root-intel-hybrid/` (coretemp, `cpu_core`/`cpu_atom`,
`thermal_throttle`, Intel `gt_*`), `root-nvidia/` (no DRM busy file; a stub
`nvidia-smi`), and `root-minimal/` stays as it is.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/system.rs` | Golden `system.json` for each fixture root; CPU and paging rates from the `.t1` delta; thermal word at each threshold; `throttling` on the Intel fixture; missing files give `null`, never an error or a zero |
| same | `advice` appears for uptime over 30 d and for 7 d with swap pressure |
| `crates/mirqab-view/tests/system.rs` | memory split sums to total, range switching, label choice |
| `tests/test-ui-system.sh` (`mirqab-ui --selftest system`, plan 001) | `mirqab-ui --selftest system` against `root-desktop` and `root-minimal`: the cards present, the GPU card absent on minimal |
| `tests/test-cli.sh` | `load --json` and `sensors --json` shapes |

## Acceptance

1. On the development machine the tab shows CPU, 16 cores, both GPUs,
   memory with zram, pressure and at least the CPU and disk temperatures,
   and the values agree with `/proc` and `sensors` read by hand.
2. Each card's range picker shows 2 m live, and 1 h after an hour of
   running.
3. With the tab closed and nothing pinned, `mirqab status` shows no
   `system` subscriber.
4. `tools/bench-idle.sh --subscribe system` is under 1 % of a core.
5. The page loads against `root-minimal` with no error line in the log.

## Checklist

1. Resolve the Unverified items; write the fixtures above.
2. `crates/mirqab-core/src/samplers/system/{mod,cpu,mem,pressure,temps,gpu,power}.rs`,
   replacing `system_core.rs` (keep its series names). Adapt parsing from
   haseen's sidecar (`core/sidecar/src/sysusage/{system,gpu,nvidia}.rs`,
   MIT, same owner; NOTICE row).
3. `minute()` for the new series; register them in `history.rs`.
4. `tests/golden/system.json` and `crates/mirqab-core/tests/system.rs`.
5. `crates/mirqab/src/cli/{load,sensors}.rs`.
6. `crates/mirqab-view/src/tabs/system.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/system/{mod,page,cpu_card,cores_grid,memory_card,pressure_card,gpu_card,temps_card,power_card,overview_strip}.rs`.
7. `tests/test-ui-system.sh`; `crates/mirqab-view/tests/system.rs`; extend
   `tests/test-cli.sh`.
8. Run `tools/bench-idle.sh --subscribe system` (2 s) and
   `tools/bench-idle.sh --subscribe system --live` (1 s, budget under 1 %
   of one core); record both numbers here.

## As built

Part 1 (2026-11-13), where it differs from the plan:

- **Two pull requests.** Part 1 is the daemon side (checklist steps 1–5
  and 8); part 2 is the page (steps 6–7).
- The stream's data structs live in `mirqab-proto::stream` (`SystemData`
  and its parts), so the daemon, the CLI and the view model share one
  shape.
- One Intel tree, not two: `root-laptop` is the Intel hybrid laptop
  (cpu_core/cpu_atom, coretemp, throttle counters, `gt_cur_freq_mhz`) and
  also carries a runtime-suspended AMD dGPU and a battery. There is no
  `root-nvidia`: the `nvidia-smi` row parser is unit-tested, and the
  NVIDIA path stays experimental until a machine confirms its fields
  (Unverified).
- A runtime-suspended card (`device/power/runtime_status` = `suspended`)
  is reported as `suspended` and nothing else is read from it: reading it
  would wake the dGPU of a hybrid laptop (haseen's sysusage plugin).
- GPU hwmon is read through the card (`device/hwmon/hwmonN`), so no
  symlink has to be followed; temperatures come from `class/hwmon`. A
  driver name seen twice gets `.1`, `.2` in the sensor id.
- Rates use three baselines: live (events), minute (history and `get`),
  and the live samples' means for the minute's gauges. A live baseline
  older than 10 s is dropped, so the first event after a quiet spell has
  no CPU value instead of the spell's average; `get` reads against the
  minute baseline without moving it (`Sampler::snapshot`).
- From the review of plan 001 phase C, done here: the first minute has a
  CPU value (the baseline is taken at probe); `swap.used` and the other
  gauges are live means when a live graph ran; shares are clamped to
  0–100; series a later plan registers are reloaded at open; prune deletes
  one series' range at a time instead of scanning the table.
- `page.in`/`page.out` are KiB per minute and per second (vmstat counts
  pgpgin in KiB); `swap.in`/`swap.out` pages. The CLI prints Celsius;
  `units.temperature` reaches the CLI with plan 005.
- The restart advice's pressure condition uses the sampler's own last 60
  minutes of memory `some60`, not a history query.

Part 2 (2026-11-13), where it differs from the plan:

- Memory "cache" is available − free, so used, cache and free add up to
  the total (on the development machine buffers + cached + reclaimable
  slab was 41 GB beside 25 GB used of 65 GB). Fixed in part 1's branch.
- A card with a graph is one `GraphCard` (`tabs/system/graph_card.rs`):
  the range picker, the graph's live subscription at `2m`, and a stored
  range fetched with `history.query` on selection and every 60 s while on
  screen (`// mirqab:sample`). The page's numbers come from one `system`
  subscription at 2 s held while the page is mapped.
- Pressure shows three small live graphs without a range picker of their
  own (one picker for three graphs would need a shared range; plan 018's
  Trends tab shows pressure history).
- `Ctx::page_ready` and `when_ready`: a page with a stream says when its
  first data is drawn, and `--selftest` waits for it as well as for the
  footer's first `self` event.
- The System tab is `built`, so it is hidden where the daemon does not
  serve `system`. The seven System cards are in `cards::CARDS` for plan
  003; each card's pin toggles its id in `home.pinned`.
- Checked by eye on the development machine (window mode, release data
  from the real `/`): 16 cores, both GPUs, zram, pressure and 20 sensors.

## Unverified

- Hybrid-core sysfs paths and the ARM `cpu_capacity` file (no such machine
  at hand): confirm against kernel documentation
  (https://docs.kernel.org/admin-guide/pm/intel_pstate.html and
  `Documentation/ABI/testing/sysfs-devices-system-cpu`) before writing the
  fixture.
- Whether `gpu_busy_percent` exists for both AMD cards under load on other
  kernels; it does on 7.2.7 **[machine]**.
- `nvidia-smi` field names: check against its `--help-query-gpu` output on
  a machine that has it, or mark the NVIDIA path experimental in the page.
