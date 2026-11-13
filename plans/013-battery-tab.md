# Plan 013: Battery tab

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW, with one catch: the development machine is a desktop with
  no system battery (`/sys/class/power_supply` holds only a peripheral's
  battery **[machine]**), so this tab cannot be seen working there. It
  needs a laptop for acceptance.
- **Depends on**: 001, 012
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner's "every tab that Linux
  allows"
- **State**: PLANNED

## Problem

Stat Space has a Battery tab: level, health as capacity against design,
cycles, charging power and time left. It is not in the owner's list of
tabs, because their screenshots come from a view without it, but the brief
says "every Stat Space tab that Linux allows". The tab appears only on a
machine that has a system battery; on the owner's desktop the tile is
hidden.

## Linux data sources

No root.

| Data | Source |
|------|--------|
| A system battery exists | `/sys/class/power_supply/*/type` = `Battery` and `scope` absent or `System` |
| Level | `capacity` (percent); `energy_now` / `energy_full` (µWh) or `charge_now` / `charge_full` (µAh) |
| Health | `energy_full` against `energy_full_design` (or the `charge_` pair) |
| Cycles | `cycle_count` (many firmwares report 0: then shown as unknown) |
| State | `status`: `Charging`, `Discharging`, `Full`, `Not charging` |
| Power | `power_now` (µW), or `current_now × voltage_now` |
| Time left | `energy_now / power_now`, smoothed over a minute; UPower's `TimeToEmpty` / `TimeToFull` when present |
| Charger | `/sys/class/power_supply/A*/online`; UPower `OnBattery` |
| Charge limit | `charge_control_end_threshold` (read only here) |
| Chemistry, maker, model | `technology`, `manufacturer`, `model_name` |
| Changes | UPower `PropertiesChanged` on the battery device; power is sampled every 1 s while the power graph is on screen (the `live` flag of plan 001; owner, 2026-11-13) and every 2 s while only a card without a graph is visible |
| Power profile | `org.freedesktop.UPower.PowerProfiles` `ActiveProfile` (on the bus **[machine]**), read only |

Kernel documentation: https://docs.kernel.org/power/power_supply_class.html.

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Battery drain per app | Dropped (architecture §1.2) |
| Adapter wattage and name | Only through `typec`/`usb_power_delivery` (plan 012); otherwise absent |
| Battery temperature | Shown when `temp` exists; often it does not |
| "Optimised charging" control | Dropped: the threshold is root-writable and vendor-specific. Shown read-only |

## Design

### Stream `battery`

Absent from `capabilities` on a machine with no system battery.

```json
{"percent":83,"state":"discharging","watts":9.5,"timeLeftSec":18000,
 "health":{"percent":91,"fullWh":52.1,"designWh":57.0,"cycles":212},
 "onAc":false,"chargeLimitPercent":80,"profile":"balanced",
 "model":"…","technology":"Li-poly","celsius":null}
```

Series: `battery.level`, `power.battery` (already in plan 002).
Events: `charger` plugged and unplugged.

### Page

1. Level: a large percent, state, time left or time to full, watts.
2. A level graph with the range picker; charger periods shaded.
3. Health: capacity now against design in Wh (not only a percent), cycles,
   a word (Good above 80 %, Fair above 60 %, Service below).
4. Details: chemistry, model, charge limit, power profile.

Pinnable card `battery.level`. Bar item `battery`. CLI `mirqab battery
[--json]`. Low-battery notifications are the desktop's job (haseen has
them); none here.

## Rejected

- **Showing the tab greyed out on desktops.** A dead tile is clutter.
- **Writing charge thresholds.** Root, vendor-specific, and a wrong write
  can stop a laptop charging.
- **Folding battery into the System tab.** The reference has a tab, and a
  laptop user wants the history graph.

## Tests and fixtures

`root-laptop`: `sys/class/power_supply/BAT0/` (energy-based) and a second
tree `root-laptop-charge/` (charge-based, `cycle_count` 0, no
`power_now`), each with a `.t1` twin; `AC/online`. A fake UPower.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/battery.rs` | Both unit families give the same shape; watts from current and voltage; health; cycles 0 becomes `null`; time left smoothing; no battery means no capability |
| `tests/test-ui-battery.sh` (`mirqab-ui --selftest battery`, plan 001) | The page on `root-laptop`; the tile absent on `root-desktop` |
| `tests/test-cli.sh` | `battery --json`; exit code 1 with a clear line on a machine without one |

## Acceptance

1. On a laptop (an owner step: none is at hand for the planner) the
   numbers match `upower -i` for the battery.
2. On the desktop the tile, the bar item and the CLI all say there is no
   battery, without errors.

## Checklist

1. `crates/mirqab-core/src/samplers/battery/{mod,sysfs}.rs`, reusing
   `dbus/upower.rs` from plan 009 or 012.
2. Series, events, `tests/golden/battery.json`, tests.
3. `crates/mirqab-view/src/tabs/battery.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/battery/{mod,page,level_card,health_card,details_card}.rs`;
   the sidebar hides the tile without the capability.
4. Bar item and CLI.

## Unverified

- Everything hardware-specific here, until run on a real laptop. The
  owner has other machines; which one to test on is their call.
