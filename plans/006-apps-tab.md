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
- **State**: IN PROGRESS (parts 1 and 2 of 4: sampler, inspector, read CLI
  and MCP; Quit and Force quit, `mirqab quit`)

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

### Part 2: Quit and Force Quit (design)

Evidence in this section was read from `origin/plan-006-apps-part1` at
d880133 (file:line references are to that tree); **[machine]** was
measured on the planner's machine on 2026-11-14, **[unverified]** was
not checked. Where the build differs, "As built (part 2)" says so.

Part 2 is one pull request. It adds the first action in mirqab that ends
another program: `apps.quit` and `apps.kill` in the daemon, and
`mirqab quit` on the command line. The dialog and the row buttons are part 3.
MCP stays read only in this plan (line 143, "read only"; plan 005's tool
table lists `top_apps` and `what_is_process` as reads): part 2 adds no MCP
tool, and `crates/mirqab/tests/mcp.rs` gains one assertion that no tool named
`quit` or `kill*` is listed.

#### What exists, and what is missing

| Piece | Today | Evidence |
| --- | --- | --- |
| Sending a signal | `act::signal` runs `kill -s NAME -- PID` through `exec.rs`. It refuses pid ≤ 1, the daemon and another *real* uid. The target is a bare pid with no start time. | `act.rs:62-100`, `act.rs:47-54` |
| The other §5.4 refusals | Not written: "Later plans add the cgroup and compositor refusals". | `act.rs:56-61`, architecture §5.4 lines 398-419 |
| A wrapper for kill(2) | None. `sys.rs` holds `peer_uid`, `uid`, `listen_fd`, `stdout_closed`. | `sys.rs:12-85` |
| Crates | `libc 0.2` is a direct dependency. `rustix` 1.1.6 is in `Cargo.lock` only through other crates. `nix` is absent. | `crates/mirqab-core/Cargo.toml`, `Cargo.lock` |
| Plans | `Plans::make` and `redeem`, 120 s, single use, on `Instant`, no cap, written for "one MCP server". Nothing calls them. | `plan.rs:15`, `plan.rs:33-38`, `plan.rs:56-97` |
| Ancestry | `guard::ancestry` stops quietly at an unreadable `stat`; a test asserts `ancestry(77) == [77]`. | `guard.rs:12-23`, `guard.rs:83-88` |
| The `action` method | Knows `ui.show`, `ui.toggle`, `ui.hide`. Returns `{"wouldDo":[…]}` or `{"done":[…]}`. | `server.rs:1219-1248` |
| `confirm` | Not parsed anywhere. `dry_run_param` is the strict boolean to copy. | `server.rs:1365-1378` |
| Error replies | `ErrorBody { code, message }`: a `needs-confirm` reply cannot carry `wouldDo`. | `frame.rs:53-57` |
| The caller | The daemon reads the peer's uid and drops its pid. | `sys.rs:12-35`, `server.rs:646` |
| Action events | `History::add_event` exists; no action writes one. | `history.rs:285` |
| systemctl through `exec.rs` | `systemctl --user start mirqab-ui.service`. | `server.rs:1289` |
| The CLI confirm step | `confirm::ask` prints the list and asks `[y/N]`. | `crates/mirqab/src/confirm.rs`, strings.rs:609-612 |

#### Decisions

**1. The signal goes through a pidfd, from two wrappers in `sys.rs`.**
`sys::pidfd_open(pid) -> io::Result<OwnedFd>` and
`sys::pidfd_signal(fd, signo) -> io::Result<()>` call
`libc::syscall(libc::SYS_pidfd_open, …)` and
`libc::syscall(libc::SYS_pidfd_send_signal, …)`. They are the only new
`unsafe`, about 20 lines, on the dependency every crate already has
(architecture §13). `act.rs` stops running `kill(1)`.

The order in `act::signal` is fixed, because it is what closes the pid-reuse
race:

1. open the pidfd;
2. read `proc/<pid>/stat` and `proc/<pid>/status` and `proc/<pid>/cgroup`
   through `Root`, and run every refusal on what was just read;
3. compare field 22 (start time) with the planned start time;
4. send the signal through the fd.

The planned process existed before step 1. If the start time still matches
after the fd is open, the fd refers to that process, and the fd cannot be
re-pointed. A check followed by `kill(pid)` leaves a window; this leaves
none.

Other properties:

- `pidfd_open` on a thread id that is not a thread-group leader fails with
  `EINVAL`: a second guard for D3.
- `ENOSYS` (a kernel before 5.3) is `failed` with a message; there is no
  fallback to `kill(pid)`. **[machine]** the planner's kernel is 7.2.
- `ESRCH` at step 1 or 4 means the process is gone: reported under `gone`,
  not as a failure.
- The names avoid the lint's mutation pattern
  (`\bkill\(` outside `act.rs`/`exec.rs`, `tools/lint.sh`).
- `act::signal` takes `Target { pid, start }` in place of a pid. Its one
  caller-facing change; nothing else calls it today.

