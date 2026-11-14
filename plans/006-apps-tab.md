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
- **State**: IN PROGRESS (parts 1, 2 and 3a of 4: sampler, inspector, read
  CLI and MCP; Quit and Force quit, `mirqab quit`; the daemon side of the
  page: the `process` stream, start times, the units a stop takes with
  it, the session bus refused. Part 3b, the page, is next)

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

- `pidfd_open` on a thread id that is not a thread-group leader fails:
  `EINVAL` on older kernels, `ENOENT` on 7.2 (review of PR #17, run on
  kernel 7.2.7 against the reviewer's own thread; `sys.rs`'s test checks
  either). Both are `failed` with `strings::act_not_leader`: a second guard
  for D3.
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

Ancestors come from a strict walk (decision 9). The socket probe fails
closed (review of PR #17): it reads `$XDG_RUNTIME_DIR`, else
`run/user/<uid>`; a socket that refuses the connection (ECONNREFUSED, left
by a compositor that died) is skipped; any other connect or `peer_pid`
error, or a directory that cannot be read, refuses the action, dry run
included (`strings::refuse_compositor_unknown`). Only a runtime directory
that does not exist, or one with no live `wayland-*` socket, means no
compositor: a server or a tty session has none.

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
write nothing. With no history file (`Options.history` is `None`), or one
that cannot be opened or written, the action still runs, and the redeem
reply's `"recorded": false` says the row is missing; `mirqab quit` then
prints `strings::QUIT_NOT_RECORDED` on stderr. Owner's decision on the
review of PR #17 (2026-11-14): act and warn. The ruling's alternative,
refusing every act while the history file cannot be opened, was not
taken.

**11. The command line.**
`mirqab quit <pid|name> [-y] [--force] [--dry-run] [--json]`.

- A number is a pid. Anything else is a group name, matched whole and
  case-insensitively against the `apps` view. No match: exit 1. Several
  matches: print them with their ids and pids, exit 1; never guess.
- `--force` selects `apps.kill`.
- Flow: call without `confirm`; print the `wouldDo` and `skipped` lines;
  `confirm::ask`; call again with the plan. `--dry-run` sends `dryRun` and
  exits 0. No terminal and no `-y`: exit 3 (005:52-59).
- Exit 0 when every target is `done` or `gone`; 1 on `refused`, when any
  target `failed`, or when nothing was done and something was skipped
  (refused or started since the confirmation; review of PR #17).
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
- Deferred from the review of PR #17 (D4): `systemctl --user kill` after
  `stop --no-block` on a transient unit that is unloaded once stopped. The
  unit may be gone before the `kill`; the command would then fail and
  Force quit would report `failed` and exit 1 although the unit stopped.
  To be settled by the owner's acceptance run (checklist 3h).

#### Known limits (part 3a)

Written down by the review of PR #17 (K8) as "Known limits (part 2)";
part 3a (checklist 6c) closed the first level of dependents and the
session bus, and this is what is left:

- Only the first level of units a stop takes with it is asked about and
  refused: a dependent's own dependents are not walked, and their
  processes are not in `wouldDo`. A dependent `.target` (or any unit)
  takes its own dependents with it, unlisted; a target has no processes
  and shows as "(0 processes)" (review of PR #18, deferred to 6e).
- Quit of a service is `stop` (owner, Q1), which runs `ExecStop=` and ends
  in SIGKILL after `TimeoutStopSec`.
- For a unit the refusals are checked at each resolution, but `systemctl`
  acts on the unit by name: there is no pidfd and start-time binding at
  the moment of the command, as there is for a signal. The dependents are
  asked again at the confirmation, and one the plan did not show refuses
  it (review of PR #18); one that starts depending between that question
  and the `stop` is still not seen.
- The session bus is recognised by its unit name only
  (`guard::SESSION_BUS`); a bus started by hand in some other unit
  (`dbus-run-session`, `dbus-launch`) is not. A check by command name
  would be a new owner question (review of PR #18, deferred to step 7).


### Part 3: the page (design)

Planner's brief, 2026-11-14 (Fable). Evidence lines in this section are
from `origin/plan-006-apps-part2` (PR #17); where the build differs, "As
built (part 3a)" says so.


The Apps page, its inspector with Quit and Force quit, and the `apps.top`
card for Home. It adds three small things to the daemon, because the page
cannot be built correctly without them: a one-process sample that may run
live (decision 3), the start time of each listed process (decision 5), and
the units a service stop takes with it (decision 9, which settles D1).

#### What exists, and what is missing

| Need | Today | Evidence |
|------|-------|----------|
| The tab | `apps` is in `TABS` but not `.built()`, so it shows the placeholder | `crates/mirqab-view/src/shell.rs:63`, `crates/mirqab-ui/src/tabs/mod.rs:53-63` |
| The list | The `apps` stream gives groups, and every process of a group named in `expand` (or all, with `true`) | `crates/mirqab-proto/src/stream.rs:245-306`, `crates/mirqab-core/src/samplers/apps/mod.rs:259-306` |
| One view per subscriber | The daemon shapes each subscriber's frame from its params, but keeps one subscription per stream and connection | `crates/mirqab-core/src/server.rs:582-634`, architecture §5.2 ("Subscribing again to the same stream replaces the params") |
| Two consumers in the interface | The client merges the handles of a stream: shortest interval, `live` if any, and **the first handle's other params win** | `crates/mirqab-ui/src/client.rs:49-58` |
| Row identity | A listed process has a pid but no start time; the sampler has both | `stream.rs:297-306`; `samplers/apps/mod.rs:48-62`, `procfs.rs:34,199` |
| The inspector's text | `get apps.inspect {pid}` | `stream.rs:321-388`, `samplers/apps/mod.rs:379-406` |
| The inspector's 1 s graph | Promised twice, served by nothing: `Apps::live()` is the default `false`, and the stream is the whole walk | plan 006 "Stream apps"; architecture §5.3 lines 391-392; `crates/mirqab-core/src/sampler.rs:23-25` |
| A live graph with params | `Graph` subscribes live with no params; only it may | `crates/mirqab-ui/src/widgets/graph.rs:329-342`, `client.rs:122-133`, `tools/lint.sh:175-181` |
| Quit and Force quit | The two-step flow; `needs-confirm` carries `plan`, `wouldDo`, `skipped`; the reply carries `recorded` | `samplers/apps/actions.rs:691-706,838-867`, `server.rs:1267-1283` |
| Reading an error's data in the interface | `request` hands the reply's `ErrorBody` (code, message, data) to its callback | `client.rs:247` |
| A confirm dialog | `confirm_dialog(heading, would_do)` exists and is presented nowhere yet; its lines are the generic `key value` join | `crates/mirqab-ui/src/widgets/confirm_dialog.rs:17`, `crates/mirqab-view/src/shell.rs:191-204`, `crates/mirqab-ui/tests/widgets.rs:290-304` |
| A service's dependents | Not known to the daemon; `wouldDo` shows one `run` entry | `actions.rs:250-270,286-305`; "Known limits (part 2)" |
| App icons | The stream carries the desktop entry's `Icon=`; the interface draws Nerd Font glyphs only | `stream.rs:259-260`; architecture §7.2 lines 643-647; `crates/mirqab-ui/src/widgets/icon.rs:8` |
| Cards | `cards::all()` is the System tab's cards only; Home subscribes to its cards' streams with no params | `crates/mirqab-view/src/cards.rs:27-29`, `crates/mirqab-ui/src/tabs/home/page.rs:132-153` |
| A selftest that acts | `--selftest <tab>` prints one tree after the first data; it cannot type, select or press | `crates/mirqab-ui/src/selftest.rs:18-83` |
| A test caller for the interface | `MIRQAB_CALLER_PID` on a fixture root; without it the interface's real pid has no `proc/<pid>/stat` there and every action is refused | `server.rs:1380-1399` |

#### Decisions

1. **The page asks for everything once and the view model does the rest.**
   The page subscribes to `apps` at 2000 ms with
   `{"limit": 1000, "expand": true}` and never changes those params.
   Search, sort (CPU, memory, name), flatten and expanding a group are
   pure functions in `crates/mirqab-view/src/tabs/apps.rs` over the last
   frame, so each answers at once and none costs a subscribe. A
   re-subscribe would make the daemon sample at once
   (`server.rs:565-571`), and a CPU figure over a baseline as short as
   `MIN_GAP` (1 s, `server.rs:50`) is coarse. The sort uses the daemon's
   own order (`mod.rs:279-289`: lower-cased name, then the name, then the
   id; ties in CPU and memory by name), and a view-model test holds the
   two to the same answer on the golden frame. When
   `totals.groups` exceeds what arrived, the list ends with
   `strings::apps_more(n)`'s wording for the page.
   The daemon builds every process for every sample whatever a view asks
   (`mod.rs:147-208`), so `expand: true` adds bytes on the socket and a
   parse in the interface, not a second walk.

2. **One subscription, merged by a rule the stream owns.** The client
   gets `Client::set_merge(stream, fn(&[Value]) -> Value)`; `wanted()`
   (`client.rs:49-58`) calls it for the handles' params instead of taking
   the first. `mirqab_view::tabs::apps::merge_params` is registered for
   `apps` in `app.rs`: the largest `limit` (absent is the daemon's 50),
   `expand` is `true` if any asks `true`, otherwise the union of the id
   lists; `sort` is never sent. A stream with no rule keeps today's
   behaviour. The card on Home needs no params at all (decision 8), so
   Home's plain `Source::subscribe` (`home/page.rs:139`) stays as it is,
   and a consumer that receives more than it asked for takes what it
   needs. The page and Home are never mapped together, but both handles
   exist for a moment while the stack switches, and "first handle wins"
   would then send the page a frame of 50 unexpanded groups.

3. **The live graph is a new stream, `process`.** One sampler,
   `crates/mirqab-core/src/samplers/process.rs`, `live()` true:
   - params `{"pid": N}`; a subscribe without a valid `pid` is
     `bad-request` (a new `Sampler::accepts(&Value) -> Result<(), String>`,
     default `Ok`, called by `subscribe` and `get`);
   - each tick it reads `proc/<pid>/stat` and `proc/<pid>/statm` for each
     distinct pid its subscribers named, eight at most, through `Root`
     and the parsers of `samplers/apps/procfs.rs`. The loop already
     collects the distinct params before it samples (`server.rs:582-591`);
     a new `Sampler::wants(&mut self, &[Value])`, default empty, hands
     them over;
   - a subscriber's view is
     `{"pid":2001,"start":5010,"name":"browser","cpu":3.1,"rssBytes":…,"threads":12,"state":"S"}`,
     `cpu` null on its first reading, or `{"pid":2001,"gone":true}` once
     the pid is gone or its start time changed. The baseline is keyed by
     pid and start time, as the `apps` sampler's is (`mod.rs:47-62`);
   - it has its own lock, so a 1 s reading never waits behind the `apps`
     walk, `get apps`' half-second sleep or the inspector's `pacman`
     (deferred D1 of PR #16).
   `stat` and `statm` are world-readable, so another user's process can be
   graphed, as the System group already shows its figures. The stream
   reads nothing else and takes no action; it joins `stream::STREAMS`,
   `capabilities`, and architecture §5.3, where "the Apps inspector's
   single-process sample" becomes its row.
   In the interface `graph::Live` gains `params: Value`, and
   `Source::subscribe_live(stream, params, on_event)` passes them;
   `widgets/graph.rs` stays the one caller (lint unchanged). The
   inspector's graph has no range picker: its range is `2m`, live only,
   and nothing is stored (plan 006, "No per-app series").

4. **What the inspector graphs.** A selected process row: that pid, live
   from `process`, two series off one event (CPU on a 0–100 scale that
   grows past 100, memory as text beside it, not a second line). A
   selected group row: the group's CPU from the `apps` frames, pushed
   into the same widget every 2 s with `Graph::push_live`
   (`graph.rs:348`), with no live subscription; a group is many pids, and
   the live rule buys one. The graph's accessible name says which it is.
   When `process` is not in `capabilities` the process graph falls back
   to the 2 s figures of its row.

5. **Row keys (D5).** `AppProcess` gains `start` (clock ticks since boot,
   the value `wouldDo` already carries). A process row's key is
   `(pid, start)`. A group row's key is a number the view model assigns:
   a group in a new frame keeps the key of the previous frame's group
   with the same `id` **only if they share at least one `(pid, start)`**;
   otherwise it is a new row with a new key. `system` always keeps its
   key. So a `session:<sid>` or `tree:<pid>` id that a new session or a
   reused pid takes over starts collapsed and unselected, and the
   inspector of the old one closes with `strings::APPS_ROW_GONE`.
   Expansion, selection and the pending dialog all hang on keys, never on
   ids or bare pids. Before the first call of Quit on a process row the
   interface compares its row's `start` with the `wouldDo` entry's; when
   they differ it shows `APPS_ROW_GONE` and asks nothing.

6. **The list is a `gtk::ListView` over a flat model.** The view model
   returns `Vec<Row>` (`Row::Group` or `Row::Process`, in display order,
   a group's processes after it when it is open; in flatten mode process
   rows only, each with its group's name dimmed). The store holds one
   plain object per position; each frame writes row `i` into object `i`
   and notifies only when the row changed, and the store grows or shrinks
   at its end. Bound list items redraw; nothing is reordered, so the
   scroll position holds. Each list item holds one `GroupRow` and one
   `ProcessRow` and shows the one its row is (one type per file,
   §7.2). `gtk::NoSelection` with single-click activate: the selected key
   lives in the view state and the row carries the `selected` class and
   `Property::Selected` for assistive technology. Cost follows the rows on
   screen, not the processes on the machine.

7. **Rows.** Group: expander button (hidden when it has one process),
   icon, name, CPU meter and number, memory, process count. Process: name,
   pid, CPU, memory, state when it is not `S` or `R`. The meter is the
   existing `Meter` (`widgets/meter.rs`), full at 100 % of one core; the
   number is percent of one core and may pass 100, as `mirqab apps`
   prints it, and it is the meter's value text. Every string from
   `strings.rs`; `plain()` (`strings.rs:1281`) on every name, as the CLI
   does.

8. **Icons.** A group's `icon` is drawn with `gtk::Image` from the icon
   theme when it is a theme name the display's `gtk::IconTheme` has
   (`has_icon`); a value containing `/` is never loaded: the interface
   reads no file the daemon's data names. Otherwise the row shows a glyph
   for its kind (new constants in `icons.rs`: app, service, terminal,
   container, flatpak, process, system). Size in em, role presentation.
   Architecture §7.2's icon rule gains one sentence: "the Apps tab's rows
   show an app's own icon from the icon theme, by name".

9. **A service's dependents (D1).** For a `Step::Unit` the daemon asks
   which units a stop takes down: `systemctl --user show -p RequiredBy
   -p BoundBy -p ConsistsOf --value -- <unit>`, then `show -p Id -p
   ActiveState` for those names, through `exec.rs`, 2 s each, one level
   deep. This goes on the `Sender` trait (`act.rs:73-91`): the real
   sender runs the commands; `Recording` answers from the fixture root
   (`run/mirqab-test/dependents/<unit>`, one unit a line), so a fixture
   root still runs nothing. The `run` entry of `wouldDo` gains
   `"alsoStops": ["b.service", …]` (active ones only), or
   `"alsoStopsUnknown": true` when the question failed. Then:
   - the processes of a dependent **service** are found by cgroup as the
     unit's own are (`actions.rs:478-483`) and pass the same refusals; a
     refused one refuses the stop whole, with
     `strings::refuse_unit_dependent(unit, dependent, why)`. This closes
     "no refusal sees them" for the first level;
   - the dialog and `mirqab quit` print one line per dependent ("also
     stops b.service (2 processes)"), or "other services that depend on
     it may stop too" when unknown;
   - deeper levels are not walked; "Known limits" keeps that sentence.
   Quit stays `stop` (owner, Q1), and only the session bus is denied by
   name (owner, Q2): see "Owner decisions (part 3)" below.

10. **The inspector is a side sheet in the window.** The page's root is
    an `adw::OverlaySplitView` with the sidebar at the end, shown while a
    row is selected and collapsed (overlaid) under 60 em of page width.
    It is in the window's widget tree, so the selftest sees it, and in a
    popover it takes no focus away (`window.rs:280-298` closes a popover
    on focus loss). Content, top to bottom: icon, name, kind; the verdict
    sentence; the graph; CPU, memory, threads, processes from the row;
    then `get apps.inspect {pid}` once per selection and on the refresh
    button (never on a timer): parents, user, started, executable,
    package, unit, sandbox, memory, files, each row left out when the
    daemon left it out. A group selection inspects the group's main
    process: the member whose parent is not a member, lowest pid on a
    tie (`apps::main_pid`, from `ppid`).

11. **Quit and Force quit.** Two buttons at the foot of the sheet. A
    process row sends `{"pid": N}`, a group row `{"group": id}`.
    - First call without `confirm`. `needs-confirm` opens
      `confirm_dialog` with the heading `QUIT_WILL` or `FORCE_QUIT_WILL`
      (`strings.rs:1427-1428`) and typed lines from
      `apps::confirm_lines(data)`: one per `signal` entry (name, pid,
      "asked for its thread" when `via` is set), one per `run` entry in
      words ("stops x.service with systemctl, 3 processes") followed by
      its pids and its `alsoStops` lines, then a "Left alone" block with
      every `skipped` entry and its reason. `would_do_line` stays for
      other actions.
    - Go ahead sends the same `args` with `confirm: true` and the plan.
      The reply is one line in the sheet: done, already gone, or each
      `failed` entry with its error. `recorded: false` adds
      `APPS_NOT_RECORDED` (the CLI's sentence without its `mirqab:`
      prefix) as a warning line that stays until the selection changes.
    - `refused`, `plan-expired` and any other error show the daemon's
      message in the sheet, in words, with no dialog. The interface
      repeats no refusal rule and asks no dry run on selection: the
      compositor probe can block the connection's thread (deferred D2 of
      PR #17).
    - The System group and a process `apps.inspect` marks `limited` show
      no buttons, and `APPS_NOT_YOURS` instead.
    - The row that holds the interface itself (its pid is
      `std::process::id()`, matched against the frame) says
      `APPS_QUIT_SELF`: "Quit closes this window. The daemon keeps
      running." The daemon's row needs nothing: the daemon refuses it in
      words.
    - After a Quit whose targets are still listed two frames later, the
      sheet says `APPS_STILL_RUNNING` beside Force quit. The daemon does
      not wait for exits (plan 006, Rejected, "Waiting for the processes
      to exit").
    - Dialog default and Escape are Cancel; Force quit's Go ahead is
      destructive, as `confirm_dialog` already sets.

12. **The card `apps.top`.** `apps::CARDS = [Card { id: "apps.top", tab:
    "apps", streams: &["apps"], size: Size::Half }]`, chained into
    `cards::all()`. It shows the five groups using the most CPU of
    whatever frame arrives, System included when it is among them (a
    root daemon eating a core must not be hidden): icon, name, meter,
    CPU. Activating a row opens the Apps tab with that group selected
    (`Ctx` keeps a pending group id; the page resolves it on its next
    frame). It is appended to `home::SEED` (plan 003: "Later plans append
    their headline card to the seed list"); a config already seeded is
    not touched. The Apps page's toolbar carries the pin control with the
    existing `PIN_TO_HOME` / `UNPIN_FROM_HOME`.

13. **What the page remembers.** Sort, the flatten switch, open groups
    and the selection live in an `AppsState` on `Ctx`, so they survive the
    page being dropped on hide and end with the interface process. The
    search field starts empty each time.

14. **Subscription lifetime.** The page subscribes on map and drops on
    unmap, as System does (`tabs/system/page.rs`); the card is Home's.
    `process` exists only while the inspector's graph is mapped with a
    process selected. Closing the window ends all three.

15. **A selftest that can act.** `mirqab-ui --selftest <tab> --do STEPS`
    (accepted only with `MIRQAB_TEST=1`): steps separated by `;`, run in
    order after the page's first data, the tree printed after the last.
    `type NAME TEXT`, `activate NAME`, `respond ID` (the open alert
    dialog's response), `wait NAME` (a visible widget of that name, 5 s).
    `NAME` is the widget name the dump already prints
    (`selftest.rs:100-103`); the page names `apps-search`,
    `apps-group:<id>`, `apps-process:<pid>`, `apps-inspector`,
    `apps-quit`, `apps-force-quit`, `apps-result`. Architecture §7.5 gains
    the paragraph.

#### Rejected

- **Per-view params from the page** (`sort`, `limit`, `expand` changed on
  every click). Each change is a subscribe, an immediate sample over a
  short baseline, and a merge problem with the card. The daemon's
  per-subscriber views stay for the CLI and MCP.
- **A second connection for the card or the inspector.** Two sockets, two
  reconnect states, and the daemon would count two interfaces.
- **Keyed subscriptions in the protocol** (several per stream and
  connection). A protocol change for one tab; the merge rule is six lines.
- **Polling `get apps.inspect` every second for the graph.** It runs
  `pacman`, reads `fdinfo`, holds the `apps` sampler's lock, and would be
  a sampler in the interface (§7.2 timers).
- **`live` on the `apps` stream with a `watch` pid.** The stream is the
  whole walk; 1 s of it is what §5.3 forbids.
- **A group's live graph** (every member each second). Unbounded pids; a
  browser is forty reads a second.
- **`gtk::ListBox` with a widget per process.** Four hundred processes
  are some four thousand widgets and a relayout every 2 s.
- **`gtk::TreeListModel` with `TreeExpander`.** It wants the model to own
  expansion; the view model already does, and tests it without GTK.
- **A dry run on every selection to grey the buttons out.** A resolve and
  a compositor probe per click on a row; the refusal in words after the
  press is enough.
- **Loading an icon by path.** The interface would read a file named by a
  desktop entry.
- **Sort and flatten in the config.** Two keys and their validation for a
  preference nobody has asked to keep.
- **Refusing a service whose dependents cannot be listed.** Where
  `systemctl` cannot answer, `stop` fails too; the dialog says the list
  is unknown.

#### Tests

| Test | Checks |
|------|--------|
| `crates/mirqab-view/tests/apps.rs` (new) | On `tests/golden/apps.json` with `expand: true`: each sort equals the daemon's `view` for the same sort; search by group name, by member name and by pid; flatten; open and closed groups; the "more" line; `main_pid`; `top_five` with and without System; `merge_params` (largest limit, `expand` union, `true` wins, no params is 50) |
| same | Keys: a group keeps its key across frames with a shared `(pid, start)`; the same `session:` id with no shared member gets a new key and loses expansion and selection; a pid with a new start is a new process row |
| same | `confirm_lines`: a signal entry, a `via` entry, a `run` entry with pids and `alsoStops`, `alsoStopsUnknown`, the skipped block; the result line for done, gone, failed, and `recorded: false`; control characters in a name come out as U+FFFD |
| `crates/mirqab-core/tests/apps.rs` | The `process` stream: figures for a fixture pid across `.t1`; `gone` for a missing pid and for a changed start; no `pid` is `bad-request`; `start` in every listed process |
| `crates/mirqab-core/tests/actions.rs` | `alsoStops` from the fixture answer; an inactive dependent left out; a dependent holding a refused process refuses the stop; a failed question gives `alsoStopsUnknown` and still plans |
| `crates/mirqab-core/tests/protocol.rs` (D6) | A re-subscribe with new params changes the next frame and leaves no old view behind; unsubscribe removes it; two subscribers that differ only in interval get the same line; `process` at 1000 ms only with `live` |
| `crates/mirqab-ui/tests/widgets.rs` | The fake `Source` gains `request` and live params. The inspector with a fake that answers `needs-confirm`: the dialog's body lists every pid; Go ahead sends `confirm` and the plan; `refused` shows the message and opens nothing; the process graph is live only while mapped and drops on a group selection; a row draws no blur |
| `tests/test-ui-apps.sh` (`--selftest apps` on `root-desktop`, `MIRQAB_CALLER_PID` a fixture shell, `MIRQAB_ACT_LOG` set) | The groups of the golden frame are listed, System last; `type apps-search Browser` leaves one group; `activate apps-group:…` shows the inspector with the verdict; `activate apps-quit` shows a dialog naming pids 2000, 2001, 2010, 2011, 2012; `respond confirm` writes five TERM lines to the act log and the result line; Quit on pid 410 shows the refusal in words and no dialog; `status` counts one `apps` subscriber while the page is shown and none after `close`, and one `process` subscriber only while a process is selected |
| `tests/test-ui-home.sh` | `apps.top` pinned shows five rows; a fresh config seeds four cards. Its "placeholder" step moves from `apps` to a tab that is still one |
| `tests/test-cli.sh` | `mirqab quit` on the fixture service prints the "also stops" line |

Affected scope only (`tests/run.sh --affected`); the new files carry
`# mirqab:covers` for `crates/mirqab-ui/src/tabs/apps/**`,
`crates/mirqab-view/src/tabs/apps.rs` and
`crates/mirqab-core/src/samplers/process.rs`.

#### Cost

To measure and record in "As built (part 3)", not gated here (the
benchmark is part 4's step 9):

- the size of one `apps` frame with `expand: true` on `root-desktop` and
  on the development machine, and the interface's CPU while the page is
  shown and idle (`tools/bench-idle.sh` reads the daemon; the interface is
  `pidstat -p $(pgrep -x mirqab-ui) 10 1`);
- the interface's RSS with Apps open against System open (§9.1: reported,
  not gated);
- the daemon's CPU with one `process` subscriber at 1 s: two small reads a
  second, expected under 0.1 % of a core;
- window closed: no `apps` and no `process` subscriber, from `mirqab
  status --json`.

#### Acceptance (part 3)

Headless, in CI: every row of the table above.

Owner checks on the development machine (they need a real session, icon
theme and compositor):

1. The list shows recognisable apps with their own icons, terminal jobs
   by name and System last; sorting, search and "show processes" answer
   at once; rows are still easy to click while the order changes every
   2 s under the CPU sort.
2. Selecting a row opens the sheet; the graph of a process moves once a
   second and stops when the window hides (`mirqab status` shows no
   `process` subscriber).
3. Quit on a `sleep 1000` started by hand lists its pid and ends it; on a
   root process the sheet says why not; on a `--user` service with a
   dependent the dialog names the dependent.
4. Quit on mirqab's own row says what it does, and does it.
5. In a popover the dialog and the sheet do not close the popover.
6. `apps.top` on Home shows five rows and opens the tab.

#### One pull request or two

Two. The daemon side stands alone and is reviewed for safety, as part 2
was; the page is large and is reviewed for the interface rules.

- **3a, daemon:** the `process` stream with `Sampler::wants` and
  `accepts`; `start` in `AppProcess` (the golden file changes);
  `alsoStops`, its refusal, and the two CLI lines; the D6 protocol tests;
  architecture §5.3 and §5.4. No interface change.
- **3b, interface:** the view model, the six files under `tabs/apps/`, the
  client's merge rule and live params, the selftest steps, the card, the
  seed, `.built()`, architecture §7.2 and §7.5, the user guide's Apps
  page.

The owner's answers to Q1 and Q2 (below) land in 3a.

#### Owner decisions (part 3)

The brief asked two questions; the owner answered on 2026-11-14.

- **Q1. Should Quit on a service be `systemctl --user kill
  --signal=TERM` instead of `stop`?** `kill` runs no `ExecStop=`, never
  escalates to SIGKILL, and does not stop `Requires=` and `PartOf=`
  dependents by itself; but a unit with `Restart=always` comes straight
  back, which is why part 2 chose `stop`. The planner recommended keeping
  `stop` now that the dialog names what it takes with it. **Owner: keep
  `systemctl --user stop`.**
- **Q2. Should some session services be denied by name?** The planner
  recommended refusing only the session bus, which like the compositor
  takes the session with it, and warning for the audio server and the
  portals, which are socket- or bus-activated and come back on next use.
  **Owner: refuse only the session bus by name** (`dbus.service` and
  `dbus-broker.service` are never stopped or signalled, a refusal with a
  reason in words, in `strings.rs`, tested). **For the audio server
  (`pipewire`, `wireplumber`, `pipewire-pulse`) and `xdg-desktop-portal*`
  services Quit is allowed, and the `needs-confirm` reply carries a
  one-line warning** that the CLI prints and 3b's dialog will show.


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
| `crates/mirqab-view/tests/apps.rs` (part 3b) | The view model on the golden frame with `expand: true`: sorts as the daemon does, search, flatten, row keys across frames, the merge rule, the dialog's lines |
| `crates/mirqab-core/tests/protocol.rs` (D6, part 3a) | A re-subscribe replaces a view and leaves no old one; an unsubscribe removes it; two subscribers that differ only in interval share one line; `process` at 1000 ms only with `live`, its params checked |
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
5. The inspector's graph of a process moves once a second and stops when
   the window hides (`mirqab status` shows no `process` subscriber).
6. Quit on a `--user` service with a dependent names the dependent in the
   dialog and in `mirqab quit`; Quit on the sound server warns first.
7. `apps.top` on Home shows five rows and opens the tab.

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
6. [ ] The page (part 3; design in "Part 3: the page (design)").
   - Deferred from the review of PR #17 (D1): before the dialog offers
     Quit on a service row, list the units a stop takes with it
     (`systemctl --user show -p RequiredBy -p BoundBy -p ConsistsOf`, a
     host-only read with a fixture answer), consider Quit as
     `systemctl --user kill --signal=TERM` instead of `stop`, and a
     deny-list for session services (the owner's call). Settled by 6c and
     the owner's decisions (part 3); "Known limits (part 3a)" keeps what
     is left.
   - Deferred from the review of PR #16 (D5): session-scope group ids
     (`session:<sid>`) can be reused after a logout; the view model's row
     keys must not rely on them alone across frames. Settled by 6b (the
     daemon side) and 6e (the keys).
   - 6a. [x] `process` stream: `samplers/process.rs`, `Sampler::wants` and
     `accepts`, `stream::PROCESS` and its data type, tests. (PR 3a)
   - 6b. [x] `start` in `AppProcess`; regenerate `tests/golden/apps.json`.
     (3a; settles D5 with 6e)
   - 6c. [x] D1: `Sender::unit_dependents`, `alsoStops` /
     `alsoStopsUnknown` in `wouldDo`, the dependent refusal, the fixture
     answer, `mirqab quit`'s lines, strings; rewrite "Known limits
     (part 2)" to what is left (deeper levels, `ExecStop=`, no pidfd for
     a unit). The owner's Q1 and Q2: `stop` kept, the session bus
     refused, the sound server and portals warned about. (3a)
   - 6d. [x] Architecture §5.3 (the `process` row, the list of live
     streams) and §5.4 (dependents in the dialog). (3a)
   - 6e. [ ] `crates/mirqab-view/src/tabs/apps.rs`: `AppsState`, `rows`,
     keys, sort, search, flatten, `main_pid`, `top_five`,
     `merge_params`, `confirm_lines`, `result_line`, `CARDS`; `cards::all`
     chains it; `home::SEED` gains `apps.top`; `shell::TABS` marks `apps`
     built; glyphs in `icons.rs`; strings. (3b)
     - Deferred from the review of PR #18 (S3, second half): a dependent
       that is not a service, scope or slice (a `.target`) takes its own
       dependents with it, unlisted; `confirm_lines` reads "and what
       depends on it" for it, and `mirqab quit`'s "also stops" line says
       the same. Walking deeper, with a depth bound, stays out until a
       real unit needs it.
   - 6f. [ ] Client: `set_merge`, `Source::request`,
     `subscribe_live(stream, params, …)`, `graph::Live.params`. (3b)
     - Deferred from the review of PR #18 (nit): `process` accepts a
       thread id and reads that thread's own `stat`; harmless and read
       only. The client sends `pid` and `start` from a row, never a typed
       number; one sentence in architecture §5.3 when 3b touches it.
   - 6g. [ ] `crates/mirqab-ui/src/tabs/apps/{mod,page,group_row,process_row,inspector,top_card}.rs`;
     `tabs::build` and `tabs::card`; the pin control. (3b)
   - 6h. [ ] `--selftest … --do`, `tests/test-ui-apps.sh`, the widget
     tests, `tests/test-ui-home.sh`. (3b)
   - 6i. [ ] Architecture §7.2 (app icons), §7.5 (selftest steps);
     `docs/user-guide/` Apps page; "As built (part 3)" with the measured
     costs. (3b)
7. [ ] Focus tracking: `crates/mirqab-core/src/desktop/hyprland.rs` (event
   socket reader, active only while `apps` has a subscriber) (part 4).
   - Deferred from the review of PR #18 (bus outside a unit): a session
     bus run by `dbus-run-session` or `dbus-launch` is in no bus unit and
     is not refused ("Known limits (part 3a)"). The owner's Q2 said "by
     unit name"; refusing by command name is a new owner question, to ask
     with part 4.
   - Deferred from the review of PR #17 (D2): the compositor probe's
     connect is blocking; a frozen compositor with a full listen queue
     holds one client thread (no lock). Make it non-blocking, which needs
     a non-blocking connect std does not offer (new `unsafe` in `sys.rs`).
8. [ ] Tests above (part 1 wrote the read side's).
   - Deferred from the review of PR #16 (D6): views on a re-subscribe
     with new params, their cleanup on unsubscribe, and two subscribers
     that differ only in interval; written with part 3, where the page
     changes `sort` and `expand` live.
   - D6: [x] `crates/mirqab-core/tests/protocol.rs`, the rows of "Part 3:
     the page (design)", Tests (3a).
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
   - Deferred from the review of PR #18 (nit): the `process` stream's
     eight-pid cap keeps the first eight in client order; decide with the
     other `process` costs whether the newest should win.

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
  failure with `strings::ACT_NO_PIDFD`, with no fallback; EINVAL or
  ENOENT (a thread id) a failure with `strings::act_not_leader`.
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
  the `SO_PEERCRED` pid of each `wayland-*` socket in `$XDG_RUNTIME_DIR`,
  else `run/user/<uid>` (`compositor_sockets`, `actions.rs:342`), read per
  resolution; a probe that fails other than by ECONNREFUSED refuses the
  action (review of PR #17).
- **The caller** is the peer's pid, kept on the `Client`
  (`crates/mirqab-core/src/server.rs:661`); `MIRQAB_CALLER_PID` replaces
  it when `MIRQAB_TEST=1` and the root is a fixture (`caller_for`,
  `server.rs:1396`; on `/` the kernel's peer pid always wins). An
  unidentified caller (`REFUSE_NO_CALLER`) and a strict walk that fails
  refuse the action, dry run included.
- **The strict walk.** `guard::ancestry_strict`
  (`crates/mirqab-core/src/guard.rs:37`) returns `WalkError::Unreadable`,
  `Loop` or `TooDeep`; the old quiet `ancestry` had no caller and is gone.
- **Plans** (`crates/mirqab-core/src/plan.rs`): `make` and `redeem`
  (`:98`, `:140`) take the boot-time clock, read once per request by
  `Actions` (`actions::Clock`; an unreadable clock fails the request with
  `strings::ACT_NO_CLOCK` before anything is planned or redeemed); at most `MAX_PLANS` = 64,
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
  nothing; without a history file, or with one that cannot be written,
  the action still runs and the reply says `"recorded": false`
  (`actions.rs:838`, `:867`).
- **The CLI.** `crates/mirqab/src/quit.rs`: a number is a pid, anything
  else a name matched whole and ignoring case against `get apps` with no
  limit (`subject`, `quit.rs:115`); several matches are printed with
  their ids and pids and exit 1. The plan is printed under "This will
  quit:" or "This will force quit:" (`confirm::ask_with`,
  `crates/mirqab/src/confirm.rs:23`); a declined or unanswerable prompt
  exits 3, as in `mcp-config`. With `--json` the list goes to stderr
  and stdout holds the answer alone. Exit 1 on `refused`, when any target
  failed, or when nothing was done and something was skipped
  (`exit_after`, `quit.rs:246`); `recorded: false` prints
  `strings::QUIT_NOT_RECORDED` on stderr. `CallError::Daemon` carries the reply's `data`
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
- **The host's service, not the deepest one** (review of PR #17, K1).
  The design took a process's unit from the deepest `.service` in its
  cgroup. A process in a nested cgroup (a rootless container running its
  own systemd, or a service's sub-cgroups) then named a unit of another
  manager, and `systemctl --user stop` would have stopped the host's unit
  of that name. `group::host_service` (`group.rs:129`) takes the first
  unit under the user manager after its slices, and only if it is a
  `.service`; a nested process is signalled by pid. A `service:` group is
  stopped only when every member is in that host service.

From the review of PR #17 (2026-11-14; the ruling kept 10 items, deferred
4 and dropped the rest):

- **K1, the host's service.** Above. Tests: `units_under_the_user_manager`
  in `group.rs`; `a_nested_cgroup_never_names_a_host_service` in
  `tests/actions.rs` (the libpod path's dry run is one `TERM 1300` entry
  with no `run`; `outer.service/payload/inner.service` stops
  `outer.service`).
- **K2, `--json` keeps stdout clean.** `confirm::ask_with` takes
  `to_stderr` (`crates/mirqab/src/confirm.rs:31`); `quit.rs:217` passes
  `json_out`. Test: `tests/test-cli.sh` parses stdout of
  `quit 2001 -y --json` alone with `jq -e`.
- **K3, the test caller only on a fixture root.** `caller_for`
  (`server.rs:1396`). Test: `the_test_caller_counts_only_on_a_fixture_root`.
- **K4, the clock.** `sys::boottime` returns `io::Result`
  (`sys.rs:108`); `Actions::with_clock` (`actions.rs:648`) and one reading
  per request (`actions.rs:672`). Test:
  `a_plan_expires_on_the_actions_clock_and_no_clock_acts_on_nothing`
  (121 s expired, 119 s acts, an erroring clock fails with no plan).
- **K5, a thread id's errno.** `open_error` maps ENOENT with EINVAL
  (`act.rs:339`). Tests in `sys.rs`: a spawned thread's tid gives ENOENT
  or EINVAL and this process's pid a pidfd; `peer_pid` of a socket pair
  and `boottime`.
- **K6, act and warn** (owner, 2026-11-14). `recorded` in the redeem
  reply; `QUIT_NOT_RECORDED`; architecture §5.4 and decision 10. The
  ruling's step 1, refusing every act while the history cannot be
  opened, was not taken. Tests: `each_redeemed_plan_is_one_action_event`
  (`recorded: true`), `an_act_without_a_history_says_it_was_not_recorded`
  (`recorded: false`, the act recorded by the sender), and
  `an_act_with_no_writable_history_runs_and_says_it_was_not_recorded` in
  `protocol.rs` (a directory where the history file should be: history
  off, a dry run lists, the confirmed act runs on the recording sender
  and replies `recorded: false`).
- **K7, the probe fails closed.** `compositor_sockets(dir)` and
  `runtime_dir` (`actions.rs:342`, `:375`), the refusal at `:409`. Test:
  `the_compositor_probe_skips_dead_sockets_and_fails_closed` (a live
  listener gives this pid; after it is dropped the stale socket is
  skipped; `.lock` and a regular file are ignored; a missing directory is
  empty).
- **K8, the limits of a unit.** The module doc of `actions.rs`, "Known
  limits (part 2)" above, architecture §5.4. No test (text).
- **K9, untested rules.** `act.rs`'s refusal test gains a kernel thread
  that is otherwise the user's (4949) and the test process as the daemon,
  each with its exact reason; `a_failed_stop_skips_the_kill` in
  `tests/actions.rs` (a sender whose `systemctl` exits 1: one `failed`
  entry with the `stop` argv, no `kill` run).
- **K10, exit 1 when nothing was done.** `exit_after` (`quit.rs:246`);
  the help text in `docs/cli.md`. Test:
  `nothing_done_and_something_left_alone_is_a_failure` (five replies).
- **Counts after the fixes.** `tests/actions.rs` 15, `protocol.rs` 19,
  `mirqab-core` unit tests 51, `mirqab` 18, `mirqab-proto` 18,
  `tests/test-cli.sh` 185 checks.

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

## As built (part 3a)

Part 3a (2026-11-14) is checklist steps 6a–6d and the D6 tests of step 8:
the daemon side of the page, as designed in "Part 3: the page (design)"
with the owner's decisions. No interface code changed. What was built,
with where it lives:

- **Two new sampler hooks.** `Sampler::accepts` returns an `Accepts`, a
  plain `fn(&Value) -> Result<(), String>` (`crates/mirqab-core/src/sampler.rs:18`,
  `:59`), copied into the slot like `view` (`server.rs:396`) and called
  by `subscribe` (`server.rs:945`) and `get` (`:1033`): an Err is
  `bad-request` and adds no subscription. `Sampler::wants(&[Value])`
  (`sampler.rs:68`) gets the subscribers' distinct params under the
  sampler's lock just before each sample (`server.rs:615`), and a `get`'s
  own params before its snapshot (`:1042`). A third hook, `idle`, came
  with the review (below).
- **The `process` stream** (`crates/mirqab-core/src/samplers/process.rs`),
  registered after `apps` in its own slot (`server.rs:379`), so its lock
  is its own. Params `{"pid": N}` and optionally `"start"`, nothing else
  (`accepts`, `process.rs:57`). Each tick reads `stat` and `statm` of the
  first eight distinct pids (`wants`, `:242`; `stream::PROCESS_MAX_PIDS`)
  through `Root` with `samplers/apps/procfs.rs`'s parsers; CPU is percent
  of one core against a baseline keyed by pid and start time, null on the
  first reading, dropped after 10 s (`sample`, `:253`). A subscriber gets
  `ProcessData` (`crates/mirqab-proto/src/stream.rs:341`):
  `{pid, start, name, cpu, rssBytes, threads, state}`, or
  `{"pid": N, "gone": true}` when the pid holds no process or another one
  than asked for (`view`, `process.rs:203`), or `{"pid": N, "capped":
  true}` past the eighth pid. `live()` is true. Golden:
  `tests/golden/process.json` (pid 2001 on `.t1`, 40 % of one core).
- **`start` in every listed process** (`stream.rs:315`, set at
  `samplers/apps/mod.rs:182`): clock ticks after boot, the value
  `wouldDo` carries. `tests/golden/apps.json` is now the view with
  `expand: true`, so it holds every process with its start time (24
  groups, 44 processes; the group fields are unchanged), which is the
  frame 3b's view-model tests read.
- **The units a stop takes with it.** `Sender::unit_dependents(root,
  unit)` (`crates/mirqab-core/src/act.rs:93`): `act::Real` (`:288`) runs
  `act::dependents_via` (`:199`) over `exec.rs`: `systemctl --user show
  -p RequiredBy -p RequisiteOf -p BoundBy -p ConsistsOf -p
  PropagatesStopTo -- <unit>` and then `show -p Id -p ActiveState --
  <names>`, 2 s each, parsed by key and by systemd's quoting
  (`dependents_named`, `:171`; `show_words`, `:122`; `active_ids`,
  `:245`); `active`, `activating`, `reloading` and `refreshing` count.
  `act::Recording` answers from `run/mirqab-test/dependents/<unit>` under
  the root (`:371`), one `<unit> <ActiveState>` a line; no file is no
  dependents, an unreadable one a failed question. `unit_step`
  (`samplers/apps/actions.rs:577`) lists each active dependent's
  processes by cgroup, for any unit type (`unit_members`, `:543`, on
  `group::host_units`), and runs the same refusals on them; a refused
  one, or the session bus as a dependent, refuses the stop with
  `strings::refuse_unit_dependent`. The stop's `wouldDo` entry gains
  `"alsoStops": [{"unit": …, "pids": […]}]`, or `"alsoStopsUnknown":
  true` when the question failed (the stop is still planned); Force
  quit's `kill` entry carries neither (`:292`). The `action` event names
  them under `alsoStops` (`:1017`).
- **The owner's decisions.** Q1: Quit on a service stays `systemctl
  --user stop`. Q2: `guard::SESSION_BUS` (`crates/mirqab-core/src/guard.rs:58`)
  is refused by name for a unit (`unit_step`, as the unit or as a
  dependent, `strings::refuse_session_bus_unit`) and for a signal
  (`act::refusal`, `act.rs:479`, `strings::refuse_session_bus`, so a
  process of the bus is refused at plan time and again through the
  pidfd). `guard::session_warning` (`guard.rs:76`) gives one line for
  `pipewire.service`, `pipewire-pulse.service`, `wireplumber.service`
  (`strings::warn_sound`) and `xdg-desktop-portal*.service`
  (`strings::warn_portal`); the dry-run reply and the `needs-confirm`
  data carry them as `warnings` (`actions.rs:824`, `:843`), for the
  unit and its active dependents, each once.
- **`mirqab quit`** prints, under a stop, one `also stops <unit> (N
  processes)` line per dependent or `other services that depend on it may
  stop too`, and each warning as `warning: …` after the list
  (`plan_lines`, `crates/mirqab/src/quit.rs:35`). Its help (and
  `docs/cli.md`) names the session bus among what is never signalled.
- **Fixtures.** `tests/fixtures/root-desktop/run/mirqab-test/dependents/pipewire.service`:
  `wireplumber.service` active (pid 1211), `pipewire-pulse.service`
  inactive; `tests/fixtures/README.md` says what it stands for.
- **Tests.** `crates/mirqab-core/tests/actions.rs` 19 (new:
  `a_stop_names_the_active_units_it_takes_with_it`,
  `a_dependent_that_holds_a_refused_process_refuses_the_stop`,
  `a_question_that_fails_still_plans_and_says_so`,
  `the_session_bus_is_never_stopped`); `crates/mirqab-core/tests/apps.rs`
  16 (new: `every_listed_process_carries_its_start_time`,
  `the_process_stream_reads_one_pid_across_two_ticks`,
  `a_missing_pid_or_a_new_start_time_is_gone`,
  `the_process_stream_takes_only_a_pid_and_a_start`);
  `crates/mirqab-core/tests/protocol.rs` 22 (D6:
  `a_re_subscribe_replaces_the_view_and_leaves_no_old_one`,
  `subscribers_that_differ_only_in_interval_share_one_line`, on a
  `Counted` stream whose view shows the sample number and the params it
  was cut with; and
  `the_process_stream_serves_one_pid_live_and_checks_its_params`);
  `mirqab-core` unit tests 56 (the stream's params and its cap, the
  dependents' parsing, the bus never signalled, the bus and warning
  lists); `mirqab` 19 (`a_stop_lists_what_it_takes_with_it_and_the_warnings`);
  `tests/test-cli.sh` 193 checks (8 new: `quit 1210` names
  `wireplumber.service`, leaves out the inactive unit, warns, carries
  `alsoStops` and `warnings` in `--json`, and records only the one
  `stop`).

Measured (resolving the brief's Unverified items that 3a depends on):

- **`systemctl show` prints properties in its own order.** On the
  development machine (systemd 262.1, 2026-11-14), `systemctl --user show
  -p ConsistsOf -p RequiredBy -p BoundBy -- pipewire.service` printed
  `RequiredBy=`, `BoundBy=wireplumber.service pipewire-pulse.service`,
  `ConsistsOf=`, in that order whatever the order of the flags, and with
  `--value` three lines with nothing to say which is which. `show -p Id
  -p ActiveState -- a b c` printed one block per unit, blank-line
  separated, an unknown unit as `ActiveState=inactive`. Hence the parse
  by key. systemd 257 was not available to check.
- **List values are quoted per element** (review of PR #18, on the same
  machine): an element is bare unless it holds whitespace or a
  shell-special character, and is then wrapped in `"…"` with a backslash
  before each `"`, `\`, `` ` `` and `$` (systemd's `shell_maybe_quote`).
  A line reads, with synthetic names,
  `ConsistsOf=plain.service "app-two\\x2dwords@autostart.service"`; the
  second name is `app-two\x2dwords@autostart.service`, its `\x2d` part of
  the name and passed back to `systemctl` as it is. Plain properties
  (`Id=`) are printed bare. `show -p` of an unknown property prints
  nothing and exits 0.
- **Five properties stop a unit's dependents with it** (`man
  systemd.unit`, systemd 262): `Requires=` (`RequiredBy`), `Requisite=`
  (`RequisiteOf`), `BindsTo=` (`BoundBy`), `PartOf=` (`ConsistsOf`), and
  the unit's own `PropagatesStopTo=`; `Wants=` does not.
- **The `apps` frame with `expand: true`** (one event line, bytes on the
  socket), from a daemon on a fixture root and `subscribe apps` with each
  params: `root-desktop` (44 processes, 24 groups) 6 727 bytes with no
  params, 11 798 with `{"limit": 1000, "expand": true}`; a scratch copy
  of `root-desktop` with its processes cloned nine times (422 processes,
  69 groups, made by a throwaway script, not committed) 14 744 with no
  params, 19 722 with `limit: 1000`, **68 565 with `{"limit": 1000,
  "expand": true}`**, about 160 bytes a process: 34 KB/s at 2 s. On the
  development machine, the sampler run once in-process on `/` (read only,
  no daemon): 269 processes, 78 groups, 55 537 bytes. Decision 1 stands:
  `expand: true` costs bytes and a parse, not a second walk. The
  interface's CPU while the page is shown is measured in 3b.
- **The daemon's CPU with one `process` subscriber at 1 s** (release
  build, fixture root, `live: true`, 37 events): no clock tick in 30 s,
  under 0.04 % of a core.

Deviations from the brief, each with its reason:

- **`accepts` is a function the slot keeps**, not a method called on
  the sampler: `subscribe` would otherwise wait on the `apps` sampler's
  lock, which a `get apps` holds for half a second and the inspector
  across `pacman`.
- **`start` is an optional param of `process`.** Without it the stream
  pins the process first seen at the pid (an empty pid stays gone); with
  it the subscriber says which process it means. 3b knows each row's
  start (D5) and should send it: a pin alone cannot tell two subscribers
  of one pid apart across a reuse.
- **`capped`.** The brief caps the pids at eight but does not say what
  the ninth subscriber gets; it gets `capped: true`, not a false `gone`.
- **`name` is `comm`.** The `apps` list's name may come from `cmdline`
  when `comm` was cut short; the stream reads `stat` and `statm` only, as
  designed.
- **A `get process` goes through `wants`.** `snapshot` takes no params,
  so the `get` hands its own over first; the next tick hands the
  subscribers' back. A `get` never moves the pins or the baseline, and
  uses the subscribers' baseline only when it holds that process
  (otherwise its own second reading 500 ms later).
- **`alsoStops` entries are objects** (`{unit, pids}`), not bare names:
  the CLI's "(N processes)" and 3b's dialog need the processes.
- **No `--value`** on `systemctl show` (measured above).
- **A `systemctl show` value that cannot be read** (by the quoting rule
  above), or a name in it that mirqab would not pass back, makes the
  whole question fail (`alsoStopsUnknown`), not the stop refused, as the
  brief's Rejected "Refusing a service whose dependents cannot be listed"
  says. Every name systemd allows reads; an odd one means the format
  changed (review of PR #18, DROP 1).
- **The fixture answer carries the state** (`<unit> <ActiveState>` a
  line, where the brief said one unit a line), so "an inactive dependent
  left out" is tested through the same filter as the real answer.
- **`warnings` is always present** in the dry-run reply and the
  `needs-confirm` data (empty when there is none), beside `skipped`.
- **The session-bus list lives in `guard.rs`**, as the brief's Q2
  suggested; the refusal for a signal is in `act::refusal` with the
  other fixed refusals.

From the review of PR #18 (2026-11-14; the ruling kept 8 items, deferred
4 and dropped 9):

- **K1, systemd's quoting.** Read word by word, a quoted element such as
  `"app-two\\x2dwords@autostart.service"` failed the unit-name check and
  the stop became `alsoStopsUnknown` with no refusal run on that unit.
  `act::show_words` (`act.rs:122`) reads the rule above and decodes no
  `\xNN`; `dependents_named` returns `Result` and checks each name there;
  `dependents_via` (`:199`) holds what `Real` asks and checks the unit
  first (`Real` did not). `unit_like` is shared (`pub(crate)`, `:106`)
  and `actions.rs::unit_name_ok` (`:376`) is written on it. Tests:
  `show_words_follow_systemds_quoting_and_decode_no_escapes`,
  `dependents_are_read_from_every_stop_property_quoted_or_not`,
  `dependents_via_asks_twice_and_keeps_the_active_ones` in `act.rs`;
  `quoted_escaped_names_from_systemctl_are_found_and_refused` in
  `tests/actions.rs` (a sender answering through `dependents_via` with a
  line in systemd 262's format: both names found, then the quoted scope
  holding the caller refuses the stop).
- **K2, `RequisiteOf` and `PropagatesStopTo`** are asked for and read.
  Tests: the two `act.rs` tests above (`WantedBy` ignored; five `-p`
  flags).
- **K3, a dependent of any unit type.** `group::host_units`
  (`samplers/apps/group.rs:142`): the slices under the user manager and
  the first unit that is not one; `host_service` is its last part when a
  `.service` (`:129`). `unit_members` matches on it, so a dependent
  scope's or slice's processes are listed and checked. `ordered` returns
  nothing for no members (`actions.rs:519`): a dependent with no process
  (a target) used to reach `group::main_process` with an empty list and
  panic the client's thread, found by the K1 test. Tests:
  `a_dependent_of_any_unit_type_is_found_and_checked` (a scope holding
  the caller refused, with a backslash in its name; a scope's pids
  listed; a slice holding the daemon refused) and `host_units` in
  `group.rs`'s `units_under_the_user_manager`.
- **K4, a confirmation does not stop a dependent it did not show.**
  `Plans::make` keeps the dependents the plan showed and `Redeemed`
  returns them (`plan.rs:42`, `:84`); `Resolution::unit_also`
  (`actions.rs:315`); at redeem, before any step, a dependent not shown
  is `strings::plan_dependents_changed` and a list that cannot be told
  now, when the plan's could, `strings::plan_dependents_unknown`
  (`actions.rs:883`, `:891`): refused, the plan spent, nothing run, no
  event. A plan whose list was unknown takes what is there. Tests:
  `a_confirmation_does_not_stop_a_dependent_it_did_not_show` (a new
  dependent refused and the plan then unknown; an unreadable answer
  refused; a shown dependent gone inactive still stops),
  `the_dependents_a_plan_showed_come_back_at_redeem` in `plan.rs`.
- **K5, `dbus-daemon.service`** joins `guard::SESSION_BUS`: where the
  reference bus is installed under that name, `dbus.service` is only its
  alias (from the ruling's memory, see Unverified). Tests: the guard's
  list test, `the_session_bus_is_never_signalled` in `act.rs` and
  `the_session_bus_is_never_stopped` in `tests/actions.rs` (pid and
  group).
- **K6, a pin does not outlive its subscribers.** `Sampler::idle`
  (`sampler.rs:74`): the loop marks a stream idle when it finds no
  subscriber after a sample (`server.rs:572`), takes the mark when the
  next sample is due (`:594`) and calls `idle` under the sampler's lock
  before `wants` (`:613`); no lock inside another. `Process::idle`
  (`process.rs:275`) drops the pins and the baseline, and a `get` reads
  no pin (`data`'s `pinned`). No time-based expiry: an interval has no
  upper bound. Tests: `a_pin_does_not_outlive_its_subscribers` in
  `tests/apps.rs`; `a_stream_left_without_subscribers_is_told_before_its_next_sample`
  in `protocol.rs` (the `Counted` stream counts `idle` calls: one), and
  the `process` test now checks that the subscriber's event after a `get`
  has a number in `cpu`.
- **K7, a `get` of a missing pid does not wait** half a second holding
  the stream's lock (`process.rs:287`). Test:
  `a_get_of_a_missing_pid_does_not_wait`.
- **K8, the docs.** Architecture §5.2 (the dry-run example), §5.3
  (`Sampler::idle`; the `process` row's pin, `capped` and `get`), §5.4
  (five properties, any unit type, the plan's dependents,
  `dbus-daemon.service`); this section, "Known limits (part 3a)" and
  "Unverified".
- **Counts after the fixes.** `tests/actions.rs` 22, `tests/apps.rs` 18,
  `protocol.rs` 23, `mirqab-core` unit tests 60, `mirqab` 19,
  `mirqab-proto` 18, `tests/test-cli.sh` 193 checks.

Left for 3b or later: everything under 6e–6i; the interface's CPU and RSS
with the page shown; the inspector's use of `process` with `start`; the
three Unverified items about Broadway (the dialog in the dump, `ListView`
activation, app icons there).

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
- Whether an `adw::AlertDialog` presented on the `adw::ApplicationWindow`
  is reached by `first_child` / `next_sibling` from the window, so the
  selftest's dump shows its body. If not, the dump adds the window's
  visible dialog explicitly. Check under Broadway before writing 6h.
- Whether `gtk::Widget::activate` on a `ListView` item under
  `NoSelection` emits `activate` for its position; otherwise the step
  calls the list view's `list.activate-item` action.
- ~~`systemctl --user show -p RequiredBy -p BoundBy -p ConsistsOf --value`
  prints three lines in that order~~ Resolved 2026-11-14
  **[machine]**, systemd 262.1: the properties come in systemd's own
  order whatever the order of `-p`, so 3a reads `Key=value` lines without
  `--value` ("As built (part 3a)"). Not checked on systemd 257.
- ~~The frame size with `expand: true` at 400 processes~~ Resolved
  2026-11-14: 68 565 bytes at 422 processes on a cloned fixture, 55 537
  at 269 on the development machine ("As built (part 3a)"); small, so
  decision 1 stands. The interface's CPU with it is measured in 3b.
- Whether the icon theme under Broadway in CI has any app icon; the
  selftest asserts the fallback glyph there, and the theme path is an
  owner check.
- The quoting of `systemctl show` list values on systemd 257 (checked on
  262.1 only; review of PR #18).
- That a distribution shipping the reference bus (dbus-daemon) names its
  user unit `dbus-daemon.service` with `dbus.service` as an alias (said
  of Fedora by the review of PR #18 from memory; the development machine
  runs dbus-broker). Both names are refused either way.
