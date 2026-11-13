# Plan 010: Disk tab

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: HIGH for one part: cleanup deletes the user's files. That part
  is its own phase, with its own review. The rest is read-only and LOW.
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

How full are the disks, what is reading and writing, are the drives
healthy, and where did the space go. Stat Space's Disk tab shows volumes,
read and write activity, drive health, and storage tools that find
reclaimable developer caches (https://statspace.app/, features list, read
2026-11-13).

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Mounted filesystems | `/proc/self/mountinfo`; skip pseudo filesystems (`proc`, `sysfs`, `tmpfs`, `devtmpfs`, `overlay` of containers, `squashfs`, `fuse.portal`…) | no |
| Space | `statvfs(mountpoint)` | no |
| btrfs: several subvolumes on one filesystem | Group mounts by device (`major:minor` in mountinfo, or the btrfs fsid in `/sys/fs/btrfs/<fsid>/`); the root is LUKS plus btrfs here **[machine]**. `statvfs` on btrfs is approximate | no |
| Device model, size, rotational, removable | `/sys/block/<dev>/{size,queue/rotational,removable,device/model}` | no |
| Device stack (partition → LUKS → filesystem) | `/sys/block/<dev>/slaves/`, `/sys/block/dm-N/dm/{name,uuid}` | no |
| I/O per device | `/proc/diskstats`: reads, sectors read, writes, sectors written, I/O time (fields 4, 6, 8, 10, 13) | no |
| I/O per process | `/proc/<pid>/io` | own processes only |
| Drive temperature | hwmon `nvme` / `drivetemp` (plan 002) | no |
| SMART (NVMe) | UDisks2 `org.freedesktop.UDisks2.NVMe.Controller` on drive objects: `SmartUpdate`, `SmartGetAttributes`, `SmartPowerOnHours`, `SmartTemperature`, `SmartCriticalWarning`, `SmartSelftestStart` **[machine]** (three NVMe drives) | no (UDisks2's polkit rules allow the active session to read; self-tests prompt) |
| SMART (SATA) | UDisks2 `org.freedesktop.UDisks2.Drive.Ata`: `SmartUpdate`, `SmartGetAttributes`, `SmartFailing` **[unverified]**, no SATA drive here | no |
| Eject, power off | UDisks2 `Filesystem.Unmount`, `Drive.Eject`, `Drive.PowerOff` | no for removable media |
| Swap | `/proc/swaps`; zram **[machine]** | no |
| Reclaimable space | A user-started scan of the home directory (below) | no |
| Package cache | `/var/cache/pacman/pkg` size, read-only; `paccache` is installed **[machine]** | reporting: no. Cleaning: **yes**, not done by this app |
| Docker space | `docker system df --format '{{json .}}'` when the user is in the `docker` group | no (group) |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| APFS snapshots, purgeable space | Replaced by btrfs awareness: one row per filesystem, subvolume mounts listed under it. Snapshot sizes need root (`btrfs qgroup`), so: dropped |
| Per-process disk activity for all processes | Own processes only; the page says "your processes" |
| SMART without UDisks2 | Absent; `smartctl` needs root and is not installed **[machine]** |

## Design

### Stream `disk`

Rates every 1 s while the activity graph is on screen (the `live` flag of plan 001; owner, 2026-11-13) and every 2 s for any other subscriber; space every 30 s; the device list on
udev change.

```json
{"filesystems":[{"id":"…","label":"System","fstype":"btrfs","device":"nvme0n1","encrypted":true,
                 "mounts":["/","/home"],"totalBytes":0,"usedBytes":0,"availBytes":0,
                 "removable":false,"readOnly":false}],
 "devices":[{"name":"nvme0n1","model":"…","sizeBytes":0,"kind":"nvme","celsius":38.0,
             "readPerSec":0,"writePerSec":0,"busyPercent":0.0,
             "health":{"state":"ok","percentUsed":3,"powerOnHours":1200,"at":0}}],
 "topIo":[{"pid":1,"name":"…","readPerSec":0,"writePerSec":0}],
 "swap":[{"name":"zram0","sizeBytes":0,"usedBytes":0}]}
```

Health is read from the properties UDisks2 already caches. `SmartUpdate`
(which wakes the drive) is called only when the user presses "Refresh
health", and at most once a day in the background while the page is open.

Series: `disk.<n>.read`, `disk.<n>.write` for the first four physical
devices (bytes per minute, from counters), `fs.root.used`.

### Storage tools (phase 2: scan, report only)

A scan the user starts, with progress and cancel, at idle I/O priority
(`ioprio_set` idle class), never crossing filesystem boundaries, never
following symlinks, never leaving `$HOME` except the read-only reports.

| Finding | Rule |
|---------|------|
| `node_modules` | A directory named `node_modules` whose parent has `package.json`; "stale" when nothing in the project (excluding `node_modules`) changed in 30 days |
| Rust `target` | A `target` directory beside `Cargo.toml` containing `CACHEDIR.TAG` |
| Python | `.venv`, `venv` beside `pyproject.toml` or `requirements.txt`; `__pycache__` |
| Build output | `.next`, `dist`, `build`, `.turbo`, `.gradle`, only beside a known project marker |
| Tool caches | `~/.cache/<name>` per directory; `~/.npm/_cacache`, `~/.cargo/registry/cache`, `~/.cache/pip`, `~/go/pkg/mod`, `~/.bun/install/cache` |
| Trash | `~/.local/share/Trash` |
| Docker | from `docker system df`: reclaimable images and build cache (report; cleaning is `docker system prune`, shown as a command) |
| Package cache | `/var/cache/pacman/pkg` size, with the command `paccache -r` to copy. This app does not run it |

Results are grouped by project and sorted by size. Nothing is selected by
default.

### Cleanup (phase 3: deletion, separate pull request)

- Only paths the scan found, re-checked at deletion time against the same
  rule (the directory still matches; it is still under `$HOME`; its
  `realpath` has no symlink component leaving `$HOME`; it is not a mount
  point).
- `disk.clean {paths}` needs `confirm`; the dialog lists every path and
  size and the total. `dryRun` returns the list.
- Deletion goes through `act::remove_path`, which refuses: any path with
  fewer than three components under `$HOME`; `$HOME` itself; any path not
  in the current scan result; any path on another filesystem than its
  parent.
- Default action is **move to the Trash** (the freedesktop Trash
  specification, https://specifications.freedesktop.org/trash-spec/) for
  anything under 1 GB, so it can be undone; larger directories are deleted
  after a second explicit confirmation naming the size. Caches under
  `~/.cache` are deleted directly.
- Every deletion is logged to `event` (`kind: "clean"`, path, bytes).

### Eject

`disk.eject {device}`: unmount then power off through UDisks2. Needs
`confirm` when a process of the user has open files there (listed).
Internal drives offer no eject.

### Page

1. Filesystems: a meter each, used and free, the mounts under it, an
   "encrypted" tag, eject for removable ones.
2. Activity: read and write graph with the range picker; per-device rows;
   "your processes" by I/O.
3. Health: one card per drive: state word (OK, Watch, Failing), percent
   used or reallocated sectors, temperature, power-on hours, "Refresh
   health".
4. Reclaim space: Scan, then the grouped results, then Clean.

Pinnable cards: `disk.space`, `disk.activity`. CLI: `mirqab disk [--json]`
(filesystems, devices and health; the scan and cleanup stay in the window).

## Rejected

- **`du` or `ncdu` as the scanner.** No rules, no progress, no cancel.
- **A background or scheduled scan.** I/O nobody asked for.
- **Auto-selecting "safe" caches.** The user picks; nothing is pre-ticked.
- **Running `paccache` or `docker prune` for the user.** Root, or data
  loss outside the home directory; the command is shown to copy.
- **`smartctl`.** Root.
- **`rm -rf` through a shell.** Deletion is a Rust walk with the checks
  above, with `openat` and `O_NOFOLLOW` so a symlink swapped in mid-walk
  cannot redirect it.

## Tests and fixtures

Add to `root-desktop`: `proc/self/mountinfo` (btrfs with two subvolumes on
a dm-crypt device, an EFI partition, a removable vfat, pseudo filesystems
to skip), `proc/diskstats` with a `.t1` twin, `sys/block/…`, `proc/swaps`.
`statvfs` is injected (a table keyed by mount point, as haseen's sidecar
does with its fake `Statfs`). A fake UDisks2 on the private bus with one
healthy NVMe, one worn, one failing ATA.

`tests/fixtures/home-clean/` is **built by a script at test time in the
sandbox**, never in the checkout: projects with `node_modules` (fresh and
stale by mtime), a `target` with and without `CACHEDIR.TAG`, a directory
merely *named* `node_modules` with no `package.json` beside it, a symlink
pointing outside the home, a nested mount point marker.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/disk.rs` | Pseudo filesystems skipped; btrfs subvolumes are one filesystem; the device stack; rates from the delta; health states from the fake UDisks2 |
| `crates/mirqab-core/tests/scan.rs` | Each rule's positive and negative case; stale by mtime; cancel stops within one directory; the scan never opens a path outside the home |
| `crates/mirqab-core/tests/clean.rs` | **Every refusal**: path not in the scan result; shallow path; the home itself; a symlinked component; a mount point; a directory that stopped matching between scan and clean. Trash placement and its `.trashinfo`; `dryRun` deletes nothing; the log row |
| `tests/test-ui-disk.sh` (`mirqab-ui --selftest disk`, plan 001) | Cards; nothing pre-selected; the dialog total equals the selection |

## Acceptance

1. Filesystems, activity and health match `df`, `iostat`-style reading of
   `/proc/diskstats` and `udisksctl info` on the development machine.
2. A scan of a test tree finds exactly the planted caches.
3. Clean moves a selected `node_modules` to the Trash after the dialog,
   and refuses each crafted bad path in the test suite.
4. With the page closed no file under the home directory is read by the
   daemon.

## Checklist

Phase 1 (read-only):

1. Resolve Unverified.
2. `crates/mirqab-core/src/samplers/disk/{mod,mounts,devices,io,health}.rs`;
   `dbus/udisks2.rs`.
3. Series; `tests/golden/disk.json`; `tests/disk.rs`.
4. `crates/mirqab-view/src/tabs/disk.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/disk/{mod,page,filesystem_card,activity_card,health_card,space_card}.rs`.
5. Eject action and its dialog; CLI `disk`.

Phase 2 (scan):

6. `crates/mirqab-core/src/scan/{mod,rules,walk}.rs`; `disk.scan`,
   `disk.scanCancel`, progress events on the `disk` stream.
7. `crates/mirqab-ui/src/tabs/disk/{reclaim_card,finding_row}.rs`; `tests/scan.rs`.

Phase 3 (cleanup; separate pull request and review):

8. `act::remove_path` with the refusals; `act::trash`; `disk.clean`.
9. `tests/clean.rs` written **before** the implementation.
10. The confirm dialog wiring and the second confirmation for large
    direct deletes.

## Unverified

- UDisks2's ATA SMART interface and attribute names: read
  http://storaged.org/doc/udisks2-api/latest/ and test on a SATA machine,
  or mark SATA health experimental.
- Which UDisks2 calls prompt through polkit for an active session
  (`SmartUpdate`, `SmartSelftestStart`): try each and record.
- The NVMe "percentage used" attribute key returned by
  `SmartGetAttributes`: record a scrubbed sample.
- btrfs `statvfs` accuracy with mixed profiles: compare with
  `btrfs filesystem usage` by hand and note the difference in the page's
  help text.