**2. Nothing is signalled unless the root is the host.** Tests start
`mirqab daemon --root <fixture>` (`tests/lib.sh:141`), and a fixture pid such
as 2001 can be a real process on the machine running the tests. So
`act.rs` signals through a `Sender` trait with two implementations:
`Real` (decision 1) and `Recording` (appends `SIG pid start` lines to a
list, or to the file named by `MIRQAB_ACT_LOG` when `MIRQAB_TEST=1`, the
gate `server.rs:1337-1347` already uses). The daemon picks `Real` only when
`Root::is_host()` (new: the base is `/`); on any other root it picks
`Recording`, whatever the environment says. `systemctl` needs no such rule:
the stub `PATH` covers it (`tests/lib.sh:75`).

**3. Two calls, always.** Architecture §5.4 says a destructive action
without `confirm: true` replies `needs-confirm` with the same `wouldDo`.
Plan 005 rule 1 says the first call never acts, and rejects "letting
`confirm: true` on the first call act" (005:139). Both hold for the daemon
action:

| Request | Reply |
| --- | --- |
| `dryRun: true` | `{"wouldDo":[…],"skipped":[…]}`; no plan is made |
| no `confirm`, or `confirm: true` without `plan` | error `needs-confirm`, with `data: {"plan":"plan-…","wouldDo":[…],"skipped":[…]}` |
| `confirm: true` and `plan` | redeem, act, `{"done":[…],"gone":[…],"failed":[…],"skipped":[…]}` |
| `confirm` that is not a boolean, or `false` with a `plan` | `bad-request` |

- `ErrorBody` gains an optional `data` field, omitted when empty. A new
  field does not bump the protocol (architecture §5.2).
- `args` is `{"pid": N}` or `{"group": "<id>"}`, exactly one; anything else
  in `args` is `bad-request`.
- A `wouldDo` entry is `{"signal":"TERM","pid":2001,"start":5010,"name":"browser"}`
  or `{"run":["systemctl","--user","stop","--no-block","--","example.service"],"unit":"…","pids":[…]}`.
  It lists every pid. It never holds a command line (repository rule).
- A plan is bound to the action name, so a plan made by `apps.quit` cannot
  be redeemed by `apps.kill` (`PlanError::OtherTool`, `plan.rs`).
- A plan is not bound to a connection: the CLI makes one connection per
  call (`cli.rs:22-53`). The id is the secret; ids come from `RandomState`
  (`plan.rs:56-73`).
- So `-y` on the command line still costs two calls. That is intended.

**4. Targets are re-computed at the second call, and the lock is released
before anything is signalled.** At redeem the daemon resolves the same pid
or group again, with fresh reads, and acts on planned ∩ current, where a
target is `{pid, start}`. A process that joined the group after the prompt
is not signalled, and is listed under `skipped` with the reason "started
after the confirmation". Resolution holds the sampler lock; signalling and
`systemctl` run after it is released (the lesson of D1).

Uid and cgroup are read again for the action. The sampler caches both once
per process (`procfs.rs`, `Info`), and a cached uid must not decide a
refusal.

**5. Refusals.** Each is checked when the plan is made and again inside
`act::signal` just before the signal. A refused single pid is error
`refused` with the reason. In a group, refused members are left out and
listed under `skipped` with their reasons; the action is `refused` only
when nothing is left to act on.

| Refused | Rule | Source |
| --- | --- | --- |
| pid ≤ 1 | as today | §5.4; `act.rs:69` |
| the daemon | `std::process::id()`, and the pid in `proc/self/stat` read through `Root` (4321 in the fixture) | §5.4; `act.rs:72` |
| another user's process, uid 0 included | all four numbers of `Uid:` must be the user's. Today only the real uid is read, so a setuid-root program passes. | §5.4, §8.2; `act.rs:47-54` |
| a kernel thread | `PF_KTHREAD` in `stat` flags | follows from uid 0; stated for the message |
| outside the user's cgroup subtree | the cgroup path must start with `/user.slice/user-<uid>.slice/` | §5.4 |
| the user manager | any process in `user@<uid>.service/init.scope` | **new**: ending it ends the session |
| the compositor and its ancestors | decision 6 | §5.4 |
| the caller and its ancestors | decision 7 | plan 005 rule 4 |
| a unit that holds a refused pid | the unit is not stopped | follows from the rows above |

**The interface is not refused.** `mirqab-ui` is a restartable service that
`mirqab close` already ends, and §5.4 does not list it. Part 3 must make the
row for mirqab itself say what Quit does.

**6. The compositor.** Two sources, joined:

- On the host: connect to each `wayland-*` socket in the runtime directory
  and read the listener's pid with `SO_PEERCRED` (`sys::peer_pid`, beside
  `peer_uid`, `sys.rs:12-35`). **[machine]** on the planner's session the
  peer of `wayland-1` is `Hyprland`. Read at plan time and again at redeem;
  no polling.
- On any root: a process of the user whose `comm` is in a fixed list
  (`Hyprland`, `sway`, `niri`, `river`, `labwc`, `wayfire`, `kwin_wayland`,
  `gnome-shell`, `cosmic-comp`, `weston`). This is what a fixture can test
  (pid 1050 in `procs.tsv`), and it covers a host where the socket probe
  fails.

Ancestors come from a strict walk (decision 9). If neither source finds a
compositor, the action proceeds: a server or a tty session has none.

