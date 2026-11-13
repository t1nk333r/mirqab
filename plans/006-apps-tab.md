# Plan 006: Apps tab and process inspector

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MEDIUM. Reading every `/proc/<pid>` every 2 s is the most
  expensive sampler in the app, and Quit and Force quit are the first
  destructive actions.
- **Depends on**: 001, 005
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Stat Space's Apps tab shows "one row per app, and every process when you
need it": helper processes are folded into their app, and an inspector
explains what a process is (where it came from, whether it is signed and
sandboxed, what started it). The owner asked for a "process inspector
mapped to Linux". Linux has no app bundles and no code signatures, but it
has cgroups, package ownership and sandbox flags, which answer the same
questions.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Process list | `/proc/[0-9]*/` | no |
| Name, state, parent, start time, CPU time, threads | `/proc/<pid>/stat` (fields 2, 3, 4, 14, 15, 20, 22) | no |
| Memory | `/proc/<pid>/statm` (resident, shared pages); `smaps_rollup` `Pss` on demand in the inspector | own processes only for `smaps_rollup` |
| Owner | `/proc/<pid>/status` `Uid` | no |
| Command line | `/proc/<pid>/cmdline` | no |
| Executable | `/proc/<pid>/exe` link; a ` (deleted)` suffix means the file was replaced | own processes; others give EACCES |
| Working directory | `/proc/<pid>/cwd` | own processes |
| **App grouping** | `/proc/<pid>/cgroup`: `0::/user.slice/user-UID.slice/user@UID.service/app.slice/app-<name>-<id>.scope` or `…/<unit>.service`. Seen for this very session **[machine]** | no |
| Desktop entry | `app-<desktop-id>…` in the scope name (systemd's XDG convention, https://systemd.io/DESKTOP_ENVIRONMENTS/), matched to `*.desktop` in `$XDG_DATA_DIRS/applications`; `Name=`, `Icon=` | no |
| Flatpak | scope `app-flatpak-<app.id>-<n>.scope`; `/proc/<pid>/root/.flatpak-info` | no |
| Container | cgroup path containing `docker-<id>.scope` or `/docker/<id>`; name from `docker ps` when the user is in the `docker` group | no (group) |
| I/O | `/proc/<pid>/io` `read_bytes`, `write_bytes` | own processes |
| Hardening | `/proc/<pid>/status`: `NoNewPrivs`, `Seccomp`, `CapEff`; namespaces in `/proc/<pid>/ns/*` compared with the daemon's own | own processes |
| Open files and sockets | `/proc/<pid>/fd/*` links, on demand | own processes |
| Owning package | `pacman -Qo <exe>` on demand, cached per path; other distributions: none, the row is omitted | no |
| Architecture | ELF header of `/proc/<pid>/exe`: byte 4 (class), bytes 18–19 (machine) | own processes |
| Focused window (for "idle but heavy") | Hyprland's event socket `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`, `activewindow` events **[unverified]** | no |
| GPU memory per process | AMD: `/proc/<pid>/fdinfo/*` `drm-memory-*` keys (https://docs.kernel.org/gpu/drm-usage-stats.html) **[unverified]** | own processes |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Code signature, notarisation, developer name | Replaced by: owning package and its repository, "not owned by any package", "executable deleted or replaced", Flatpak app id |
| Rosetta / translated | Replaced by ELF machine; a foreign machine type under `qemu-user` is flagged |
| App Sandbox entitlement | Replaced by Flatpak, seccomp, no-new-privileges, namespaces |
| "Not responding" | Dropped: no compositor reports it portably. Recorded |
| Energy impact | Dropped (architecture §1.2) |
| Other users' details | Limited: name, CPU and memory only. Shown under "System", read-only |

## Design

### Grouping

A process belongs to the first of:

1. its systemd **app scope or service** under the user manager (cgroup
   path), named from the desktop entry when the scope name maps to one;
2. a **Flatpak** app id;
3. a **container** (one group per container);
4. a **terminal session**: processes under a terminal's scope are grouped
   by session id (`stat` field 6) and named after the session leader's
   foreground job, so `vite` in a terminal is not "the terminal";
5. the **process tree** fallback: the highest ancestor that is not `systemd`,
   a shell or a terminal.

Groups owned by other uids collapse into one "System" group, shown last and
closed. Kernel threads (parent 2) are hidden.

A group's CPU is the sum of its members' deltas, in percent of one core
(so it can exceed 100) with a toggle for percent of the machine; memory is
the sum of RSS minus shared pages counted once per group (an estimate; the
inspector's PSS is the honest number).

### Stream `apps`

Every 2 s while subscribed, also while the window is open: the list is a
table, not a live graph, and this walk is the costliest sampler, so the
1 s rule of plan 001 does not apply to it (architecture §5.3). The
inspector's graph of one selected process is a live graph: while it is on
screen that one pid's `stat` and `statm` are read every 1 s. To bound the cost: `stat` and `statm` for every
process each tick; `cgroup`, `cmdline`, `status` only for pids not seen
before (a pid plus start time is the cache key).

```json
{"groups":[{"id":"scope:app-firefox-1234","name":"Firefox","icon":"firefox","kind":"app",
            "cpu":14.2,"rssBytes":0,"procs":23,"threads":310,"ioReadPerSec":0,"ioWritePerSec":0,
            "pids":[1234,1260],"top":{"pid":1260,"name":"Web Content","cpu":9.0}}],
 "totals":{"procs":412,"threads":2100,"cpu":31.0},
 "sort":"cpu"}
```

`params`: `sort` (`cpu`, `mem`, `name`), `limit` (default 50),
`expand` (group ids whose processes are returned in full).

### Inspector (`get apps.inspect {pid}`)

On demand, one process: name, pid, parent chain to the user manager (each
with name and pid), user, started, CPU time, threads, state; executable
path, package and repository, deleted flag, architecture; unit or scope,
Flatpak id, container; sandbox line (seccomp mode, no-new-privs,
capabilities named when non-zero, which namespaces differ); memory (RSS,
PSS, swap); counts of open files and sockets with the first 50 paths;
listening ports (links to Ports once plan 007 lands).

A plain-words verdict heads it, from fixed rules: "Packaged by
`extra/firefox`", "Flatpak app, sandboxed", "Not owned by any package,
running from your home directory", "Executable was replaced since it
started (an update); restart it".

### Actions

`apps.quit {pid|group}` sends SIGTERM; `apps.kill` sends SIGKILL. Both need
`confirm`, list every pid they would signal, and obey the refusals of
architecture §5.4. A group quit signals the group's processes, leaders
first. For a systemd service the action is `systemctl --user stop <unit>`
through `exec.rs`, so the unit does not restart it.

### Page

- A search field, a sort control (CPU, memory, name), and a "show
  processes" switch that flattens groups.
- Rows: icon, name, a small CPU bar and number, memory, process count.
  Expanding a row lists its processes. Selecting a row opens the inspector
  as a side sheet with Quit and Force quit.
- "Idle but heavy": a row is tagged when its group has used over 20 % of a
  core for 5 minutes and none of its windows had focus in that time.
  Hyprland only; without the focus source the tag is absent.
- Pinnable card `apps.top`: the five heaviest groups.

### CLI and MCP

`mirqab apps [--sort cpu|mem] [--all] [--json]`, `mirqab what <pid>
[--json]`, `mirqab quit <pid|name> [-y] [--force] [--dry-run]`. MCP tools
`top_apps` and `what_is_process` (read only).

### History and events

No per-app series (unbounded names). The minute writer stores the top
three groups by CPU and by memory as one `event` row (`kind: "top-apps"`),
which plan 018 uses for spike rewind. That costs one `apps` sample per
minute only if `config.trends.topApps` is on (default on; the cost is
measured in step 9 and the default flips if it breaks the idle budget).

## Rejected

- **Grouping by process name.** Five `node` processes are five things.
- **Grouping only by process tree.** Everything launched from a launcher
  or a terminal collapses into it; cgroup scopes exist to solve this.
- **eBPF for per-process network and energy.** Needs root and a toolchain.
- **Showing `/proc/<pid>/environ`.** It holds tokens.
- **A "renice" or "suspend" control.** Not in the reference; more ways to
  break a session.

## Tests and fixtures

`tests/fixtures/root-desktop/proc/<pid>/` for about thirty synthetic
processes, generated by `tools/mkfixture.sh procs` from a readable table
(`tests/fixtures/procs.tsv`: pid, ppid, uid, name, cgroup, cmdline, exe,
rss, utime…): a browser with children in one scope; a Flatpak app; a
terminal with two shells running `vite` and `cargo`; a `--user` service; a
container process; root-owned daemons (unreadable `exe`: the fixture omits
the link); a kernel thread; a process whose exe is ` (deleted)`. Desktop
entries under `usr/share/applications/`. A `.t1` twin with advanced CPU
times. A stub `pacman` answering `-Qo`.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/apps.rs` | Each grouping rule picks the expected group; the System group; kernel threads hidden; CPU from the delta; sorting and `limit`; a pid reused with a new start time is a new process |
| same | Inspector golden output for five processes; verdict rules; unreadable files give omitted rows, not errors |
| same | Actions under a recording `act`: `wouldDo` lists; refusals for uid 0, pid 1, the daemon, the compositor; a service is stopped through the stub `systemctl` |
| `tests/test-ui-apps.sh` (`mirqab-ui --selftest apps`, plan 001) | The page lists the groups, search filters, the inspector opens, the confirm dialog shows the pids |
| `tools/bench-idle.sh --subscribe apps` | With a 400-process fixture: under 1.5 % of a core |

## Acceptance

1. On the development machine the list shows recognisable apps with their
   names and icons, terminal jobs by name, and a closed System group.
2. The inspector for a packaged binary names its package; for a binary in
   the home directory it says so.
3. Quit on a test process (`sleep 1000` started by hand) shows the dialog,
   and ends it after confirmation; the same on a root process is refused
   with the reason.
4. The cost benchmark passes.

## Checklist

1. Resolve Unverified; write `procs.tsv` and the generator.
2. `crates/mirqab-core/src/samplers/apps/{mod,procfs,group,desktop,inspect}.rs`.
3. `crates/mirqab-core/src/samplers/apps/actions.rs` on `act.rs` and
   `plan.rs`.
4. `tests/golden/apps.json`, `tests/apps.rs`.
5. CLI `apps`, `what`, `quit`; MCP `top_apps`, `what_is_process`.
6. `crates/mirqab-view/src/tabs/apps.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/apps/{mod,page,group_row,process_row,inspector,top_card}.rs`.
7. Focus tracking: `crates/mirqab-core/src/desktop/hyprland.rs` (event
   socket reader, active only while `apps` has a subscriber).
8. Tests above.
9. Measure `--subscribe apps` and the top-apps minute cost; record both.

## Unverified

- Hyprland's event socket path and the `activewindow` event format:
  `hyprctl` could not reach the compositor from the planning shell. Check
  https://wiki.hypr.land/IPC/ and a live session; record a sample line as a
  fixture.
- `drm-memory-*` fdinfo keys on amdgpu: check a live process; if absent,
  drop per-process GPU memory from the inspector.
- Whether every launcher on the owner's desktop puts apps in
  `app-*.scope` units: check `systemd-cgls --user` and record which
  launchers do not (those fall to rule 5).
