# Plan 019: Support tab and the health report

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: LOW for the system, MEDIUM for privacy: a report is made to
  be shared, so what goes into it is the whole design.
- **Depends on**: 010, 012, 013, 018
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner; free in the reference too
- **State**: PLANNED

## Problem

Stat Space's Support tab produces a health report for a machine
(battery against design capacity, SSD wear, thermal and memory pressure
history, stability, a few hands-on tests) to share when selling or
troubleshooting, plus diagnostics for the app itself. This plan builds
the same idea from Linux sources, as a local file the user chooses to
share.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Machine model and vendor | `/sys/class/dmi/id/{sys_vendor,product_name,product_version,board_name,bios_version,bios_date}` (world-readable) | no |
| Serial numbers | `/sys/class/dmi/id/{product_serial,board_serial,product_uuid}` are mode 0400 root **[machine]**: not readable, and not wanted | not read |
| CPU, memory size, GPUs | Plan 002's sampler | no |
| Memory modules (count, speed) | DMI table type 17 needs root (`dmidecode`): dropped. Total size only | — |
| Battery health | Plan 013 | no |
| Drive health and wear | Plan 010 (UDisks2 SMART) | no |
| 30 days of pressure and heat | `history.db` (plan 018's summary) | no |
| Stability: unclean shutdowns | Boot list from `journalctl --list-boots -o json` and whether each previous boot's last entries include a clean shutdown target **[unverified]**; needs journal read access (group `systemd-journal` or `wheel`) | group |
| Stability: kernel errors | `journalctl -k -p err -b --output=json` counts by message class (machine check, I/O error, GPU reset, OOM); same access | group |
| Stability: crashed programs | `coredumpctl list --json=short` (count and program names of the user's own dumps) **[unverified]** | no for own |
| Failed units | `systemctl --failed --output=json` and `systemctl --user --failed --output=json` | no |
| Kernel taint | `/proc/sys/kernel/tainted` | no |
| Firmware updates pending | `fwupdmgr get-updates --json` if installed, on request only | no |
| Hands-on tests | Display: full-screen colour fields; keyboard: a key tester; speakers: a left/right tone through PipeWire; microphone: a level meter; camera: not included (needs a video pipeline) | no |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Apple diagnostics codes | No equivalent |
| Memory module details | Needs root: dropped |
| Battery cycle limits per model | No table of rated cycles: the report shows cycles and capacity, no verdict from cycles |
| Camera test | Dropped for now |
| A hosted share link | Dropped: a file, which the user sends however they like |

## Design

### The report

`support.report {sections, options}` builds a JSON document in the
daemon; the interface and the CLI render it. Sections: Machine, Battery,
Storage, Thermal and pressure (30 days), Stability, Hands-on tests
(results the user recorded this session), and a verdict line per section
(Good, Attention, Problem) with the rule that produced it written beside
it.

**What is never in a report:** serial numbers, UUIDs, MAC addresses,
hostnames, user names, home paths, SSIDs, IP addresses, process command
lines, device names of peripherals, mount paths beyond `/` and `/home`.
The builder works from an allow-list of fields, not a deny-list: a field
is in the report only if `report/schema.rs` names it.

**Options, all off by default:** include Bluetooth and USB device names;
include the list of installed kernel modules; include the drive's model
string (it is on by default, since a buyer needs it; listed here because
it can be turned off).

Before saving, the page shows the report exactly as it will be written
("This is everything that will be in the file"), with a Save button.

Formats: Markdown and JSON written by the daemon; PDF by the interface
through `gtk::PrintOperation` with the export action and a file name
(no dialog) **[unverified]**, or dropped in favour of HTML if that does
not work without a print portal. Saved to a path
the user picks (default `~/Documents/mirqab-report-<date>.md`); the date
is the only variable part of the name.

CLI: `mirqab report [--json|--markdown] [--out FILE]`.

### Stability rules

| Verdict | Rule |
|---------|------|
| Problem | A machine-check error, a failing drive, or more than three unclean shutdowns in 30 days |
| Attention | Kernel I/O errors, GPU resets, OOM kills, a worn drive (over 80 % used), battery under 80 % of design, sustained memory pressure |
| Good | None of the above |

Without journal access the Stability section says which checks were
skipped and why, and gives no verdict rather than "Good".

### Diagnostics for the app itself

A second card, "About and diagnostics":

- Versions: the app, the daemon protocol, GTK and libadwaita (run-time versions),
  kernel; the
  capabilities list with what is missing and which package provides it.
- Self-cost now and over 30 days (the `self` series).
- The daemon's log (the last 200 lines from the user journal for
  `mirqab.service`), with a Copy button. Logs are written without paths
  under the home directory or process arguments (a rule from plan 001,
  tested there).
- The MCP action log: every destructive action an agent asked for, with
  its outcome (`event` rows of kind `mcp-action`).
- The helper's status (plan 015).
- "Reset": clear history, clear caches, reset settings. Each is an
  action with `confirm` and `dryRun`, and each says what it deletes.
- Links: the repository's issues page
  (`https://github.com/t1nk333r/mirqab/issues`), the licence,
  the attributions from `NOTICE.md`.

No "contact support" form, no upload, no telemetry.

## Rejected

- **`inxi`, `hw-probe`, `dmidecode` output pasted in.** They include
  identifiers by default, and two of them want root.
- **Uploading to a paste service.** The user's choice of channel, not
  the app's.
- **A deny-list scrubber over free text.** It fails open. The report has
  no free text from the system except model strings.
- **A stress test.** Heat and wear for a number nobody can interpret.
- **A score out of 100.** A verdict with its rule is honest; a score is
  not.

## Tests and fixtures

Add to `root-desktop` and `root-laptop`: `sys/class/dmi/id/*` with
made-up vendor and model strings, and serial files present and readable
in the fixture **on purpose**, holding the marker string
`SERIAL-MUST-NOT-APPEAR`. Stubs for `journalctl`, `coredumpctl`,
`systemctl`, `fwupdmgr` printing synthetic JSON (a clean history, an
unclean one, and "access denied").

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/report.rs` | Each section from fixtures; each verdict rule, positive and negative; no journal access gives "not checked", never "Good" |
| same | **Privacy**: the marker serial, the fixture's hostname, user name, home path, MAC, SSID and address markers appear nowhere in the JSON or Markdown, with every option on and off |
| same | The schema allow-list: a field added to a sampler's output does not reach the report unless the schema names it |
| `tests/test-ui-support.sh` (`mirqab-ui --selftest support`, plan 001) | The preview equals the saved file byte for byte; the options change it |
| `tests/test-cli.sh` | `report --json` validates against `share/mirqab/report.schema.json` |
| `tests/test-reset.sh` | Each reset's `--dry-run` deletes nothing and lists what it would |

## Acceptance

1. A report for the development machine reads sensibly to a person and
   contains no identifier (the owner reads it before anything else is
   done with it).
2. On a laptop the battery section matches plan 013's page.
3. The saved file equals the preview.
4. The diagnostics card shows versions, missing optional packages and
   the MCP action log.

## Checklist

1. Resolve Unverified.
2. `crates/mirqab-core/src/report/{mod,schema,machine,stability,render_md}.rs`;
   `share/mirqab/report.schema.json`.
3. `crates/mirqab-core/src/samplers/journal.rs` (the three commands
   through `exec.rs`, each bounded by `--lines` and a time limit).
4. Tests, with the privacy test written first.
5. `crates/mirqab-view/src/tabs/support.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/support/{mod,page,report_card,report_preview,tests_card,display_test,key_test,sound_test,diagnostics_card,reset_card}.rs`.
6. CLI `report`; the reset actions.

## Unverified

- Nothing about the DMI serial files: the code never opens them, and
  lint greps for the three names.
- How to tell an unclean shutdown from the journal reliably; if there is
  no robust signal, drop that line rather than guess.
- `coredumpctl` and `journalctl --list-boots` JSON output on systemd 262 **[machine]**
  and what the user can read without the journal group.
- Exporting a PDF with `gtk::PrintOperation` and no dialog on a
  Hyprland session; otherwise write HTML.
- A full-screen window for the display test on Hyprland: a second
  `gtk::Window` with `fullscreen_on_monitor`, or a layer surface on the
  overlay layer anchored to all four edges; try the plain window first.