**7. The caller's own ancestry is refused.** `sys::peer_pid` on the
accepted connection gives the caller. The daemon walks its ancestry
strictly and refuses those pids. This gives plan 005's rule 4 to the
command line now (an agent that runs `mirqab quit` from a shell is the same
case as an MCP server), and plan 007 reuses it for `kill_port`.

Under a fixture root the real caller has no `proc/<pid>/stat`, and a strict
walk would refuse everything. When `MIRQAB_TEST=1`, `MIRQAB_CALLER_PID`
replaces the peer pid (the `server.rs:1337-1347` pattern). Tests set it to a
fixture pid, which also makes the rule testable: with the caller at 3060,
`quit 3000` (its terminal) is refused.

**8. Groups: leaders first.** The order is: the group's main process
(`group.rs:266-274`), then the other session and process-group leaders, then
the rest; oldest first inside each class. Each target gets its own pidfd and
re-check. The call does not wait for exits: it reports `done` for a signal
delivered, and the stream shows the result.

**9. Thread ids, containers, services.**

- **D3.** A pid whose `Tgid:` differs is a thread. The action resolves it to
  the thread-group id and plans that; `wouldDo` shows the tgid and
  `"via": <tid>`. The start time recorded is the leader's.
- **D4.** A `container:` group with any member on another uid is refused
  whole, with the reason and the command to use
  (`podman stop <id>` / `docker stop <id>`, the 12-hex id from
  `group.rs:134`). mirqab does not run the runtime in part 2. A container
  group entirely on the user's uid is signalled like any group.
  *Rejected:* running `podman stop` through `exec.rs` now. Plan 007 lists
  `docker stop` under `ports.stop` (007:103-108) and will add the stub and
  the fixture for it; one place should own that.
- **Services.** For a `service:` group, or a pid whose cgroup is a
  `.service` under the user manager, the action is
  `systemctl --user stop --no-block -- <unit>` through `exec.rs`, 5 s
  timeout. The unit name comes from the cgroup path (`group.rs:110-120`) and
  must match `[A-Za-z0-9:_.@\\-]+\.service`; otherwise `refused`.
  `--no-block` because a plain `stop` can wait `TimeoutStopSec` (90 s by
  default), longer than the CLI's 10 s read timeout (`cli.rs:24`).
  **[machine]** a transient unit with `Restart=always` came back after a
  plain SIGTERM to its main pid, and was `inactive` with no main pid 2 s
  after `stop --no-block`.
  `apps.kill` on a service runs the same stop, then
  `systemctl --user kill --signal=KILL -- <unit>` **[unverified]**.
  A `.scope` is signalled by pid: a scope does not restart.

**10. Every act is recorded.** One `event` row per redeemed plan, kind
`action`, through `history.rs:285`:
`{"action":"apps.quit","target":{"group":"…"}|{"pid":N},"caller":{"pid":N,"name":"zsh"},"results":[{"pid":N,"name":"…","signal":"TERM","result":"done|gone|failed|skipped"}],"unit":"…"}`.
Names are `comm`; no command lines. A dry run and a `needs-confirm` reply
write nothing. With no history file (`Options.history` is `None`) the action
still runs.

**11. The command line.**
`mirqab quit <pid|name> [-y] [--force] [--dry-run] [--json]`.

- A number is a pid. Anything else is a group name, matched whole and
  case-insensitively against the `apps` view. No match: exit 1. Several
  matches: print them with their ids and pids, exit 1; never guess.
- `--force` selects `apps.kill`.
- Flow: call without `confirm`; print the `wouldDo` and `skipped` lines;
  `confirm::ask`; call again with the plan. `--dry-run` sends `dryRun` and
  exits 0. No terminal and no `-y`: exit 3 (005:52-59).
- Exit 0 when every target is `done` or `gone`; 1 on `refused`, or when any
  target `failed`.
- `CallError::Daemon` gains the reply's `data` (`cli.rs:16-19`, `:47-49`).
- A new `CLI_COMMANDS` entry (strings.rs:19) drives help, `docs/cli.md` and
  the completions. Every printed line is a function in `strings.rs`, printed
  through `out.rs`. Add `quit` to the list in architecture §11, and the
  `needs-confirm` `data` and the new refusals to §5.2 and §5.4.
- Code: `crates/mirqab/src/quit.rs`, called from the match at
  `main.rs:104-135`.

#### What moves in from plan 007 step 6

Part 2 is the first destructive action, so everything that guards *the act*
lands here. What guards *the MCP surface* stays in plan 007.

