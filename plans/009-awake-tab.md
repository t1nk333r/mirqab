# Plan 009: Awake tab

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW. An inhibitor is a file descriptor; closing it, or the
  daemon dying, releases it. The worst failure is a machine that stays
  awake until the daemon exits.
- **Depends on**: 001, 005
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Keep the machine awake for a while, and see what else is keeping it awake.
Stat Space's tab has timers, "until a task exits", options for the display
and the lid, a list of what blocks sleep with a 7-day log, and (Pro) rules
such as "while on the charger" or "while a download runs"
(https://statspace.app/features/keep-awake, read 2026-11-13). The owner
asked for it on `systemd-inhibit`.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Take an inhibitor | `org.freedesktop.login1.Manager.Inhibit(what, who, why, mode)` on the system bus returns a file descriptor (https://systemd.io/INHIBITOR_LOCKS/); `org.freedesktop.login1` is on the bus **[machine]** | no, for `sleep` and `idle`. `handle-lid-switch` is allowed for the active local session by logind's default polkit rules **[unverified]** |
| Who blocks sleep | `Manager.ListInhibitors()` → `(what, who, why, mode, uid, pid)`. Five inhibitors seen, all `delay` mode **[machine]**: only `block` mode stops sleep, so the list filters on it and shows `delay` ones separately as "will delay" | no |
| Inhibitor changes | No signal exists for the list; poll every 5 s while the page is visible, every 60 s for the log | no |
| Suspend and resume times | `Manager.PrepareForSleep(bool)` signal | no |
| Idle inhibition as the compositor sees it | Wayland `idle-inhibit` needs a surface; the daemon has none. Hyprland's idle daemon honours logind `idle` inhibitors by default **[unverified]** | no |
| On AC power | UPower `org.freedesktop.UPower` property `OnBattery`, with `PropertiesChanged` | no |
| Process exit | `pidfd_open(pid)` and poll | no (own or any visible pid) |
| Download running | The `net` sampler's receive rate (plan 011); until then, the rule is unavailable | no |
| Audio playing | PipeWire stream state via `pw-dump` (plan 014) | no |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| "Prevent sleep when the lid closes" | **Possible** (`handle-lid-switch`), kept, with a warning about heat in a bag |
| "Keep the display on" vs "keep the system on" | Kept: `idle` (display and auto-suspend from idleness) vs `sleep` (explicit suspend too) |
| Listing which app holds a display-sleep assertion through Wayland idle-inhibit | Limited: the compositor does not publish it. Hyprland: `hyprctl clients -j` may show an inhibiting window **[unverified]**; otherwise only logind inhibitors are listed, and the page says so |

## Design

### What a hold is

A hold is one logind inhibitor owned by the daemon, `who = "mirqab"`,
`why` = the reason shown in the page, `mode = block`.

| Option | `what` |
|--------|--------|
| Keep the system awake (default) | `sleep:idle` |
| Keep the display on too | the same (idleness covers the display); without this option the hold is `sleep` only, so the screen may blank while the machine stays up |
| Ignore the lid | adds `handle-lid-switch` |

Ends: a timer; "until process N exits" (pidfd); "until HH:MM"; manual off.
Only one manual hold exists at a time; starting another replaces it.
Holds do not survive a daemon restart, on purpose: an inhibitor nobody
remembers taking is worse than one that lapsed. The active hold is written
to `$XDG_RUNTIME_DIR/mirqab/awake.json` only so the CLI can report it.

A safety stop: on battery below 10 % a hold with "ignore the lid" is
released and a notification says why.

### Rules (later phase, same plan)

Off by default, each a switch in the page: while on the charger; while a
chosen process name is running (checked every 30 s against plan 006's
cache); while a download is active (receive rate above 100 kB/s for 30 s;
needs plan 011); while audio plays (needs plan 014). A rule that is on
keeps its one input alive (a D-Bus match, or a 30 s check), which is
within the idle budget; the benchmark gains a rules mode.

### Stream `awake`

```json
{"hold":{"active":true,"what":["sleep","idle"],"reason":"timer","endsAt":0,"pid":null,"lid":false},
 "blockers":[{"who":"…","why":"…","what":["sleep"],"mode":"block","pid":1234,"app":"…","own":false}],
 "delays":[{"who":"NetworkManager","what":["sleep"]}],
 "rules":[{"id":"charger","on":false,"active":false}],
 "lastSleep":{"at":0,"resumedAt":0}}
```

Blockers seen are written to the `event` table (`kind: "blocker"`, who,
why, start and end) and kept 7 days.

### Actions

`awake.start {duration|untilPid|untilTime, display, lid}`, `awake.stop`,
`awake.rule {id, on}`. Reversible, so no confirmation; `dryRun` returns the
inhibitor that would be taken. The app never releases another program's
inhibitor; the page offers "Quit <app>" through plan 006 instead.

### Page

- A large on/off control with the remaining time; duration chips (15 m,
  30 m, 1 h, 2 h, 4 h, until I turn it off); "until a process exits" with a
  picker of the user's processes; switches for the display and the lid.
- "What's keeping this machine awake": blockers with the app, the reason
  it gave and how long.
- "Recently": the 7-day log, grouped by day.
- Rules.
- Pinnable card `awake.toggle`. Bar item `awake`. Sidebar tile marked
  while a hold is active.

### CLI and MCP

`mirqab awake 2h [--display] [--lid]`, `mirqab awake --until-pid N`,
`mirqab awake off`, `mirqab awake` (status), `mirqab blockers [--json]`.
MCP: `keep_awake` (duration capped at 12 h from MCP; no lid option from
MCP), `sleep_blockers`.

## Rejected

- **Spawning `systemd-inhibit sleep infinity`.** A child process per hold
  and no way to change the reason; the D-Bus call is the same thing
  without the process.
- **A Wayland idle-inhibit surface in the interface.** It would only work
  while the window is open.
- **Simulating input.** Dishonest and breaks idle detection for other
  tools.
- **Persisting holds across restarts.** See above.

## Tests and fixtures

Tests use the private bus of plan 004 with a fake `login1` and `UPower`
(a small test helper serving `Inhibit`, `ListInhibitors`, `PrepareForSleep`
and `OnBattery`, recording calls).

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/awake.rs` | `what` for each option combination; the timer releases the descriptor; a replaced hold releases the old one; daemon shutdown releases; `untilPid` releases when a spawned `sleep 1` exits |
| same | Blockers: `delay` entries are not blockers; the daemon's own hold is marked `own`; log rows open and close |
| same | The charger rule follows `OnBattery`; low battery releases a lid hold |
| `tests/test-ui-awake.sh` (`mirqab-ui --selftest awake`, plan 001) | The control, chips, the blocker list |
| `tests/test-cli.sh` | Duration parsing (`90m`, `2h`, `1h30m`), `awake off`, `blockers --json` |
| `tools/bench-idle.sh --rule charger` | Idle budget holds with a rule on |

## Acceptance

1. `mirqab awake 1m` appears in `systemd-inhibit --list` as `mirqab` in
   `block` mode and disappears after a minute.
2. The machine does not auto-suspend during a hold and does afterwards
   (an owner check on a laptop).
3. Another program's `block` inhibitor appears in the list within 5 s with
   its reason.
4. Stopping the daemon releases the hold.

## Checklist

1. Resolve Unverified.
2. `crates/mirqab-core/src/dbus/{mod,login1,upower}.rs` (shared with plans
   012 and 013).
3. `crates/mirqab-core/src/samplers/awake/{mod,hold,blockers,rules}.rs`.
4. `tests/golden/awake.json`; the bus helper under `tests/helpers/`;
   tests.
5. CLI, MCP, bar item.
6. `crates/mirqab-view/src/tabs/awake.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/awake/{mod,page,hold_control,blocker_list,log_list,rules,toggle_card}.rs`.
7. Rules for charger and process; leave download and audio rules behind
   capability checks until plans 011 and 014 are merged.

## Unverified

- That logind lets an unprivileged active session take
  `handle-lid-switch` (`org.freedesktop.login1.inhibit-handle-lid-switch`
  polkit action): check `pkaction --verbose` and a live call on a laptop.
- That the owner's idle daemon honours logind `idle` inhibitors: check its
  documentation and a live test (take a hold, wait past the idle timeout).
- `hyprctl clients -j` field for an inhibiting window: `hyprctl` could not
  reach the compositor from the planning shell.
