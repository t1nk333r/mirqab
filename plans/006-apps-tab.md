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
- **State**: IN PROGRESS (part 1 of 4: sampler, inspector, read CLI and MCP)

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
| GPU memory per process | AMD: `/proc/<pid>/fdinfo/*` `drm-memory-vram`, `drm-memory-gtt`, `drm-memory-cpu`, one client's fds sharing a `drm-client-id` (https://docs.kernel.org/gpu/drm-usage-stats.html) **[machine]**, 2026-11-14 | own processes |

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

1. [x] Resolve Unverified (part 1: launcher scopes and DRM keys; the
   Hyprland event format waits for part 4); write `procs.tsv` and the
   generator.
2. [x] `crates/mirqab-core/src/samplers/apps/{mod,procfs,group,desktop,inspect}.rs`.
3. [ ] `crates/mirqab-core/src/samplers/apps/actions.rs` on `act.rs` and
   `plan.rs` (part 2).
4. [x] `tests/golden/apps.json`, `tests/apps.rs` (the read side; the
   actions' tests come with part 2).
5. [ ] CLI `apps`, `what`, `quit`; MCP `top_apps`, `what_is_process`
   (part 1 did `apps`, `what` and both MCP tools; `quit` is part 2).
6. [ ] `crates/mirqab-view/src/tabs/apps.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/apps/{mod,page,group_row,process_row,inspector,top_card}.rs` (part 3).
7. [ ] Focus tracking: `crates/mirqab-core/src/desktop/hyprland.rs` (event
   socket reader, active only while `apps` has a subscriber) (part 4).
8. [ ] Tests above (part 1 wrote the read side's).
9. [ ] Measure `--subscribe apps` and the top-apps minute cost; record both
   (part 4).

## As built (part 1)

Part 1 (2026-11-14) is checklist steps 1, 2, the read side of 4 and the
read commands of 5. Where it differs from the plan or adds to it:

- **Four pull requests.** Part 1: sampler, inspector, `apps` and `what`,
  `top_apps` and `what_is_process`. Part 2: Quit and Force quit,
  `mirqab quit`. Part 3: the view model and the page. Part 4: Hyprland
  focus, "idle but heavy", the benchmark and the top-apps minute event.
- **Per-subscriber views.** `apps` is measured once per tick and each
  subscriber receives it sorted, limited and expanded by its own `params`:
  `Sampler::view` (`crates/mirqab-core/src/sampler.rs:47`), applied per
  distinct params in `sampler_loop` (`crates/mirqab-core/src/server.rs:595`)
  and to `get`; `interval`, `live` and `stream` are taken out of the params
  first (`server.rs:1349`). The System group is always last and outside
  `limit` (`crates/mirqab-core/src/samplers/apps/mod.rs:254`). `expand`
  takes group ids or `true` (every group; `mirqab apps --all`).
- **Queries under a stream.** `get apps.inspect {pid}` is
  `Sampler::query` (`sampler.rs:54`): a `get` whose stream name has a dot
  goes to the stream before it (`server.rs:973`); an unknown query is
  `unknown-stream`, a missing pid `bad-request`, a gone process `failed`.
  `stream::APPS_INSPECT` names it; it is not in `STREAMS`.
- **"Ours" through Root.** The daemon's uid is read from
  `proc/self/status` through `Root` at probe (`mod.rs:308`), so a fixture
  says whose processes are the user's and CI's uid does not matter.
  Users and homes come from `etc/passwd`; an account missing there (a
  systemd-homed one) falls back to the daemon's `$HOME`.
- **Grouping** (`group.rs:257`). A Flatpak or container scope is checked
  before rule 1: such scopes sit under the user manager and are not app
  scopes. Rule 1 tries the unit's application id with and without a
  leading launcher segment (`unit_app_ids`, `group.rs:173`, after
  systemd's `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope` and
  `…[@<RANDOM>].service`) against the desktop entries, then the main
  process's name against `Exec=` and `StartupWMClass`; with no match a
  scope is named after its main process (the oldest member that is not a
  shell) and a plain service after its unit. `init.scope` (the user
  manager) is a service group named `systemd`. Rule 4 splits a terminal's
  scope by the sessions its shells lead (`unit_groups`, `group.rs:316`);
  a session's job is its foreground process group's leader, else its
  oldest process that is not a shell, and a script run by `node`,
  `python` and the like is named after the script (`vite`); a session of
  shells only stays with the terminal. Rule 5 climbs only through other
  rule-5 processes of the user (`tree_groups`, `group.rs:409`). Group ids:
  `scope:`, `service:`, `flatpak:`, `container:` (12 hex), `session:`,
  `tree:`, `system`.
- **Containers are named after their main process** (`postgres (container
  c0ffee00c0ff)`). The `docker ps` name lookup of the data-source table is
  left for later: it is a command per tick for a name the main process
  already gives.
- **CPU** is per pid and start time (`mod.rs:122`): a process new since
  the baseline, or a pid reused with another start time, counts 0 for its
  first interval instead of its whole lifetime. A baseline older than 10 s
  is dropped, as in `system`. `get` uses a subscriber's baseline when it is
  1–10 s old, else takes its own second reading 500 ms later (2 % of a
  core is the smallest step it can tell). `totals.cores` is there for the
  percent-of-the-machine toggle.
- **Memory** is the plan's estimate: every member's resident pages minus
  its shared pages, plus the largest shared part once. Storage rates
  (`ioReadPerSec`, `ioWritePerSec`) sum the members whose `io` the daemon
  may read; another user's group has none.
- **The cache** keys on pid and start time and also on the command name,
  so a process that exec'd is read again (`procfs.rs:260`); `status`,
  `cmdline`, `cgroup` and, for the user's processes no user unit claims,
  `root/.flatpak-info` are read once per process.
- **Desktop entries** come from a fixed list of directories under `Root`
  (the user's `~/.local/share` and Flatpak exports, then
  `var/lib/flatpak/exports`, `usr/local/share`, `usr/share`), not
  `$XDG_DATA_DIRS`, so a fixture reads the same everywhere; they are read
  again only when one of the directories changed. `Hidden=true` masks an
  id; localised names and action groups are skipped.
- **Inspector** (`inspect.rs:370`). The verdict's rules, in order: kernel
  thread, another user's process (limited to name, CPU and memory),
  replaced executable, Flatpak, container, packaged (with the repository
  from `pacman -Sddp --print-format %r`, or "no sync repository" for a
  local or AUR build), not owned and in the home directory, not owned,
  unknown (no pacman). The package is asked as `pacman -Qqo <path>`
  through `exec.rs`, cached per path (`inspect.rs:103`); a missing pacman
  is remembered and never asked again; a Flatpak or container path is
  never asked about. The verdict carries a `code` and the text from
  `strings.rs`. Listening ports wait for plan 007. Fields that cannot be
  read are left out of the JSON.
- **The ELF header is read only inside the tree.** `Root::read_prefix`
  (`crates/mirqab-core/src/root.rs:87`) refuses a link that leaves a
  fixture tree, so a fixture's `exe` link (a path of an imagined machine)
  is never followed onto the live one; on `/` it reads
  `proc/<pid>/exe` as the kernel gives it. `foreign` compares with
  `proc/self/exe`.
- **GPU memory** sums `drm-memory-vram`, `-gtt` and `-cpu` once per
  `drm-client-id` (`inspect.rs:280`); absent keys leave it out.
- **MCP.** `top_apps` (`sort`, `limit` 1–50, default 10) and
  `what_is_process` (`pid`) have input schemas; the tool table gained an
  `input` field. `what_is_process` leaves out the command line, the
  working directory and open files' paths, which can hold secrets, and
  keeps the counts (`crates/mirqab/src/mcp/tools.rs:250`). A bad argument
  is a tool error, so the agent can correct it.
- **CLI.** `mirqab apps [--sort cpu|mem|name] [--all] [--json]` and
  `mirqab what PID [--json]` live in `crates/mirqab/src/apps.rs`; the MCP
  tools print the same lines. Long names are cut with an ellipsis to keep
  the columns.
- **Fixtures.** `tests/fixtures/procs.tsv` (tab-separated) and
  `tools/mkfixture.sh procs` write 39 processes into `root-desktop` and
  their twins into `root-desktop.t1` (pid 7000 reused there), with
  `proc/self/ns`, `etc/passwd` and seven desktop entries beside them;
  `tests/test-fixtures.sh` checks the files are the table's. The home is
  `/users/user` so `tools/secrets.sh`'s home-path pattern stays useful.
  `tests/fixtures/bin/pacman` is the stub. `crates/mirqab-core/tests/system.rs`
  now copies links as links when it copies a fixture.
- **Tests.** `crates/mirqab-core/tests/apps.rs`: 10 tests (grouping rules,
  System, kernel threads, CPU and storage deltas, pid reuse, sort, limit
  and expand, the stream's golden `tests/golden/apps.json`, the
  inspector's golden for five processes `tests/golden/apps-inspect.json`,
  verdict rules, pacman once per path and absent, unreadable files, the
  ELF header inside the tree only); 10 unit tests in the new modules; one
  more in `protocol.rs` (per-subscriber views and the query errors) and
  in `crates/mirqab/tests/mcp.rs` (both tools, nothing private sent);
  `tests/test-cli.sh` (147 checks, 23 of them new) and the new
  `tests/test-fixtures.sh` (7 checks). `tests/run.sh --affected` selects
  8 of 15 files (the protocol and `Root` are shared infrastructure): 274
  checks pass.

## Unverified

- Hyprland's event socket path and the `activewindow` event format:
  `hyprctl` could not reach the compositor from the planning shell. Check
  https://wiki.hypr.land/IPC/ and a live session; record a sample line as a
  fixture. 2026-11-14: the socket
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`
  exists on the development machine **[machine]**; no event was captured
  yet. Left for part 4.
- ~~`drm-memory-*` fdinfo keys on amdgpu~~ Resolved 2026-11-14
  **[machine]**: amdgpu exposes `drm-memory-vram`, `drm-memory-gtt` and
  `drm-memory-cpu` in `/proc/<pid>/fdinfo/*`; one process holds several
  DRM fds sharing a `drm-client-id`, so the inspector counts each client
  once. Kept, in the inspector only; left out when absent.
- ~~Whether every launcher puts apps in `app-*.scope` units~~ Resolved
  2026-11-14 **[machine]**: apps launched from Hyprland binds land in
  `app-Hyprland-<command>-<8 hex>.scope` (an `app-Hyprland-zen.sh-….scope`
  was seen), after systemd's `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope`
  and `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service`
  (https://systemd.io/DESKTOP_ENVIRONMENTS/); a desktop-id scope looks
  like `app-com.example.App-12345.scope`, Flatpak's like
  `app-flatpak-<app id>-<n>.scope`; other tools make scopes of their own
  (`tool-xyz-<hex>.scope`) and plain `<name>.service` units under
  `app.slice`. An application id that is no desktop entry (`zen.sh`)
  names the group after the main process, not the launcher. The fixture
  carries a case of each; none of them falls to rule 5, which is left
  for processes outside the user manager (the session scope).