| Item of 007 step 6 (007:202-221) | Where | Reason |
| --- | --- | --- |
| Targets as `{pid, starttime}`, compared again at redeem and again before the signal | **part 2** | pid reuse is a risk for any signal, whoever asks (decisions 1, 4) |
| Rule 2: the daemon re-computes targets at `confirm: true` | **part 2** | the gap between prompt and act is the same on the command line |
| Rule 5: every act written to the `event` table | **part 2** | the first act is here; kind `action`, with the caller. Plan 007 adds `"via":"mcp"` |
| Acceptance 3: the confirm flow end to end with a recording act | **part 2** | for the daemon and the CLI. Plan 007 repeats it through MCP |
| `confirm` a strict boolean that must be `true` | **part 2** (daemon side) | the daemon parses it first here |
| A cap on live plans | **part 2** | plans now live in the daemon and any client can make them: 64, the oldest dropped |
| The ancestry walk failing closed | **part 2** | decisions 6 and 7 depend on it. `guard::ancestry_strict -> Result`; an unreadable `stat`, a loop or depth 64 is an error and the action is `refused`. The `ancestry(77) == [77]` test becomes an error case |
| Plan expiry on `CLOCK_BOOTTIME` | **part 2** | `plan.rs` changes signature here anyway. A plan made before a suspend must not be live after it. `sys::boottime() -> Duration` (`clock_gettime`); `make` and `redeem` take a `Duration` |
| D3 thread id → tgid; D4 sub-uid containers | **part 2** | deferred to this step by the review of PR #16 (decision 9) |
| A `Ctx` for tool handlers | plan 007 | MCP plumbing; no tool acts in plan 006 |
| Per-tool `input_schema`, strict tool arguments | plan 007 | MCP only. The daemon action is strict in part 2 (decision 3) |
| Descriptions lead with the side effect; `destructiveHint` test | plan 007 | MCP only |
| Rule 4 for the MCP server's own ancestry, and `kill_port` | plan 007 | the mechanism lands here (decision 7); the MCP wiring does not |

Plan 007's step 6 should be edited in the same PR to say which lines part 2
took.

#### Rejected

- **Keeping `kill(1)` through `exec.rs`.** A bare pid, a fork per target,
  and no way to bind the start-time check to the signal.
- **`libc::kill` in `sys.rs`.** The same race between check and call.
- **Adding `rustix` or `nix` as a direct dependency.** Two syscalls do not
  earn a dependency line in architecture §13.
- **Returning the plan in a success reply.** §5.4 names the code
  `needs-confirm`; a client that ignores the code must not read success.
- **Acting on `confirm: true` at the first call.** Plan 005, line 139.
- **`systemctl --user kill` alone for Force Quit of a service.** A unit with
  `Restart=` set comes back.
- **Waiting for the processes to exit.** It ties a daemon thread to a
  program that ignores SIGTERM. Part 3 can offer Force Quit when the row is
  still there.
- **Refusing the whole group when one member is refused.** The session
  scope always holds the compositor; nothing in it could be quit.
- **Binding a plan to its connection.** The CLI and MCP connect per call.

#### Tests

No test signals a process it did not start.

| Test | Where | Checks |
| --- | --- | --- |
| Refusals | `crates/mirqab-core/tests/actions.rs`, recording sender, `root-desktop` | pid 1; 410 (uid 0); 600 (uid 81); 2 (kernel thread); 4321 (the daemon); 1100 (user manager); 1050 (compositor) and 900 (its parent); a new setuid row (real uid 1000, effective 0); the caller's ancestors with the caller at 3060. Each gives `refused` and its reason, and the recording is empty |
| `wouldDo` | same | the browser scope lists 2001 first, then 2000, 2010, 2011, 2012, each with its start time; a dry run makes no plan |
| Two-step | same | no `confirm` → `needs-confirm` with a plan; `confirm: true` without a plan → the same; redeem signals exactly the planned set; a second redeem is `bad-request`; a quit plan redeemed as kill is refused |
| Re-compute | same, plan on `root-desktop`, redeem on `.t1` | pid 7000 has a new start time in `.t1` (`procs.tsv`): it is skipped, not signalled |
| Session group | same | 1050 is under `skipped`; the others are signalled |
| Containers | same | the group of 5000 and 5001 (uid 100998) is refused with the `podman stop c0ffee00c0ff` hint |
| Thread id | same, scratch copy of the fixture | a `status` with another `Tgid` plans the tgid |
| Service | same, stub `tests/fixtures/bin/systemctl` that logs its argv | `service:` of 1300 runs `stop --no-block -- <unit>`; kill adds the `kill --signal=KILL` line; an odd unit name is refused; a dry run logs nothing |
| Event | same, with a history file | one `action` row per redeem; none for a dry run; no command line in it |
| Plans | unit tests in `plan.rs` | boot-time expiry; the cap drops the oldest; `{pid,start}` intersection |
| Strict walk | unit tests in `guard.rs` | unreadable `stat`, loop and depth are errors |
| Fixture root never signals | `actions.rs` | a daemon on a fixture root picks the recording sender with `MIRQAB_TEST` unset |
| A real signal | `crates/mirqab-core/tests/signal.rs` | the test spawns `sleep 1000`, calls the real path with `Root::host()`, and `wait()` reports SIGTERM; a second child with a wrong start time is not signalled and is then killed by the test through `Child::kill`. Under `crates/*/tests`, outside the lint's glob |
| CLI | `tests/test-cli.sh`, `start_daemon root-desktop`, `MIRQAB_ACT_LOG` | `quit 2001 --dry-run` exits 0 and logs nothing; `quit 2001` with no terminal exits 3; `quit 2001 -y` logs `TERM 2001 5010`; `--force` logs `KILL`; `quit 410` and `quit 1` exit 1 with the reason; `quit "Example Browser" -y` logs five lines, 2001 first; an unknown name exits 1; `quit --help` |
| MCP stays read only | `crates/mirqab/tests/mcp.rs` | no acting tool is listed |

`act.rs`, `server.rs` and `mirqab-proto` are shared infrastructure: the
affected selection widens to every `# mirqab:covers-shared` test. New test
files declare `# mirqab:covers`. Add the new rows to `procs.tsv` and
regenerate with `tools/mkfixture.sh procs`; the `apps` goldens change with
them.

