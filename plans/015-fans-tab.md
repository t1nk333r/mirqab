# Plan 015: Fans tab and the root helper

## Status

- **Priority**: P2
- **Effort**: L, in two phases (read-only, then control)
- **Risk**: HIGH for control. A fan left at a low duty under load can
  overheat hardware. Control is opt-in, behind the root helper, with a
  deadman, a floor and a temperature cut-out. Reading is LOW.
- **Depends on**: 001, 002
- **Category**: tab, privileged helper
- **Planned at**: 2026-11-13, from the owner: "fans need root; do it only
  through an opt-in helper"
- **State**: PLANNED

## Problem

Stat Space's Fans tab shows each fan's speed and the temperatures, and
lets the user set a fan by hand or by a curve. On Linux, reading speeds
needs nothing; writing a duty needs root.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Fan speed | `/sys/class/hwmon/hwmon*/fan<N>_input` (RPM), `fan<N>_label`, `fan<N>_min`, `fan<N>_max` (https://docs.kernel.org/hwmon/sysfs-interface.html) | no |
| Duty | `pwm<N>` (0 to 255), `pwm<N>_enable` (0 full, 1 manual, 2 and above automatic; the meaning of values above 1 is driver-specific, same document) | read: no. Write: **yes**, the files are mode 0644 root **[machine]** |
| Temperatures | `temp<N>_input`, `temp<N>_label`, from plan 002's sensor sampler | no |
| What this machine has | The GPU's hwmon (`amdgpu`) exposes `fan1_input`, `pwm1`, `pwm1_enable`. The motherboard's hwmon (`asus`) exposes **no** fan or pwm files **[machine]**: the Super-I/O chip's driver is not loaded, so case and CPU fans are invisible here | — |
| Missing Super-I/O driver | `sensors-detect` (package `lm_sensors`) finds the module (`nct6775`, `it87`, …); loading it is the user's step and needs root. The page explains this when a machine shows fewer fans than expected | yes, not done by this app |
| Laptop fans | Vendor platform drivers: `thinkpad_acpi` (`/proc/acpi/ibm/fan` and hwmon), `asus-nb-wmi`, `dell-smm-hwmon`; all through hwmon | no to read |
| CPU package power | `/sys/class/powercap/intel-rapl:0/energy_uj`, mode 0400 root **[machine]** | **yes**: read through the helper once it is enabled |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Every fan on every machine | Only fans a loaded hwmon driver exposes. No SMC equivalent; the page lists what it found and links the explanation |
| Control without root | Not possible. Reading always works; control appears only with the helper |
| Fans managed by firmware only (many laptops) | Read-only; the control is hidden when `pwm<N>_enable` is absent or a write is refused |
| GPU fan control on NVIDIA's proprietary driver | Dropped: it goes through `nvidia-settings` and an X display, not hwmon |

## Design

### Phase 1: reading (no helper)

Stream `fans`, every 1 s while a fan graph is on screen (the `live` flag of plan 001; owner, 2026-11-13), every 2 s otherwise:

```json
{"helper":"disabled",
 "fans":[{"id":"hwmon3/1","chip":"amdgpu","label":"GPU fan","rpm":820,
          "minRpm":0,"maxRpm":3600,"dutyPercent":32,"mode":"auto","controllable":true}],
 "temps":[{"id":"hwmon2/1","label":"CPU","celsius":48.0}],
 "curves":[],"powerPackageWatts":null}
```

`id` is the chip name plus an index resolved at start, not the `hwmonN`
number, which changes between boots; the example shows the form only.
`helper` is `disabled`, `enabled`, or `unreachable`.

Series: `fan.<n>.rpm` for the first four fans. Bar item `fans`.

Page: one card per fan (a speed dial drawn as an arc path with `GtkSnapshot`, architecture §7.4; RPM, duty,
mode), the temperature list beside it, a history graph, and a banner:
"Fan control is off. Turning it on installs nothing new; it starts a
small root service that only sets fan duties." with a button that runs
the enable flow. With no fans found: an explanation and the
`sensors-detect` pointer, no empty cards.

### Phase 2: the helper `mirqab-fand`

Architecture §8.1 is the contract. This plan adds the detail.

**Enabling.** `mirqab helper enable [--dry-run]`, or the banner's button
(which shows the same text first). Dry-run prints exactly:

```
would run (as root, one prompt):
  install -d -m 0755 /etc/mirqab
  write /etc/mirqab/fand.allow: <uid>
  systemctl enable --now mirqab-fand.service
```

One `pkexec` runs a fixed script shipped in the package
(`/usr/lib/mirqab/helper-enable`), with the uid as its only argument,
validated as digits. `helper disable` runs the mirror script: sets every
channel to automatic through the socket first, then
`systemctl disable --now`, then removes the allow-list. `pkexec` appears
only in `crates/mirqab/src/elevate.rs`.

**Protocol.** JSON lines over `/run/mirqab-fand.sock`:

| Request | Reply | Notes |
|---------|-------|-------|
| `{"op":"ping"}` | `{"ok":true,"version":1}` | Resets the deadman |
| `{"op":"list"}` | chips, channels, each with current enable value and whether a write is possible | Read-only |
| `{"op":"read-energy"}` | `{"uj":…,"maxUj":…}` | RAPL package 0 only |
| `{"op":"set-pwm","chip":"amdgpu","index":0,"channel":1,"percent":45}` | `{"ok":true}` or a refusal with a reason | Saves the original `pwm_enable` on the first write |
| `{"op":"set-auto","chip":…,"channel":1}` or `{"op":"set-auto","all":true}` | `{"ok":true}` | Always allowed |

Refusals: caller's uid not in the allow-list; an unknown chip or channel;
percent below the floor (20 % by default, per channel in
`/etc/mirqab/fand.conf`) or above 100; any readable temperature above
90 °C; a request longer than 512 bytes; more than 10 writes per second.
The helper never accepts a path, never runs a command, never opens a
network socket.

**Failsafe, in order of independence:**

1. The deadman: no `ping` for 30 s, or the client's disconnect, restores
   every touched channel's original enable value.
2. SIGTERM and SIGINT handlers do the same; the unit has
   `ExecStopPost=/usr/bin/mirqab-fand --restore` for the case where the
   process was killed (it reads the saved values from
   `/run/mirqab-fand.state`).
3. The helper's own over-temperature watch: every 5 s while any channel
   is manual, if a temperature passes 90 °C it sets all to automatic and
   refuses further writes until it drops below 80 °C.
4. Suspend: on resume everything is automatic until the daemon asks
   again (the daemon listens for logind's `PrepareForSleep`).

The unit is hardened: `ProtectSystem=strict`, `ProtectHome=yes`,
`PrivateNetwork=yes`, `NoNewPrivileges=yes`,
`ReadWritePaths=/sys/class/hwmon /sys/devices /run`, `CapabilityBoundingSet=`
empty, `SystemCallFilter=@system-service`
(https://www.freedesktop.org/software/systemd/man/systemd.exec.html).

**Control in the user daemon.** Modes per fan: Auto (the default, and what
everything returns to), Manual (a fixed percent), Curve.

A curve is up to eight points of (temperature, percent) against one
chosen sensor, linear between points, flat outside them. Evaluated every
2 s by the daemon; a new duty is sent only when it differs by 2 points or
more and the temperature moved past a 3 °C hysteresis band since the last
change, so the fan does not hunt. Curves live in `config.json`
(`fans.curves`); a curve that needs the helper while it is unreachable
shows a warning and leaves the fan automatic.

Curves keep running with the window closed: a curve is "an enabled rule"
in the sense of architecture §9.2, so the `fans` sampler stays on at 2 s
while any curve is active. Curve evaluation stays at 2 s even while a
live graph has the sampler at 1 s: the 1 s decision is about drawing, not
about control. Its cost is measured (`tools/bench-idle.sh
--rule curve`) and shown in the page.

Actions: `fans.set {id, percent}`, `fans.auto {id|all}`,
`fans.curve.set {id, sensor, points}`, `fans.curve.clear {id}`. All take
`dryRun`. `fans.set` below 30 % needs `confirm`. None is exposed through
MCP.

Page additions: a mode picker per fan; a slider for Manual; a curve
editor (a custom widget: the curve as one stroked path, points dragged
with a `gtk::GestureDrag` and movable with the arrow keys, the live
temperature as a marker); "Everything to Auto" always visible while any fan is not
automatic.

CLI: `mirqab fans [--json]`, `mirqab fans set <id> <percent>|auto
[--dry-run]`, `mirqab helper enable|disable|status [--dry-run]`.

## Rejected

- **A udev rule making `pwm` files writable by a group.** No failsafe when
  the user's process dies; recorded in architecture §8.1.
- **`fancontrol` or `thinkfan` as the backend.** Both are root daemons
  with their own config files; driving them means writing under `/etc`
  and restarting services per change.
- **CoolerControl's daemon.** GPL-3.0 and far larger than the need;
  reference only.
- **Putting the curve loop in the helper.** More code as root. The helper
  stays a validator with a deadman; the logic stays unprivileged.
- **A D-Bus system service with polkit actions per call.** A prompt, or a
  rules file to avoid one; the allow-list is simpler to audit.
- **Fan control through MCP.** An agent has no business setting fans.
- **Loading kernel modules for the user.** Root, and machine-specific.

## Tests and fixtures

Add to `root-desktop`: a GPU hwmon with `fan1_input`, `pwm1`,
`pwm1_enable`; a Super-I/O hwmon (`nct6798`) with five fans and five pwm
channels with different enable values; `.t1` twins. `root-minimal` has no
fans. `root-laptop` has a read-only fan with no `pwm_enable`.

The helper is tested unprivileged against a fixture root: it takes
`--root DIR --socket PATH --allow FILE` in test builds only (a cargo
feature, absent from release builds, and lint checks the release binary
has no `--root` string).

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/fans.rs` | Discovery; stable ids across renumbered hwmon directories; duty percent; mode from enable values; no fans gives an empty list |
| `crates/mirqab-core/tests/curve.rs` | Interpolation; hysteresis (a temperature oscillating by 2 °C sends nothing); the 2-point threshold; helper unreachable leaves Auto |
| `crates/mirqab-fand/tests/protocol.rs` | **Every refusal**: uid not allowed, unknown chip, a channel that does not exist, below the floor, above 100, over-temperature, oversized request, rate limit, an `op` it does not know, malformed JSON |
| `crates/mirqab-fand/tests/failsafe.rs` | Deadman after 30 s (clock injected); disconnect; SIGTERM; `--restore` from the state file; the over-temperature watch; original enable values come back exactly, including a driver-specific 5 |
| `crates/mirqab-fand/tests/paths.rs` | A hwmon directory that is a symlink out of the fixture's `sys/devices` is ignored; a `pwm1` that is a symlink is refused |
| `tests/test-helper.sh` | `helper enable --dry-run` prints the three lines and runs nothing; the stub `pkexec` records one call with the fixed script and a numeric uid |
| `tests/test-ui-fans.sh` (`mirqab-ui --selftest fans`, plan 001) | Phase 1 page; the banner; controls hidden with the helper disabled |
| `tools/lint.sh` | `pkexec` and `sudo` appear only in `elevate.rs`; `mirqab-fand` depends on `libc` and `serde_json` only |

## Acceptance

1. With the helper disabled, the GPU fan's speed shows and no control is
   offered; nothing runs as root.
2. `mirqab helper enable --dry-run` changes nothing. Without `--dry-run`,
   one prompt, and the service is up.
3. A manual duty takes effect; killing the user daemon returns the fan to
   automatic within 30 s; stopping the helper returns it at once.
4. A curve follows a temperature change without hunting.
5. `mirqab helper disable` leaves no service enabled, no file under
   `/etc/mirqab`, and every fan automatic.
6. The failsafe and refusal suites pass.

The owner runs acceptance 2 to 5 by hand: they change real hardware.

## Checklist

Phase 1:

1. Resolve Unverified.
2. `crates/mirqab-core/src/samplers/fans/{mod,discover}.rs`; series;
   `tests/golden/fans.json`; `tests/fans.rs`.
3. `crates/mirqab-view/src/tabs/fans.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/fans/{mod,page,fan_card,dial,temp_list,helper_banner}.rs`.
4. CLI `fans`; bar item.

Phase 2 (its own pull request, reviewed as security-sensitive):

5. `crates/mirqab-fand/tests/{protocol,failsafe,paths}.rs` written first.
6. `crates/mirqab-fand/src/{main,proto,hwmon,deadman,allow,energy}.rs`.
7. `dist/mirqab-fand.service`, `dist/helper-enable`, `dist/helper-disable`.
8. `crates/mirqab/src/elevate.rs`: the one `pkexec` call site;
   `crates/mirqab/src/cli/helper.rs`.
9. `crates/mirqab-core/src/fans/{client,curve,control}.rs`; the actions.
10. `crates/mirqab-ui/src/tabs/fans/{mode_picker,curve_editor}.rs`.
11. RAPL through the helper: `power.package` in plan 002's sampler.
12. `docs/helper.md`: what the helper can do, what it cannot, how to
    remove it.

## Unverified

- The `pwm1_enable` values `amdgpu` accepts and whether a manual duty
  holds (the driver may clamp to a minimum): test by hand, as the owner.
- That `SystemCallFilter` and `ProtectSystem=strict` with
  `ReadWritePaths` over sysfs still allow the writes: check with
  `systemd-analyze security` and a real start.
- Whether logind's `PrepareForSleep` reaches the user daemon in time to
  release control before suspend; if not, the helper watches
  `/sys/power/state` itself.
- Laptop drivers' behaviour is unverified until run on one.