#### Acceptance (part 2's share of acceptance 3)

The dialog is part 3. In part 2 the owner runs, on their own session:

1. `sleep 1000 &`, then `mirqab quit <pid>`: it prints the pid and `sleep`,
   asks, and after `y` the shell reports the job terminated.
2. The same with `--dry-run`: the job is still there.
3. `mirqab quit 1`, and `mirqab quit <pid of a root process>`: each prints
   the reason and exits 1.
4. `mirqab history` (or the `event` query) shows one `action` row, for
   step 1 only.

#### Unverified (part 2)


- `systemctl --user kill --signal=KILL` after `stop --no-block` leaves a
  `Restart=always` unit stopped. Only the `stop` half was measured.
- The compositor name list beyond Hyprland. Only Hyprland's socket was
  probed.
- That a daemon started by socket activation can connect to the
  `wayland-*` socket of every session type (a sandboxed unit could not).
  The name list is the fallback.
- That `proc/<tid>/status` is readable through `Root` for a thread id on
  the host, as the inspector's D3 path assumes.
- How `group.rs` places pid 900 (`login`, uid 0) in the session scope; the
  session-group test should be written against what the sampler returns.

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
3. [x] `crates/mirqab-core/src/samplers/apps/actions.rs` on `act.rs` and
   `plan.rs` (part 2).
   - Deferred from the review of PR #16 (D3): the inspector accepts a
     thread id; `actions.rs` resolves the target to its thread-group id
     and re-checks the start time before a signal.
   - Deferred from the review of PR #16 (D4): a rootless container's
     processes on a sub-uid cannot be signalled by the user; Quit on such
     a group goes through the runtime or reports why it cannot.
   - [x] 3a. `sys.rs`: `pidfd_open`, `pidfd_signal`, `peer_pid`,
     `boottime`. `Root::is_host`.
   - [x] 3b. `act.rs`: `Target`, the `Sender` trait with `Real` and
     `Recording`, the refusals of decision 5, the order of decision 1.
     The `kill(1)` call goes.
   - [x] 3c. `guard.rs`: `ancestry_strict`. `plan.rs`: boot-time clock,
     cap of 64, `{pid,start}` targets.
   - [x] 3d. `samplers/apps/actions.rs`: resolve a pid or a group to
     targets (D3, D4, services, leaders first, the compositor), with
     fresh reads.
   - [x] 3e. `frame.rs`: `ErrorBody.data`. `server.rs`: `apps.quit` and
     `apps.kill` in `action`, strict `confirm`, the plan store in
     `Inner`, the caller's pid kept per connection, the `action` event.
   - [x] 3f. CLI: `quit.rs`, the strings, `docs/cli.md`, completions.
   - [x] 3g. Fixture rows, the stub `systemctl`, the tests above (no
     stub: see "As built (part 2)").
   - [x] 3h. Architecture §5.2, §5.4, §11; plan 007 step 6 trimmed; this
     plan's As built; the owner's acceptance steps (listed, not yet run
     by the owner).
4. [x] `tests/golden/apps.json`, `tests/apps.rs` (the read side; the
   actions' tests come with part 2).
5. [x] CLI `apps`, `what`, `quit`; MCP `top_apps`, `what_is_process`
   (part 1 did `apps`, `what` and both MCP tools; part 2 `quit`).
6. [ ] `crates/mirqab-view/src/tabs/apps.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/apps/{mod,page,group_row,process_row,inspector,top_card}.rs` (part 3).
   - Deferred from the review of PR #16 (D5): session-scope group ids
     (`session:<sid>`) can be reused after a logout; the view model's row
     keys must not rely on them alone across frames.
7. [ ] Focus tracking: `crates/mirqab-core/src/desktop/hyprland.rs` (event
   socket reader, active only while `apps` has a subscriber) (part 4).
8. [ ] Tests above (part 1 wrote the read side's).
   - Deferred from the review of PR #16 (D6): views on a re-subscribe
     with new params, their cleanup on unsubscribe, and two subscribers
     that differ only in interval; written with part 3, where the page
     changes `sort` and `expand` live.
   - Deferred from the review of PR #16 (D7): `root/.flatpak-info` is read
     with no size cap or file-type check (a FIFO there would stall the
     walk); read it only when it is a regular file, through
     `read_prefix`, with a test.
9. [ ] Measure `--subscribe apps` and the top-apps minute cost; record both
   (part 4).
   - Deferred from the review of PR #16 (D1): `get apps` holds the sampler
     for its 0.5 s second reading, and `apps.inspect` holds it across
     pacman, so a subscriber's frame comes late; measure it, then move the
     sleep and the pacman call outside the lock.
   - Deferred from the review of PR #16 (D2): the inspector's GPU memory
     reads every `fdinfo` of the process; measured with the inspector's
     cost.

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

From the review of PR #16 (2026-11-14; the ruling kept 11 items,
deferred 7 to the checklist steps above and dropped 13):

- **Unit names are unescaped.** `group::unit_unescape`
  (`crates/mirqab-core/src/samplers/apps/group.rs:161`) decodes `\xNN` in
  the ids `flatpak_scope` and `unit_app_ids` take from a unit name, after
  splitting on the name's own dashes; the unit name and the group id stay
  escaped, as systemd shows them. A malformed escape stays as written.
- **`Exec=` lookup names only real apps.** A `NoDisplay=true` entry is
  found by its id but gives no `by_exec` key, and an `Exec=` whose program
  is a shell, an interpreter or a launcher gives none: `LAUNCHERS`
  (`desktop.rs:63`: `xdg-open`, `wine`, `java`, `electron`, `flatpak`,
  `snap`, `steam`, `sh`, `env`), and by rule (`launches`) any `…-launch`
  program or one whose first argument is `launch` or `open`. The ruling
  listed the toolkit's two launchers by name; `tools/lint.sh` forbids the
  toolkit's names anywhere in `mirqab-core` (architecture §3), so the rule
  catches them instead.
- **Names that are not UTF-8.** `stat`, `status` and `cgroup` are read as
  bytes and decoded lossily (`procfs::read_text`, `procfs.rs:236`), in the
  walk and in the inspector, so a process naming itself with arbitrary
  bytes is listed (U+FFFD in its name) and inspected as the user's.
- **Printed text is plain.** `strings::plain` (`strings.rs:1270`) turns
  every control character and the bidirectional overrides into U+FFFD; it
  is applied in `fit`, `what_line`, `proc_ref`, `apps_container`, and in
  `what_lines` to the verdict, the working directory, the command line
  and open files' paths. `mirqab what` now prints those paths (`open`
  lines). The stream and `--json` stay raw: JSON escapes.
- **`what_is_process` is an allow-list** (`inspect_summary`,
  `crates/mirqab/src/mcp/tools.rs:246`): named fields only, absent ones
  as null; no `cgroup`, command line, working directory or path.
  `MCP_WHAT` says what is left out.
- **Samples are stamped after they get the sampler.** `sampler_loop`
  keeps its own `now` for the schedule and passes `Instant::now()` taken
  under the sampler's lock (`crates/mirqab-core/src/server.rs:590`); the
  `apps` snapshot stamps both readings when they were taken (`mod.rs:371`).
  A tick that waited behind a `get` no longer reads up to 25 % high.
- **Rootless containers keep their sub-uid processes.** A container under
  `user@<own uid>.service` is the user's container whatever uid its
  process runs as (`base`, `group.rs:235`); a foreign uid anywhere else
  is still System. Fixture pid 5001 runs as uid 100998.
- **The contract.** `docs/architecture.md` §5.2 describes stream params,
  per-subscriber views and `get <stream>.<query>`; §5.3 describes
  `Sampler::view`, `Sampler::query` and the `apps` params.
- **A cut list says so.** `totals.groups` counts the groups before the
  limit (`stream.rs:309`, set at `mod.rs:212`); `mirqab apps` ends with
  "and N more" when the list was cut, and `top_apps` sends the count.
- **A process is read again on its second sighting.** The cache marks an
  entry settled after one more `read_info` (`procfs.rs:279`), so a process
  a launcher moved into its scope just after its first sample is
  regrouped; later moves are not followed.
- **An answer the CLI or a tool cannot read is an error**, not an empty
  list: `parse_apps` and `parse_inspect` (`crates/mirqab/src/apps.rs:16`)
  give `bad_daemon_answer`, exit 1 in the CLI and a tool error in MCP.
- **Fixtures.** `procs.tsv` gained a third shell running a bare
  `python3`, a `python3` script as a scope's main process, an escaped
  Hyprland scope and an escaped Flatpak scope, and pid 7200 reused with
  the same name and a new start time: 45 processes in `root-desktop`, 45
  in `.t1`. Four desktop entries were added (`two-words`, a Flatpak
  `org.example.Two-Words`, a `NoDisplay` URL handler running the browser,
  and an `Exec=python3` app). Golden changed: `tests/golden/apps.json`
  (`totals.groups`, the new groups); `tests/golden/apps-inspect.json` is
  unchanged.
- **Tests.** `crates/mirqab-core/tests/apps.rs`: 12 tests (new
  `a_name_that_is_not_utf8_is_still_listed` and
  `a_process_moved_after_its_first_sample_is_regrouped`; the grouping,
  reuse and view tests assert the new cases); `protocol.rs`: 17 (new
  `a_sample_is_stamped_after_it_got_the_sampler`, which fails with the
  old stamp); `crates/mirqab/tests/mcp.rs` compares the exact field sets
  of both tools; unit tests: `mirqab-core` 41, `mirqab` 16 (parse, plain
  rows and the "more" line, the tool's parse), and one more in
  `mirqab-proto` (`plain`).

## As built (part 2)

Part 2 (2026-11-14) is checklist step 3 (3a–3h) and `quit` of step 5, as
designed in "Part 2: Quit and Force Quit (design)" above. What was built,
with where it lives:

- **The signal.** `sys::pidfd_open` and `sys::pidfd_signal`
  (`crates/mirqab-core/src/sys.rs:130`, `:147`) are the only new
  `unsafe`, beside `sys::peer_pid` (`:22`, `SO_PEERCRED` shared with
  `peer_uid`) and `sys::boottime` (`:107`). `act::signal`
  (`crates/mirqab-core/src/act.rs:294`) takes a `Target { pid, start }`
  (`:67`) and a `&dyn Sender` (`:76`); `kill(1)` is gone. ESRCH at open,
  at the start-time check or at send is `ActError::Gone`, ENOSYS a
  failure with `strings::ACT_NO_PIDFD`, with no fallback.
- **Two senders.** `act::Real` signals through the pidfd and runs
  commands through `exec.rs`; `act::Recording` (`act.rs:114`) writes
  `TERM 2001 5010` and `RUN systemctl …` lines to memory and, when
  `MIRQAB_TEST=1`, to `MIRQAB_ACT_LOG`. `act::sender_for` (`act.rs:180`)
  returns `Real` only when `Root::is_host()`
  (`crates/mirqab-core/src/root.rs:30`).
- **Refusals** (`act::refusal`, `act.rs:228`), checked when the plan is
  made and again inside `act::signal`: pid ≤ 1; the daemon
  (`std::process::id()` and `proc/self/stat` under the root); a kernel
  thread (`PF_KTHREAD`, before the uid so the message says so); any of the
  four `Uid:` numbers not the user's; a cgroup outside
  `/user.slice/user-<uid>.slice/`; `user@<uid>.service/init.scope`; and
  the guard's pids: the compositor and its parents, then the caller and
  its parents (`make_guard`,
  `crates/mirqab-core/src/samplers/apps/actions.rs:364`). Reasons are
  `strings::refuse_*`.
- **The compositor** is every process of the user whose `comm` is in
  `actions::COMPOSITORS` (`actions.rs:52`) plus, on the live machine only,
  the `SO_PEERCRED` pid of each `wayland-*` socket in `$XDG_RUNTIME_DIR`
  (`compositor_sockets`, `actions.rs:333`), read per resolution.
- **The caller** is the peer's pid, kept on the `Client`
  (`crates/mirqab-core/src/server.rs:661`); `MIRQAB_CALLER_PID` replaces
  it when `MIRQAB_TEST=1` (`test_caller`, `server.rs:1386`). An
  unidentified caller (`REFUSE_NO_CALLER`) and a strict walk that fails
  refuse the action, dry run included.
- **The strict walk.** `guard::ancestry_strict`
  (`crates/mirqab-core/src/guard.rs:37`) returns `WalkError::Unreadable`,
  `Loop` or `TooDeep`; the old quiet `ancestry` had no caller and is gone.
- **Plans** (`crates/mirqab-core/src/plan.rs`): `make` and `redeem`
  (`:98`, `:140`) take the boot-time clock; at most `MAX_PLANS` = 64,
  the oldest dropped; targets are `act::Target`; `redeem` returns the
  planned targets still there (`kept`) and the rest (`gone`).
- **Resolution** (`resolve`, `actions.rs:491`). A pid: its `Tgid:` first
  (D3: the thread group is planned, `"via"` names the thread id asked
  for, the start time is the leader's); a process in a `.service` under
  the user manager is that unit (`unit_step`, `actions.rs:447`); anything
  else is one signal. A group: by id from `group::group` on the fresh
  walk; a `container:` group with a member on another uid is refused
  whole with the runtime's stop command (D4,
  `group::container_runtime`, `group.rs:160`); a `service:` group with a
  `.service` unit is stopped; any other group is signalled in the order
  of decision 8 (`ordered`, `actions.rs:428`, on `group::main_process`,
  now public), its refused members under `skipped`, refused only when
  nothing is left (with the `skipped` list in the error's `data`).
- **Services.** `systemctl --user stop --no-block -- <unit>`, and for
  `apps.kill` then `systemctl --user kill --signal=KILL -- <unit>`
  (`unit_commands`, `actions.rs:281`), through the sender, 5 s each; the
  unit name must match `[A-Za-z0-9:_.@\-]+\.service`. A failed `stop`
  skips the `kill`.
- **The protocol.** `ErrorBody.data` and `Reply::err_data`
  (`crates/mirqab-proto/src/frame.rs:61`, `:105`); `stream::APPS_QUIT`,
  `stream::APPS_KILL`. `actions::parse` (`actions.rs:164`) is strict:
  `args` exactly one of `pid` and `group`, `dryRun` and `confirm`
  booleans, `plan` a string, `confirm: false` with a plan a bad request.
  The server calls `Actions::handle` (`actions.rs:628`) from
  `apps_action` (`server.rs:1267`) with no lock of its own held; the
  plans' lock is held only inside `make` and `redeem`.
- **The event.** Each redeemed plan writes one `event` row of kind
  `action` (`actions.rs:814`): `action`, `target`, `caller` (pid and
  `comm`), `results` (pid, name, signal, `done|gone|failed|skipped`),
  and `unit` for a service. A dry run and a `needs-confirm` reply write
  nothing; without a history file the action still runs.
- **The CLI.** `crates/mirqab/src/quit.rs`: a number is a pid, anything
  else a name matched whole and ignoring case against `get apps` with no
  limit (`subject`, `quit.rs:115`); several matches are printed with
  their ids and pids and exit 1. The plan is printed under "This will
  quit:" or "This will force quit:" (`confirm::ask_with`,
  `crates/mirqab/src/confirm.rs:23`); a declined or unanswerable prompt
  exits 3, as in `mcp-config`. Exit 1 on `refused` or when any target
  failed. `CallError::Daemon` carries the reply's `data`
  (`crates/mirqab/src/cli.rs:20`). The `quit` entry of `CLI_COMMANDS`
  makes the help, `docs/cli.md` and the completions; `cli_positionals`
  now skips placeholders with capitals, so `PID|NAME` is not offered as
  two words to complete.
- **Fixtures.** `procs.tsv` gained pid 6300, `suidtool`, real uid 1000
  and effective 0 (`uids=1000:0:0:1000`, a new extra in
  `tools/mkfixture.sh`): 46 processes in each tree. Golden changed:
  `tests/golden/apps.json` (one more group; totals 24 groups, 44
  processes); `apps-inspect.json` is unchanged.
- **Tests.** `crates/mirqab-core/tests/actions.rs`: 11 tests (every
  refusal of the brief's list with its reason, and the recording empty;
  a caller that cannot be walked; `wouldDo` 2001, 2000, 2010, 2011, 2012
  with start times and no plan on a dry run; two calls, a second redeem
  `bad-request`, a quit plan redeemed as kill or for another pid
  `refused`; pid 7000 re-computed on `.t1`; the session tree with 1050
  skipped; the container with `podman stop c0ffee00c0ff`; a thread id;
  the service's `stop` and `kill` lines and an odd unit name; one
  `action` event per redeem, none for a dry run or a plan, no command
  line; a fixture root never gets `Real`).
  `crates/mirqab-core/tests/signal.rs`: the one real signal, to a
  `sleep` the test started, through `Root::host()`; `wait()` reports
  SIGTERM; a second child with a wrong start time is `Gone` and still
  running, then ended by the test. `protocol.rs` (18): strict arguments
  over the socket and a refusal without a walkable caller.
  `crates/mirqab/tests/mcp.rs`: no tool named `quit`, `kill*` or
  `*quit*` is listed. Unit tests: `mirqab-core` 47 (refusals and the
  recording sender in `act.rs`, the strict walk's three errors, plans'
  boot-time expiry, cap, subject binding and intersection, strict
  arguments and unit names), `mirqab` 17 (plan and result lines are one
  plain line each). `tests/test-cli.sh`: 182 checks, 35 of them new
  (dry run sends nothing, no terminal exits 3, `-y` records
  `TERM 2001 5010`, `--force` records `KILL 7100 34000`, 410, 1 and the
  caller's terminal 3000 refused with their reasons, a name records five
  lines with 2001 first, an unknown and an ambiguous name exit 1, a
  group's compositor listed as left alone, a user service stopped
  through systemctl, `--help`).

Deviations from the design, each with its reason:

- **Resolution holds no sampler lock.** The design had it hold the
  `apps` sampler's lock. It walks `proc/` with an empty cache of its own
  instead: decision 4 wants uid and cgroup read again anyway, group ids
  do not depend on desktop entries, and nothing the sampler holds is
  needed, so a subscriber's frame never waits on an action.
- **No stub `systemctl`.** On a fixture root the recording sender also
  records the `systemctl` commands and never runs them, so
  `tests/fixtures/bin/systemctl` was not written. The in-process tests
  (`actions.rs`) have no stub `PATH`, and a real `systemctl --user stop`
  would reach the developer's session; one rule, "a fixture root never
  acts", now covers signals and commands. The `exec.rs` path for
  `systemctl` is `act::Real::run`, a direct call of `exec::run`.
- **Start time before refusals.** `act::signal` compares the start time
  (step 3) before the refusals (step 2). Both stay between opening the
  pidfd and sending, so the race stays closed; a reused pid reports
  `gone` instead of a refusal about an unrelated process.
- **A plan is bound to its subject too.** `redeem` takes the action and
  the subject (`{"pid": N}` or `{"group": …}`) and refuses either
  mismatch as `PlanError::OtherTool`, so the second call sends the same
  `args`. Re-computing "the same pid or group" needs the subject, and a
  plan for one pid cannot be spent on another. A plan offered with a
  request that is refused now is spent all the same.
- **A unit acts whole.** A service is stopped when any process planned
  in it is still there; processes that joined the unit since end with
  it, since `systemctl stop` cannot spare them. A unit whose planned
  processes are all gone is not stopped, and its new ones are listed as
  started after the confirmation.
- **The real signal test has two outcomes.** Inside the user's slice it
  asserts SIGTERM; outside one (CI runs in a container, cgroup `/`) the
  cgroup refusal holds for the test's own child too, so it asserts that
  refusal and that nothing was sent. The development machine ran the
  first branch (its cgroup is under `user@1000.service`).
- **A single sub-uid pid** in a rootless container (5001) is refused as
  another user's process; the runtime hint of D4 is given for the group.

## Acceptance (part 2)

The owner runs these on their own session (not run by the builder: the
safety rules of this work forbid signalling anything but a test's own
child):

1. `sleep 1000 &`, then `mirqab quit <pid>`: it prints the pid and
   `sleep`, asks, and after `y` the shell reports the job terminated.
2. The same with `--dry-run`: the job is still there.
3. `mirqab quit 1`, and `mirqab quit <pid of a root process>`: each
   prints the reason and exits 1.
4. The `action` events (`history.events` with `kind: "action"`) show one
   row, for step 1 only.

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
