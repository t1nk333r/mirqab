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
- **State**: DONE (2026-11-15). Parts 1–4 of 4: sampler, inspector,
  read CLI and MCP; Quit and Force quit, `mirqab quit`; the daemon side
  of the page: the `process` stream, start times, the units a stop takes
  with it, the session bus refused; the page, its inspector, the
  `apps.top` card; the focus source, the "idle but heavy" tag, the
  `top-apps` minute row and the 400-process benchmark (4a); the tag on
  the page, Escape, the press fix, start times from the page, the narrow
  focus credit and the GPU row's cap (4b). Every headless check passes.
  **Owner checks still open** (they need the real session): the look
  and feel on Hyprland (acceptance part 3, 1–6: theme icons, sorting and
  clicking under a moving CPU order, the sheet overlaid and beside the
  list, the popover, Top apps on Home, the selected row's contrast); the
  tag on a real busy app (`yes > /dev/null &` for five minutes, cleared
  by a click on its terminal) and `focus: hyprland` with the page open,
  none closed (acceptance part 4, 1–2); a Hyprland restart with the
  daemon running (part 4, 3); `top-apps` rows after a busy ten minutes
  (part 4, 4); Escape and the press on the real desktop (part 4, 6–7),
  including Escape with focus in the search entry (the entry clears
  itself first) and the press through GTK's real event dispatch, which
  the tests drive only through the shortcut's action and `Handle::press`;
  the row's selected state read back by a screen reader (Orca) after a
  reordering frame (the tests check what the page wrote, "Review of
  PR #21", item 2); the interface's CPU and RSS with Apps open on
  Wayland (the Broadway numbers are in "As built (part 3b)"); and
  whether a Flatpak app's window reports its host pid ("Unverified
  (part 4)")

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
  and the `stop` is still not seen. The unit itself is kept too: a
  confirmation that would stop another unit, or a unit where the plan
  showed signals, or signal where it showed a unit, is refused (audit).
- ~~The session bus is recognised by its unit name only
  (`guard::SESSION_BUS`); a bus started by hand in some other unit
  (`dbus-run-session`, `dbus-launch`) is not.~~ Closed by part 4a (owner,
  2026-11-14, Q1): it is refused by its command name too
  (`guard::SESSION_BUS_COMMS`).


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
6. [x] The page (part 3; design in "Part 3: the page (design)").
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
   - 6e. [x] `crates/mirqab-view/src/tabs/apps.rs`: `AppsState`, `rows`,
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
   - 6f. [x] Client: `set_merge`, `Source::request`,
     `subscribe_live(stream, params, …)`, `graph::Live.params`. (3b)
     - Deferred from the review of PR #18 (nit): `process` accepts a
       thread id and reads that thread's own `stat`; harmless and read
       only. The client sends `pid` and `start` from a row, never a typed
       number; one sentence in architecture §5.3 when 3b touches it.
   - 6g. [x] `crates/mirqab-ui/src/tabs/apps/{mod,page,group_row,process_row,inspector,top_card}.rs`;
     `tabs::build` and `tabs::card`; the pin control. (3b)
   - 6h. [x] `--selftest … --do`, `tests/test-ui-apps.sh`, the widget
     tests, `tests/test-ui-home.sh`. (3b)
   - 6i. [x] Architecture §7.2 (app icons), §7.5 (selftest steps);
     `docs/user-guide/` Apps page; "As built (part 3)" with the measured
     costs. (3b)
7. [x] Focus tracking: `crates/mirqab-core/src/desktop/hyprland.rs` (event
   socket reader, active only while `apps` has a subscriber), "idle but
   heavy" (part 4; design in "Part 4: focus, 'idle but heavy', the cost
   (design)").
   - Deferred from the review of PR #18 (bus outside a unit): owner
     question 1 of part 4; decision 15 if yes.
   - Deferred from the review of PR #17 (D2): the compositor probe's
     blocking connect → decision 11, 4a.
   - Deferred from the review of PR #19 (S4): `Subject::parse` and
     `{pid, start}` → decision 8, 4a and 4b.
   - Deferred from the review of PR #19 (S5): `apps.inspect` with
     `start` → decision 9, 4a and 4b.
   - Deferred from the review of PR #19 (S8): rows shifting under a
     press → decision 16, 4b.
   - Deferred from the review of PR #19 (N3, N8, N4): → decision 16, 4b.
   - Deferred from the review of PR #19 (N6): stays deferred to the plan
     that adds reconnects (decision 16).
   - Owner question (review of PR #19, S7): Escape → owner question 2,
     decision 16, 4b.
   - 7a. [x] `sys::connect_timeout`; `compositor_sockets` uses it;
     `runtime_dir` moves to `desktop/mod.rs`. (4a)
   - 7b. [x] `desktop/{mod,hyprland}.rs`: the `Focus` trait, instance
     discovery, the reader thread, `FocusState`, the request cap and the
     error cap; `tests/focus.rs` with the fake Hyprland. (4a)
   - 7c. [x] `samplers/apps/focus.rs`: `credited`, `Thresholds`, the
     per-group CPU windows; `Apps::wants` starts and `idle` stops the
     reader; `AppGroup.idleHeavy`, `AppsData.focus`, `status.focus`;
     `MIRQAB_HEAVY_WINDOW_MS` under `MIRQAB_TEST=1` on a fixture root.
     (4a)
   - 7d. [x] S4 and S5 in `actions.rs` and `mod.rs`; `strings::apps_process_gone`.
     (4a)
   - 7e. [x] D1: `Sampler::prime` and `Answer`; `Owners` behind its own
     lock; the server's `get` and query paths. D2 (GPU): `fd/` first,
     the 1024 cap. D7: `.flatpak-info` as a regular file through
     `read_prefix`. (4a)
   - 7f. [x] The session bus by `comm` (owner, 2026-11-14: yes). (4a)
   - 7g. [x] CLI: the `idle` column in `mirqab apps`, the focus line in
     `mirqab status`; `top_apps` carries `idleHeavy`. (4a)
   - 7h. [x] 4b: the caption and the sheet line; Escape; S8; N3; N8; N4;
     `start` in `args` and in the inspect params; `APPS_ROW_GONE` on the
     gone error. (4b)
     - Deferred from the review of PR #20 (S4): the focus credit narrowed
       (owner decision 6, 2026-11-15). (4b)
     - Deferred from the review of PR #20 (nit 3): the GPU row's cap on
       DRM `fdinfo` reads, not on the `fd/` listing. (4b)
   - 7i. [x] Architecture §5.2, §5.3, §5.4, §6.1, §7.2, §7.5, §9.1, §12;
     "As built (part 4a)" and "(part 4b)"; the plan's state; the README
     row. (4a: §5.2, §5.3, §5.4, §6.1, §6.3, §9.1, §12, "As built (part
     4a)"; 4b: §5.3's focus rule, §7.2, §7.5, "As built (part 4b)", the
     state, the README row)
8. [x] Tests above (part 1 wrote the read side's; part 4's table is in
   "Part 4 … Tests").
   - D6: [x] `crates/mirqab-core/tests/protocol.rs` (3a).
   - Deferred from the review of PR #16 (D7): `.flatpak-info` → 7e, its
     test in `tests/apps.rs`. (4a)
   - Deferred from the review of PR #19 (gap 5): the client's merge when
     a handle drops → a unit test. (4b)
   - Deferred from the review of PR #19 (gap 6): the `process`
     subscriber's params in the Broadway test. (4b)
   - Deferred from the review of PR #19 (gap 8): `rows()`'s `selected`
     flags. (4b)
   - Deferred from the review of PR #19 (N4, nit): the expander during a
     member-only search → decision 16, with its view-model test. (4b)
   - 8a. [x] `tests/focus.rs`; the focus, tag, minute-event, S4, S5, D1,
     D2, D7 rows of the table in `tests/apps.rs`, `tests/actions.rs`,
     `tests/protocol.rs`, `sys.rs`, `config.rs`. (4a)
   - 8b. [x] `tests/test-bench.sh` (`--procs`), `tests/test-fixtures.sh`
     (`--clones`), `tests/test-cli.sh` (`idle` column, `config set`,
     a bare pid still sent). (4a)
   - 8c. [x] `crates/mirqab-view/tests/apps.rs`, `crates/mirqab-ui/tests/widgets.rs`,
     `tests/test-ui-apps.sh` with the fake Hyprland. (4b)
9. [x] Measure `--subscribe apps` and the top-apps minute cost; record
   both (part 4; decision 12).
   - Deferred from the review of PR #16 (D1): measured (the late frame),
     then fixed → 7e.
   - Deferred from the review of PR #16 (D2): the inspector's `fdinfo`
     reads, measured with the inspector's cost → 7e.
   - Deferred from the review of PR #18 (nit): the eight-pid cap keeps
     the first eight; decided in decision 12, no change.
   - Deferred from the review of PR #19 (N10): one `apps.inspect` per
     click; with D1's `Later` the lock is not held across pacman, so a
     click costs the interface one request and the daemon one fast read;
     measured in 4a, and a debounce is added only if an inspect exceeds
     50 ms on the development machine.
   - Deferred from the review of PR #19 (N9): the selected row's
     contrast, an owner check on Hyprland (acceptance part 3, item 1).
   - 9a. [x] `tools/mkfixture.sh procs --clones K`; `tools/bench-idle.sh
     --procs N`; the CI line `bench --subscribe apps --max-cpu 1.5
     --procs 400`. (4a)
   - 9b. [x] `Sampler::minute_events`; `Apps::minute_events` with the
     leader rule and the repeat rule; `trends.topApps` in `config.rs`,
     the schema, `docs/cli.md`; `history_loop` reads the key; the 30-day
     fill test with a row a minute. (4a)
   - 9c. [x] The measurements of decision 12 on the development machine,
     in "As built (part 4a)"; the `trends.topApps` default flipped if
     the idle run is over budget (it is not: 0.006 %; the default stays
     on). (4a)

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
  shells only stays with the terminal. Code given inline (`python3 -c`,
  `perl -e`, `node -e`/`--eval`, `deno eval`) or a script argument that
  is not a plain file name of at most 64 bytes names the job after the
  interpreter, and an option's separate value (`-W ignore`) is skipped:
  the name reaches the top-apps row and agents (`script_of`, audit of
  2026-11-21). Rule 5 climbs only through other
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
  `the_unit_and_dependents_a_plan_showed_come_back_at_redeem` in
  `plan.rs`. **Audit fix:** `Plans::make` keeps the unit the plan showed
  apart from `also`, whose None meant both "no unit" and "dependents
  unknown", so a pid planned for `TERM` and moved into a user service
  before the confirmation was stopped through that service. At redeem,
  before the dependents check, a unit that differs from the planned one
  (another unit, a unit for signals, or signals for a unit) is
  `strings::plan_unit_changed`: refused, the plan spent, nothing run, no
  event. Test: `a_confirmation_does_not_stop_a_unit_the_plan_did_not_show`.
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

## As built (part 3b)

Part 3b (2026-11-14) is checklist steps 6e–6i: the interface side of
"Part 3: the page (design)". What was built, with where it lives:

- **The view model** (`crates/mirqab-view/src/tabs/apps.rs`). `CARDS`
  (`:24`) holds `apps.top`, chained first into `cards::all()`;
  `home::SEED` gains it; `shell::TABS` marks `apps` built, so the tile
  hides where `apps` is not served. `Keys::assign` (`:144`) gives a group
  the previous frame's key only while the two share a process
  (`Members::share`: pid and start when both frames list processes, pid
  otherwise); System keeps its key. `AppsState::take` (`:250`) assigns
  keys, prunes open groups, resolves a pending group from the card, and
  reports a lost selection. `sorted`/`sorted_processes` (`:314`, `:323`)
  are the daemon's order with System last; `rows` (`:470`) searches
  (group name, a member's name, a pid by its first digits; a group that
  matches only by members is shown open with those members), expands and
  flattens, and ends with `apps_more` or `APPS_EMPTY`. `main_pid`
  (`:534`), `top_five` (`:559`), `merge_params` (`:583`), `selection`
  (`:649`: what to inspect, the `args`, the live graph's pid and start,
  the targets, "not yours", "holds the interface"), `confirm_lines`
  (`:720`), `start_differs` (`:779`), `result_lines` (`:788`),
  `not_recorded`, `deeper` (`:702`) and `inspect_rows` (`:829`). Glyphs:
  `EXPAND`, `COLLAPSE`, `REFRESH` and one per kind in `icons.rs`; strings
  in the "Apps page" section of `strings.rs`.
- **6e's deferral (S3, second half).** A dependent that is no service,
  scope or slice reads "and what depends on it", in the dialog
  (`apps_confirm_also_stops`) and in `mirqab quit` (`quit_also_stops_line`
  gains `deeper`; `crates/mirqab/src/quit.rs:43`, its test checks a
  `.target`).
- **The client** (`crates/mirqab-ui/src/client.rs`). `set_merge` (`:208`)
  stores a `fn(&[Value]) -> Value` per stream and `wanted` (`:55`) calls
  it over the handles' params; `app.rs:126` registers
  `apps::merge_params` for `apps`. `Source` gains `request`, `get` (for
  `get apps.inspect`, which carries a stream) and live params
  (`subscribe_live(stream, params, …)`, `:139`–`:143`); `graph::Live`
  gains `params`, and `widgets/graph.rs` stays the one caller (lint
  unchanged). 6f's deferral: the inspector sends `{"pid", "start"}` from
  the selected row (`inspector.rs`, `selected`), never a typed number;
  architecture §5.3 says so.
- **The page** (`crates/mirqab-ui/src/tabs/apps/`): `page.rs` (toolbar
  with search, sort `DropDown`, "Show processes" switch and the pin
  control; a `ListView` over a `gio::ListStore` of `BoxedAnyObject` rows
  under `NoSelection` with single-click activate; each frame writes row
  `i` into object `i`, redraws only bound items whose row changed, and
  grows or shrinks the store at its end, `refresh` `:135`; the
  `OverlaySplitView` in an `adw::BreakpointBin` that overlays the sheet
  under 60 em, `:397`; `apps` subscribed on map with
  `{"limit": 1000, "expand": true}` and dropped on unmap, `:415`);
  `group_row.rs`, `process_row.rs` (one widget each, rebound per
  position); `inspector.rs` (header, verdict, graph, stats, the
  `apps.inspect` rows asked once per selection and on refresh, Quit and
  Force quit: `act` `:408`, `confirm` `:439`, `redeem` `:475`);
  `top_card.rs` (five flat buttons; a press sets
  `ctx.apps.pending_group` and shows the tab); `mod.rs` (`set_app_icon`,
  `:32`: `gtk::IconTheme::has_icon` by name, else the kind's glyph).
  `Ctx.apps` keeps the `AppsState` (decision 13). `tabs::pin_button` is
  the pin control for a card id, shared with System's cards
  (`Card::set_pinnable` went).
- **The selftest** (`crates/mirqab-ui/src/selftest.rs`). `--do STEPS`
  (refused with exit 64 unless `MIRQAB_TEST=1`, `lib.rs`): `type`,
  `activate`, `respond`, `wait`, `pause`, run by `step` (`:103`); each
  step that names a widget waits up to 5 s for it, looking every 50 ms
  with one-shot timers. Architecture §7.5 has the paragraph and the
  widget names.
- **Docs.** Architecture §5.3 (the client sends a row's pid and start),
  §7.2 (app icons from the theme, by name), §7.5 (`--do`); README's
  status list has the Apps tab and the four-card seed.

Tests:

- `crates/mirqab-view/tests/apps.rs`, 13 tests: each sort equals
  `mirqab_core::samplers::apps::view` on the golden frame (groups and
  processes; `mirqab-core` is a dev-dependency of the view crate, both
  headless); search by group, member and pid; open and closed groups,
  the expander, flatten with group names and System last; the "more"
  line (`limit: 3`: 20 more, System outside the limit); `main_pid`;
  `top_five` with and without a heavy System; `merge_params`; keys
  across frames (a shared member keeps the key; the same `session:` id
  with no shared member is a new, closed, unselected row; a frame with
  no processes keeps the keys; a pid with a new start is a new row); the
  pending group; `selection`; `confirm_lines` (signal, `via`, a stop
  with pids, `alsoStops`, a `.target`, `alsoStopsUnknown`, Force quit's
  `kill`, the left-alone block, warnings, control characters as U+FFFD);
  `result_lines` and `not_recorded`; `icon_name` and `deeper`. The view
  crate's tests: 55 in all; `crates/mirqab-view/tests/shell.rs` and
  `home.rs` follow the built tab and the seed.
- `crates/mirqab-ui/tests/widgets.rs`, 7 under Broadway (2 new): the
  inspector with a fake `Source` (a process is graphed live with its pid
  and start and inspected once; a group drops the live graph and feeds
  it from frames; Quit's first call has no `confirm`; the dialog's body
  lists all five pids; Go ahead sends `confirm` and the plan; a refusal
  is the message and no second dialog; System shows no buttons; live
  only while mapped; no blur), and a group row's name and its meter.
- `tests/test-ui-apps.sh` (new), 52 checks: 24 groups, System last; the
  browser row shows the glyph and no image; no inspector before a
  selection; a search leaves one group; selecting it shows the inspector
  with "Packaged by core/bash" (its main process, pid 2000); Quit's
  dialog is in the dump and names pids 2000, 2001, 2010, 2011, 2012, and
  the act log is empty; `respond confirm` writes the five TERM lines
  (main process first) and "Done."; Quit on pid 1050 shows "pid 1050 is
  the session's compositor…", no dialog, nothing sent; pid 410 shows no
  Quit and `APPS_NOT_YOURS`; `--do` without `MIRQAB_TEST=1` exits 64; one
  `process` subscriber while process 2001 is selected and none after;
  none with a group selected; one `apps` subscriber with the tab open and
  none after `close`; no GTK criticals.
- `tests/test-ui-home.sh`, 54 checks: four seeded cards, the Top apps
  card with five rows, a press on cargo's row opens Apps with cargo in
  the inspector; the placeholder step moved from `apps` to `fans`.

Measured (2026-11-14, release build, fixture daemon on `root-desktop`,
Broadway with the Cairo renderer, 60 s after 10 s of settling; CPU from
`utime + stime` of `/proc/<pid>/stat`, `VmRSS` from `status`):

| Window shows | Interface CPU | Interface RSS | Daemon CPU | Subscribers |
|--------------|---------------|---------------|------------|-------------|
| Apps, nothing selected | 0.033 % of a core (2 ticks) | 61.3 MiB | 0.033 % | `apps` 1 |
| Apps, process 2001 selected (live graph) | 2.2 % | 208 MiB | 0.033 % | `apps` 1, `process` 1 |
| System alone (for comparison) | 2.0 % | 207 MiB | 0.033 % | `system` 1 |
| Closed | — | — | — | none |

The Apps list itself costs next to nothing between frames. A live graph,
on either page, costs the same 2 % and some 150 MiB more under Broadway,
which ships every redrawn surface to its client; on Wayland plan 001
measured the open window at 50 MiB, so the Hyprland figures are an owner
check. The `apps` frame sizes are 3a's.

Deviations from the design, twelve, each with its reason:

- **No `docs/user-guide/`** exists in the repository; the Apps page's
  text went into README's status list, as plans 002 and 003 did for
  System and Home.
- **`Source::get`** beside `request`: `get apps.inspect` carries a
  stream, which `request(method, params)` cannot.
- **The 410 check.** The design's test row says "Quit on pid 410 shows
  the refusal in words and no dialog", but 410 is another user's process,
  and decision 11 hides the buttons there. The test checks that (no
  Quit, `APPS_NOT_YOURS`) and checks the refusal in words on pid 1050,
  the compositor.
- **More steps and names**: `pause MS` (the subscriber checks need the
  interface to stay up with a process selected), `apps-verdict`,
  `apps-flatten`, `apps-dialog`, `apps-notice` and `apps-top:<id>`.
- **`inspect_rows` moved into the view model** from `mirqab what`'s
  `what_lines`, so the inspector and the CLI print the same lines. The
  inspector passes `private: false` (no working directory, command line
  or paths), as the design's list of rows has none of them.
- **Home is ready once every stream has spoken**
  (`tabs/home/page.rs`, `seen`): with `apps.top` seeded, Home has cards
  on two streams, and the first `apps` event alone let the selftest dump
  Home with the System cards still empty.
- **Unexpanded frames.** A frame cut for the card can arrive while the
  page's subscription replaces the card's; the page skips frames with no
  processes (`apps::expanded`), and keys fall back to pids when a frame
  lists none, so neither the open groups nor a process's selection is
  lost to one (found by `tests/test-ui-home.sh`'s card press).
- **The sheet is hidden when nothing is selected**, not only slid away:
  an overlaid `OverlaySplitView` sidebar stays mapped, so the dump showed
  it, and its graph would have stayed mapped.
- **A lost selection** closes the sheet and says `APPS_ROW_GONE` on a
  line above the list (`apps-notice`) until the next selection; the
  closed sheet cannot hold it.
- **Memory beside the graph** is the stats row under it, from the 2 s
  frame; the live event's `rssBytes` is not shown.
- **Sizes in em become px at build time** for the sheet's width (20 to
  26 em) and the 60 em breakpoint: `adw::BreakpointCondition` takes px,
  pt or sp, not em.
- **Go ahead is destructive for Quit too**: `confirm_dialog` sets it for
  every action, and the design's "as confirm_dialog already sets" was
  kept rather than adding a second appearance.

Known limits (part 3b):

- `APPS_QUIT_SELF` matches `std::process::id()` against the frame; on a
  fixture root that pid is meaningless (the test asserts nothing about
  it).
- The "still running" line follows the 2 s frames after a Quit, so it
  shows some 4 s after the Quit at the earliest.
- The measurements above are Broadway's.
- `start_differs` sees a reused pid only in `signal` entries, so a pid
  recycled inside a `--user` service passes to the dialog, which names
  the unit (review of PR #19, S4, deferred to step 7).
- `apps.inspect` is asked by pid alone, so a reused pid shows the new
  process's verdict under the row's name until the next frame drops the
  row (review of PR #19, S5, deferred to step 7).

From the review of PR #19 (2026-11-14; the ruling kept 10 items, deferred
13 and dropped 4):

- **K1 (B1), an act's outcome outlives its row and its sheet.**
  `apps::Outcome`, `outcome`, `outcome_notice` (`apps.rs:833`, `:841`,
  `:856`; `strings::apps_outcome`). `redeem` (`inspector.rs:571`) always
  builds the outcome: under its own row it shows in the sheet and is kept
  for `take_outcome` (`:355`); when the selection moved on before the
  reply it goes to `on_outcome` (`:591`). The page writes it into
  `apps-notice` when the row goes (`page.rs:131`, `say_outcome` `:232`),
  with the `warning` class when unrecorded; `APPS_ROW_GONE` is left for a
  row that went with no act. Tests: `an_outcome_outlives_its_row_and_says_when_unrecorded`
  (view); the widget tests `the_inspector_asks_before_it_quits_and_says_a_refusal_in_words`
  (`warned()`, then a frame without the browser and `take_outcome` with
  `warn`) and `one_act_one_dialog_and_go_ahead_sends_what_it_showed`
  (a held reply after the selection moved: `on_outcome` once, with the
  acted key); `tests/test-ui-apps.sh` on a copy of the fixture root,
  whose five browser `proc/<pid>` directories are removed once the TERMs
  are recorded: the row is gone, the sheet closed, and `apps-notice`
  says "Example Browser: Done.".
- **K2 (S1), no `--do` and no forwarded `--selftest` without a test
  run.** `Opts::refusal(first, test_env)` (`app.rs:67`; `test_env`
  `:79`; `strings::UI_SELFTEST_NOT_FORWARDED`) is checked in `run()`
  (`lib.rs:37`), in the command-line handler before anything is built
  (`lib.rs:63`), and in `selftest::run` (`selftest.rs:32`). Architecture
  §7.5 says so. Tests: the unit test
  `steps_need_a_test_run_and_a_selftest_never_joins_a_running_interface`;
  `tests/test-ui-apps.sh` runs a unique interface without `MIRQAB_TEST`
  on a private bus (`dbus-run-session` with a config that has no service
  directories) and sends `org.gtk.Application.CommandLine` built with
  GLib's variant API (`python3 -I` with `gi`, written by the test):
  `--tab apps` is 0, `--selftest apps --do "<select 2001, quit,
  respond confirm>"` and `--selftest apps` are 64, the interface stays
  up, dumps no tree, and the act log stays empty. Skipped with a note
  where `dbus-run-session` or `gi` is missing.
- **K3 (S2), Go ahead sends what the dialog showed.** `confirm` captures
  a `Pending` (`inspector.rs:83`: plan, key, args, name, targets) and
  `redeem` sends it; only the dialog open now answers (`:553`), and
  `show(None)` and a new selection close it (`close_dialog`, `:365`).
  Test: the second widget test (the row goes, then another row is
  selected, with a dialog open: closed, and its Go ahead sends nothing).
- **K4 (S3), one act in flight, one dialog.** `busy` and the open dialog
  stop `act` (`:483`); both buttons go insensitive while busy; any
  response clears the dialog. Test: two Quits and a Force quit send one
  request and keep the first dialog; cancel sends nothing and frees the
  next act.
- **K5 (S6), "still running" watches what was signalled.**
  `apps::watched` (`apps.rs:871`): the `done` signal entries' pid and
  start, and for a stop the targets the daemon did not leave alone;
  `redeem` watches it (`inspector.rs:597`). Tests:
  `a_quit_watches_only_what_was_signalled` (view) and `watching()` in
  the first widget test.
- **K6 (S7), no hide gesture.** `set_enable_hide_gesture(false)`
  (`page.rs:370`), and anything else that hides the sheet while a row is
  selected clears the selection (`connect_show_sidebar_notify`, `:413`).
  Test: `hiding_the_sheet_clears_the_selection` (the page built on a
  `Ctx` with a dead socket).
- **K7 (gap 2), the dialog's safe defaults.** Test:
  `the_confirm_dialog_defaults_to_cancel`, and the presented dialog's
  default and close responses in the first widget test; cancel sends
  nothing in the second.
- **K8 (gap 3), `start_differs` through the interface.** Test: a plan
  naming 2001 with start 9999 opens no dialog, sends nothing more, and
  says `APPS_ROW_GONE`.
- **K9 (N1), an entry of unknown shape is shown as sent**
  (`apps.rs:756`, `shell::would_do_line`). Test:
  `an_entry_of_unknown_shape_is_shown_as_sent`.
- **K10, docs.** The deviation count (twelve), the two Known limits
  above, and the deferred items at checklist steps 7, 8 and 9.
- **Counts after the fixes.** `crates/mirqab-view/tests/apps.rs` 16;
  widget tests 10 under Broadway; `mirqab-ui` unit tests 2;
  `tests/test-ui-apps.sh` 67 checks.

Left for the owner to check by eye on Hyprland (acceptance part 3, 1–6):
icons from the real theme; sorting and search under a changing CPU
order; the sheet overlaid at the default 58 em and beside the list in a
wider window; the process graph moving each second and `mirqab status`
showing no `process` subscriber after hiding; Quit on a `sleep 1000`, on
a root process, on a `--user` service with a dependent, and on mirqab's
own row; the dialog and sheet in a popover; Top apps on Home; and the
interface's CPU and RSS with Apps open on Wayland.

## Part 4: focus, "idle but heavy", the cost (design)

Planner's brief, 2026-11-14 (Fable). Evidence lines are from
`origin/plan-006-apps-part3b` at 2778563 (PR #19, approved) for the
interface and the plan, and from `main` at b36e8e2 for the daemon, which
PR #19 does not change. **[machine]** was measured on the development
machine on 2026-11-14 (Hyprland 0.56.2, kernel 7.2.7); **[unverified]**
was not checked. Where the build differs, "As built (part 4)" says so.

Part 4 closes plan 006: the focus source and the "idle but heavy" tag
(step 7), the tests the reviews deferred (step 8), the benchmark and the
top-apps minute event (step 9), and the review items the earlier parts
deferred to these steps. It is two pull requests, 4a (daemon) and 4b
(interface), as parts 2 and 3 were: the daemon side carries every safety
item and is reviewed for safety; the page is reviewed for the interface
rules. "One pull request or two" below says what goes where.

### What exists, and what is missing

| Need | Today | Evidence |
|------|-------|----------|
| A focus source | None. The data-source table names Hyprland's event socket and marks it unverified; nothing reads it | plan 006, "Linux data sources", `.socket2.sock` row; `crates/mirqab-core/src/samplers/apps/` has no focus code |
| Where the compositor is known | `actions::compositor_sockets` finds the `wayland-*` listener's pid by `SO_PEERCRED`, per resolution, with a **blocking** `UnixStream::connect` | `actions.rs:410-430`; deferred D2 of PR #17 |
| The runtime directory | `runtime_dir`: `$XDG_RUNTIME_DIR`, else `run/user/<uid>` under the root | `actions.rs:434-440` |
| CPU over time per group | The sampler keeps one baseline (`live: Option<Tick>`) and reports the rate since the previous sample; nothing spans minutes | `samplers/apps/mod.rs:49-62`, `:361-370` |
| A sampler learning it has subscribers | `Sampler::wants` before each sample; `Sampler::idle` once after the last subscriber left | `sampler.rs:68`, `:74`; `server.rs:594-616` |
| A sampler's work with no subscriber | `Sampler::minute` once a minute under the sampler's lock, series values only; no hook writes an `event` | `sampler.rs:85`; `server.rs:510-535` (`history_loop`) |
| Event rows | `History::add_event(ts, kind, data)`; the only writer is the `action` event | `history.rs:285`; `actions.rs:1023` |
| Config | `config::defaults` has no `trends` group; `check` refuses an unknown key | `config.rs:14-24`, `:58` |
| The benchmark | `tools/bench-idle.sh --subscribe STREAM` holds one subscription at 2000 ms; budgets 0.3 % idle, 1 % live; CI runs idle and `system --live` | `tools/bench-idle.sh:48-50`, `.github/workflows/ci.yml:125-127` |
| A 400-process fixture | `root-desktop` has 46 processes; 3a measured a nine-times clone made by a throwaway script, not committed | "As built (part 3a)", frame sizes |
| `get apps` under the lock | `snapshot` sleeps 500 ms holding the sampler's lock when no baseline is 1–10 s old | `mod.rs:376-388`; `server.rs:1031-1045`; deferred D1 of PR #16 |
| `apps.inspect` under the lock | `query` runs `pacman -Qqo` (5 s timeout) under the same lock | `mod.rs:393-406`; `inspect.rs:103-125`; D1 of PR #16 |
| GPU memory | `gpu` reads every `fdinfo/*` of the process | `inspect.rs:280-315`; deferred D2 of PR #16 |
| `.flatpak-info` | Read with `root.read`, no size or type check | `procfs.rs` (D7 of PR #16) |
| The action's subject | `Subject::parse` takes exactly one key, `pid` or `group`; a reused pid inside a service passes to the dialog | `actions.rs:112-134`; `start_differs` only sees `signal` entries (S4 of PR #19) |
| The inspector's identity | `apps.inspect {pid}`; no `start`; a reused pid shows the new process's verdict | `mod.rs:393-406`; `inspector.rs:434` (S5 of PR #19) |
| The `process` stream's cap | The first eight pids in client order; the ninth gets `capped` | `process.rs:242-250` |
| Escape | The window's `ShortcutController` (scope Managed) binds Escape to hide a popover; in window mode Escape is unbound | `window.rs:235-251` |
| The page on unmap | Drops its subscription and keeps `Page.frame`, shown again on re-map until the next frame | `page.rs:478-480` (N3 of PR #19) |
| Rows under a press | `activate(pos)` reads the store's row at `pos`; a frame between press and release can move it | `page.rs:204-220` (S8 of PR #19) |

### Hyprland's IPC, as measured

- **The directory** is `$XDG_RUNTIME_DIR/hypr/<HIS>/` with `.socket.sock`
  (requests), `.socket2.sock` (events), `hyprland.lock` and
  `hyprland.log` **[machine]**. `HIS` is `HYPRLAND_INSTANCE_SIGNATURE`:
  `<40 hex>_<10 digits>_<10 digits>` here. The variable is in the user
  manager's environment on the development machine (`systemctl --user
  show-environment` lists it and `WAYLAND_DISPLAY`) **[machine]**, but
  `mirqab.service` is `WantedBy=default.target` (`dist/mirqab.service`),
  so a daemon started at login may have started before Hyprland imported
  it, and a Hyprland restart changes it while the daemon lives.
  `hyprland.lock` holds the compositor's pid on its first line and the
  `WAYLAND_DISPLAY` name on its second **[machine]**.
- **Events** are one line each, `EVENT>>DATA\n`
  (https://wiki.hypr.land/IPC/, "Events list"). `activewindow>>CLASS,TITLE`;
  `activewindowv2>>WINDOWADDRESS`; `openwindow>>WINDOWADDRESS,WORKSPACEADDRESS,CLASS,TITLE`;
  `closewindow>>WINDOWADDRESS`; `windowtitlev2>>WINDOWADDRESS,TITLE`. No
  pid is in any event. The wiki does not say whether `WINDOWADDRESS`
  carries a `0x` prefix; `j/activewindow` and `j/clients` print
  `"address": "0x…"` (14 characters) **[machine]**, and Hyprland's source
  formats the event address with `{:x}`, without the prefix
  **[unverified]**. The reader strips an optional `0x` and compares the
  rest, so either is fine. A 60 s and a 10 s capture of the live socket
  caught no event (nothing changed focus): no real `activewindow` line was
  recorded, so the parser is written to the wiki's format and tested on
  synthetic lines (`activewindowv2>>5633a0b1c2d3`, `activewindowv2>>`
  for no window, `activewindow>>example-term,~`).
- **Requests**: write the command to `.socket.sock` and read to EOF;
  `j/` prefixes a JSON answer. `j/activewindow` answers one object with
  `pid` (a number), `address`, `class`, `title`, `focusHistoryID` (0 for
  the focused window) and 27 more keys; `j/clients` answers every window
  the same way (8 windows, 5 400 bytes, 7 distinct pids here)
  **[machine]**. A `pid` is the Wayland client's, which the compositor
  learns by `SO_PEERCRED`, so a Flatpak app's window reports its host pid
  **[unverified]**.

### Decisions

**1. One module, `crates/mirqab-core/src/desktop/hyprland.rs`, behind a
`Focus` trait.** `desktop/mod.rs` holds

```rust
pub trait Focus: Send {
    /// Starts watching; Err names why there is no source (no Hyprland).
    fn start(&mut self, root: &Root) -> Result<&'static str, String>;
    /// The pid focused now, if any, and the pids focused since `since`.
    fn focused(&mut self, since: Instant) -> FocusedSince;
    fn stop(&mut self);
}
```

and `hyprland::Hyprland` is its one implementation in this plan (the
checklist's `desktop/hyprland.rs`). The name clashes with nothing: the
desktop-entry reader is `samplers/apps/desktop.rs`, and the new directory
is `src/desktop/`, for what the compositor tells the daemon; architecture
§12 gains the line. Sway and friends are a later plan: niri and sway have
event streams of their own shape, and the trait is the seam.

**2. The reader runs only while `apps` has a subscriber.** `Apps::wants`
is the first thing the server calls when a sample is due and
`Apps::idle` the last after the subscribers left (`server.rs:612-616`),
so: `wants` with a non-empty list starts the reader if it is not running
(`Focus::start`), and `idle` stops it (`Focus::stop`). Starting is
cheap (a connect, one `j/activewindow` request) and happens once per
"page opened"; the owner's §9.2 rule 2 holds: with the window closed
there is no reader thread, no open socket and no `/proc` read for focus.
`start` is also where Hyprland is looked for, so a Hyprland started after
the daemon, or restarted with a new signature, is found the next time the
page opens.

**3. How the instance is found.** In order: `$HYPRLAND_INSTANCE_SIGNATURE`
when set and `<runtime>/hypr/<HIS>/.socket2.sock` exists; otherwise each
directory under `<runtime>/hypr/` whose `hyprland.lock`'s first line is a
pid with a `proc/<pid>/` under `Root`, the newest by directory mtime when
several (an old instance's directory can outlive it). `<runtime>` is
`actions::runtime_dir` (moved to `desktop/mod.rs` and shared with the
compositor probe). Nothing found is `Err("no Hyprland instance")`: the
tag is absent, `status.focus` is null, and nothing is logged above
debug. The lock file is read through `Root::read_prefix` (64 bytes).
*Rejected:* asking the `wayland-*` socket's peer whether its `comm` is
`Hyprland`; it still would not give the instance directory.

**4. The reader thread.** `start` connects to `.socket2.sock` with
`sys::connect_timeout(path, 500 ms)` (decision 11), asks `j/activewindow`
once on `.socket.sock` (same connect, `set_read_timeout(500 ms)`, read
to EOF, 64 KiB cap) to learn the focus now, then spawns one thread named
`focus-hyprland` (64 KiB stack) that blocks in `BufReader::read_line` on
the event socket. The state it feeds is `Arc<Mutex<FocusState>>`:

```rust
struct FocusState {
    now: Option<u32>,                 // the focused pid
    history: VecDeque<(Instant, u32)>, // pids that gained focus, 6 min kept
    by_address: HashMap<String, u32>, // window address -> pid
}
```

Per line: `activewindowv2>>ADDR` looks `ADDR` up in `by_address`; a miss
asks `j/activewindow` once (the window just focused is the active one),
caches `address -> pid`, and pushes `(Instant::now(), pid)`; an empty
`ADDR` sets `now = None`. `closewindow>>ADDR` evicts the address. Every
other event is ignored (`activewindow` carries the class and title,
which the daemon neither needs nor keeps: no window title is ever stored
or logged). A request that fails or times out leaves `now` unchanged and
counts an error; after 5 consecutive errors the reader stops and `start`
will try again on the next `wants` after an `idle`. EOF or a read error
(Hyprland exited) ends the thread the same way. `stop` is
`UnixStream::shutdown(Both)` on a `try_clone` of the event socket, which
returns the blocked `read_line` with 0 and lets the thread exit; `stop`
joins it with no timeout, as the shutdown is what wakes it. A burst of
focus changes (a script cycling windows) costs at most one request per
new address, since `by_address` answers the rest; a cap of 10 requests a
second drops the excess with `now` unchanged.

**5. Mapping a focused pid to app groups.** A focus credits more than the
group that holds the pid, because rule 4 of "Grouping" splits a
terminal's scope into session groups: the user looking at a terminal
whose shell runs `cargo` has `cargo` in view. So a focused pid `P`
credits every group that

- contains `P`, or
- has a member in the same host unit as `P` (`group::host_units`,
  `group.rs:142`; the terminal's `app-…foot….scope` and the `session:`
  groups carved from it), or
- has a member whose `sid` is `P`'s `sid` (the fallback for processes
  outside the user manager).

A pid the walk does not know (a window whose client exited, or a
compositor-side window) credits nothing. The mapping is a pure function
`focus::credited(procs, groups, focused: &[u32]) -> HashSet<group index>`
in `samplers/apps/focus.rs`, tested on the fixture. *Rejected:* crediting
only the group that holds the pid (tags `cargo` in a focused terminal);
crediting by desktop entry (`class` in `activewindow`): the class names
an app, not a process, and would need the `.desktop` match again.

**6. "Idle but heavy".** `Apps` keeps, while it has subscribers, one
window per group id: `VecDeque<(Instant, f64)>` of the group's CPU per
sample, trimmed past `HEAVY_WINDOW` = 300 s, and the `Instant` it was
first seen. A group is tagged when, at the sample:

- a focus source is running (decision 2: `focused()` answered, not `Err`);
- the window spans at least 300 s (first point at or before `now − 300 s`);
- the mean CPU over the window is at least `HEAVY_CPU` = 20 % of one
  core (percent of one core, as the row prints: a browser at 25 % of one
  of sixteen cores is 1.6 % of the machine and still a tag, which is the
  point);
- no pid credited to it (decision 5) gained focus in the window and it is
  not credited by the focus now.

The windows are dropped in `idle()` with the baseline, so a page opened
after a pause starts counting afresh: the daemon samples nothing while
nobody looks, and a tag needs five minutes of looking. The frame carries
it as `AppGroup.idleHeavy: bool`, `#[serde(skip_serializing_if =
"Not::not")]`, and `AppsData.focus: Option<&'static str>` (`"hyprland"`,
or absent), so the golden file does not change and a client can tell
"not tagged" from "cannot be tagged". `status.focus` says the same for
the owner (`{"source": "hyprland"}` or null). Thresholds live in
`samplers/apps/focus.rs` as a `Thresholds { window, cpu }` the tests
shrink to seconds.

**The tag only tags.** No notification, no sound, no entry in `event`:
plan 018 owns alerts and the spike recorder, and the owner's rule is that
an on-screen element needs a reason. The row gets one dim caption,
`strings::APPS_IDLE_HEAVY` ("busy while not in use"), after the process
count; the inspector gets one line under the stats,
`strings::APPS_IDLE_HEAVY_LINE` ("Over 20 % of a core for five minutes
with none of its windows in focus"); `mirqab apps` prints `idle` in a new
last column only when any row has it, and `--json` and `top_apps` carry
the field. See owner question 3.

**7. The top-apps minute event.** `Sampler::minute_events(&mut self, root)
-> Vec<(&'static str, Value)>` (default empty) joins `minute`;
`history_loop` writes each with `add_event(ts − 30, kind, &data)`,
inside its one wakeup, after the series. `Apps::minute_events` returns at
most one row, kind `top-apps`:

```json
{"cpu":[["Firefox",41.2],["cargo",30.0],["System",4.1]],
 "mem":[["Firefox",2147483648],["code",1073741824],["System",536870912]],
 "procs":412}
```

Names only (`AppGroup.name`, as the `action` event already stores
`comm`s), never ids, units, pids or command lines: a group id can hold a
launcher's command name. How the rate is read:

- **with a subscriber**, from the last sample's data (already built;
  the mean of the minute's samples would be truer but needs keeping
  every sample; the last 2 s rate is what the page shows, and the row
  says which app, not how much exactly);
- **without one**, from one walk per minute differenced against the
  previous minute's walk (`minute_tick: Option<Tick>`), the §6.3 rule
  for counters: the first minute after start writes nothing. The walk
  reads `stat` and `statm` for every pid and `cgroup`, `status`,
  `cmdline` once per new pid (the cache, `procfs.rs:260`), under the
  sampler's lock, inside the one wakeup the idle budget already pays.

**The row is written only when it says something.** Every minute would be
43 200 rows at roughly 150 bytes plus the `(kind, ts)` index, some
7–8 MB of the 40 MiB history budget (§6.2: 38.4 MiB measured with 64
series). So a row is written when the CPU leader is at or above 10 % of
one core, and skipped when it would repeat the previous row's six names
in the same order (the `cpu` figures may differ). A quiet machine writes
a row an hour; a busy one, one a minute. Plan 018's spike recorder reads
`top-apps` rows around a spike, and a spike minute always has a leader
over 10 %. `config.trends.topApps` (boolean, default `true`, in
`config::defaults`, `check`, the schema and `docs/cli.md`'s config list)
turns it off; `history_loop` reads it from `inner.config` before calling
`minute_events`, so the walk is skipped too. **The default flips to
`false` if `tools/bench-idle.sh` (idle, no subscriber) leaves budget:
decision 12 says how it is measured.** The 30-day synthetic fill test
(`history.rs`) gains one `top-apps` row a minute at the row's real size
to check the file stays under 40 MiB with the worst case.

**8. S4: the subject carries the start time.** `Subject::parse` accepts
`{"pid": N}` and `{"pid": N, "start": S}` (`start` a non-negative
integer; any other extra key is still `bad-request`), `Subject::Pid(pid,
Option<u64>)`. `resolve` compares `S` with the process's `stat` field 22
after D3's tgid step (the leader's start, as `wouldDo` reports it); a
mismatch is `Failed` with `strings::apps_process_gone(pid)` ("pid N is
another process now"), for a signal and for a service alike, at plan time
and at redeem. The plan's subject binding (`redeem` compares the subject,
"As built (part 2)") already makes the second call send the same
`start`. The interface sends `{"pid", "start"}` from the row (4b); the
CLI's `mirqab quit <pid>` has no row and sends the pid alone, as today.
`start_differs` in the view model stays as a second check and now also
covers a `run` entry's `pids` (which are bare pids) by trusting the
daemon's answer instead.

**9. S5: the inspector asks with the start time.** `get apps.inspect
{pid, start?}`: with `start`, a process whose `stat` start differs is
`Failed` with the same `apps_process_gone` message, read before anything
else, so no verdict is formed for the wrong process. The interface sends
the row's `start` (4b) and shows `APPS_ROW_GONE` in the sheet on that
error. `mirqab what PID` sends none.

**10. D1: no sleep and no `pacman` under the sampler's lock.** Two small
hooks in `sampler.rs`, both default no-ops, so the server's shape does
not change for other streams:

- `fn prime(&mut self, root, now) -> Option<Duration>`: a `get` calls it
  under the lock first; `Apps` and `Process` take their first reading
  into a private `get_base` and return `Some(SNAPSHOT_GAP)` when no
  subscriber baseline is 1–10 s old (else `None`). The server releases
  the lock, sleeps the gap, re-locks and calls `snapshot`, which uses
  `get_base` when it is under 10 s old. A `get` of a missing pid on
  `process` still answers at once (K7 of PR #18: `prime` returns `None`
  when the first reading is empty).
- `Sampler::query` returns `Answer`: `Ready(QueryResult)` or
  `Later(Box<dyn FnOnce() -> QueryResult + Send>)`. `inspect` reads
  everything it reads from `/proc` under the lock (fast), then returns
  `Later` with the partial result and an `Arc<Mutex<Owners>>` (the
  pacman cache moves behind its own lock on `Apps`); the closure asks
  pacman and forms the verdict after the server dropped the sampler's
  lock. A subscriber's 2 s tick never waits behind a 5 s pacman timeout
  again.

Measured first (decision 12, "late frame"), then fixed in 4a whatever the
number says: the code holds the lock across a sleep by construction.

**11. D2 of PR #17: a connect that cannot hang.** `sys::connect_timeout(path:
&Path, timeout: Duration) -> io::Result<UnixStream>`: `socket(AF_UNIX,
SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK)`, `connect`, on `EINPROGRESS`
`poll(POLLOUT, timeout)` then `getsockopt(SO_ERROR)`, then clear
`O_NONBLOCK` and `UnixStream::from_raw_fd`. About 50 lines, four
`unsafe` blocks with their contracts, beside `pidfd_open` in `sys.rs`;
`ETIMEDOUT` when `poll` returns 0. The compositor probe
(`compositor_sockets`) and the Hyprland reader (decision 4) use it; a
timeout on a `wayland-*` socket is "a compositor that cannot be told"
and refuses the action as any other probe error does (K7 of PR #17).
Test: a `UnixListener` with backlog 0 whose one slot a first connection
fills; the second `connect_timeout(…, 200 ms)` returns `ETIMEDOUT` in
under a second **[unverified: whether Linux lets the second connect
reach `EINPROGRESS` or completes it from the backlog; the test asserts
"returns within a second", not the errno]**. *Rejected:* a connect on a
helper thread abandoned on timeout (a leaked thread per frozen
compositor); `libc::alarm` (process-wide).

**12. Measurement (step 9).** All on the development machine, release
build, recorded in "As built (part 4)" with the date:

- `tools/bench-idle.sh --subscribe apps --max-cpu 1.5 --procs 400`: a
  new `--procs N` option builds a scratch root from `root-desktop` with
  its user processes cloned until the tree holds at least N (a new
  `tools/mkfixture.sh procs --out DIR --clones K`, which offsets pids,
  `ppid`, `pgrp`, `sid` by `K × 100000` and suffixes scope and session
  names, so groups multiply too), and measures against it. Budget 1.5 %
  of one core (the Tests table). CI gains `bench --subscribe apps
  --max-cpu 1.5 --procs 400` beside the two runs it has. Nothing is
  committed under `tests/fixtures/` for it.
- Idle (`tools/bench-idle.sh`, no subscriber) with `trends.topApps` on:
  the 60 s run sees one minute wakeup, which is one walk; the budget is
  the existing 0.3 % and 15 MiB. The decision rule of decision 7
  applies to this number. Also recorded: idle with the key off, for the
  difference.
- `apps` with one `process` subscriber and the inspector open, from
  `mirqab status` and `pidstat`, with the GPU `fdinfo` reads: the
  inspector's cost per `apps.inspect` (time, from a debug log line under
  `MIRQAB_TEST=1`) on a browser with many fds, before and after D2's cap
  (decision 13).
- The late frame (D1): with `--subscribe apps` held by `socat`, a loop of
  `mirqab apps` calls every 3 s; the gap between consecutive event lines
  on the subscription, before and after decision 10. Expected before:
  gaps of 2.5 s; after: 2.0 s.
- The reader: `mirqab status` and `pidstat -p <daemon> 10 1` with the
  Apps page open while the owner switches windows for a minute, against
  the same with the page closed.
- The `process` stream's cap (nit of PR #18): the interface holds one
  `process` subscriber at a time (`inspector.rs:390`), and no other
  client subscribes to it; **the first eight stay**, and `capped` tells a
  ninth. Decided here, nothing to measure: newest-wins would let a
  scripted ninth subscriber evict the page's graph.

**13. D2 of PR #16: fewer `fdinfo` reads.** `gpu` lists `fd/` once,
follows each link with `read_link` and reads `fdinfo/<n>` only where the
link names `/dev/dri/`; the `fd/` listing is capped at 1024 entries, past
which `gpu` is left out and the inspector says nothing (an absent key, as
for any unreadable field). The fixture's `drm=` extra already puts the
`fd` link beside the `fdinfo` file (`tools/mkfixture.sh`, `fd=N:TARGET`).

**14. D7 of PR #16: `.flatpak-info` is a regular file or nothing.**
`procfs` reads `proc/<pid>/root/.flatpak-info` only when `symlink_metadata`
says it is a regular file, through `Root::read_prefix` (4 KiB); a FIFO, a
socket or a directory there is "no Flatpak". Test: a FIFO at that path in
a scratch copy of the fixture; the walk returns within the test's
timeout.

Audit 2026-11-21, L6: the open also carries `O_NOCTTY`. The daemon is a
session leader without a terminal under systemd, so a tty planted at that
path would otherwise become its controlling terminal on open. Test:
`root.rs`, `opening_a_tty_does_not_make_it_the_controlling_terminal`
(a child copy of the test binary starts a session, opens a fresh
pseudo-terminal through `read_regular_prefix`, and checks `/dev/tty`).

Audit 2026-11-21, L7: `O_NONBLOCK` does not stop an open (or a lookup) on
FUSE from waiting on its server, so a process whose root sat on a hung
FUSE mount stalled the walk under the sampler lock. The fix pins what it
asks about. `Root::read_regular_prefix_in` opens `proc/<pid>/root` with
`O_PATH` (following that link jumps to the root's dentry and calls no
filesystem), takes its mount id from the `mnt_id:` line of
`/proc/self/fdinfo/<fd>` (the kernel prints it from the descriptor; no
filesystem is asked, unlike `fstatfs` or a `statx` that may refresh
attributes), and clears it before `.flatpak-info` is looked up. The file
is then opened `O_PATH|O_NOFOLLOW` through `/proc/self/fd/<rootfd>`, its
mount id cleared too (a file can be a mount of its own), checked to be a
regular file, and reopened for reading through its own descriptor with
`O_NONBLOCK|O_NOCTTY`. Because every step holds a descriptor, a
`pivot_root` or a mount between a check and the read changes nothing that
was checked. `procfs::read_flatpak_info` clears a mount id only when the
process's `mountinfo` lists it (field 1) as a `tmpfs`
(`procfs::tmpfs_mount`). On the live machine a mountinfo that cannot be
read clears nothing; a fixture without one (the fixtures have none) is
read as before through `read_regular_prefix`. This costs one more read
per process that reaches the `.flatpak-info` check, once in the process's
life (`Info` is cached), plus two `fdinfo` reads.

Why tmpfs only: bubblewrap builds every Flatpak root on a tmpfs. Checked
on the development machine on 2026-11-21: for a running Flatpak app, the
`O_PATH` descriptors of its root and of its `.flatpak-info` carry the same
mount id, and its mountinfo lists that id at `/` with root `/newroot` and
type `tmpfs`. Rejected options:

- Looking for a FUSE line at `/` or `/.flatpak-info` in mountinfo (the
  first fix). The kernel omits mounts the root cannot reach, so a process
  chrooted into a directory of a FUSE mount has no line at `/` at all.
- Requiring a mount at `/` and no FUSE among the mounts at `/` (the
  second fix, rejected in delta review). A process in its own user and
  mount namespace can chroot below a FUSE mount and mount a tmpfs over its
  own `/`: mountinfo then lists only that tmpfs, but a chroot root does not
  follow an overmount, so `proc/<pid>/root` still resolves to the FUSE
  directory. Mount points prove nothing; the pinned mount id does. The
  reviewer confirmed on this machine that the `O_PATH` descriptor's
  `mnt_id` in that setup is absent from the process's mountinfo, so the
  id check refuses it. Reading mountinfo before an unpinned open also left
  a race with `pivot_root`.
- Clearing any type that does not start with `fuse`, as the review
  suggested. An overlay with a FUSE lower layer is not `fuse*` but looks
  its names up on the FUSE server; only Flatpak roots matter here, and
  they are tmpfs.
- `statx(STATX_MNT_ID, AT_STATX_DONT_SYNC)` for the mount id. It works,
  but it reaches the filesystem's `getattr`, where `fdinfo` does not, and
  needs `unsafe` for a call std does not wrap.

Tests: `root.rs`, `a_pinned_read_asks_about_the_directory_mount_before_the_name`
(a refused root mount is refused before the name is looked up),
`a_pinned_read_reads_a_regular_file_on_a_cleared_mount`,
`a_pinned_read_refuses_what_is_not_a_regular_file` (a FIFO, a symlink, a
directory, and a name that is not one component) and
`a_pinned_read_does_not_leave_a_fixture_tree`; `tests/apps.rs`,
`flatpak_info_is_not_opened_through_a_fuse_root`, whose fixture mountinfo
names the scratch tree's real mount id (`Root::mount_id`): FUSE, overlay
or ext4 for the root's mount, a tmpfs at `/` that is not the root's mount
(the review's bypass), the root's mount not listed, a line without a type
and an empty mountinfo are skipped; a tmpfs root's mount, wherever it is
mounted and with FUSE elsewhere, and no mountinfo at all are read. The
bypass row failed against the second fix before this one.

**15. The session bus by command name (owner question 1).** See the
question; if the owner says yes, `guard::SESSION_BUS_COMMS = ["dbus-daemon",
"dbus-broker", "dbus-broker-launch"]` joins `act::refusal` beside the
unit-name check (`act.rs:479`), with `strings::refuse_session_bus_comm`,
and a fixture row (a `dbus-daemon` under a `tree:` group, uid 1000)
tests it.

**16. The interface (4b).**

- **The tag.** `GroupLine.idle_heavy: bool` from `AppGroup.idleHeavy`;
  `group_row.rs` shows `APPS_IDLE_HEAVY` as a `caption dim-label` after
  the process count, visible only when set; the group's accessible label
  appends it; `Selection.idle_heavy` puts `APPS_IDLE_HEAVY_LINE` under
  the stats in the sheet. No colour, no icon: a word.
- **Escape** (owner question 2): a `ShortcutController` with scope
  `Local` on the page's root binds Escape to "clear the selection" when a
  row is selected and lets it propagate otherwise, so in popover mode a
  second Escape still hides the popover (`window.rs:250`) and in window
  mode Escape does nothing with no sheet open. The dialog's own Escape
  (Cancel, K7 of PR #19) comes first because the dialog is modal.
- **S8**: a `GestureClick` on the `ListView` sets `pressed` on `pressed`
  and clears it on `released`/`cancel`; `refresh(true)` with `pressed`
  set keeps the frame and defers the store write to the release. A
  frame is never lost (the next write uses the latest frame). The owner
  check of acceptance part 3 item 1 stays, but the fix is cheap and lands
  before it.
- **N3**: `connect_unmap` also clears `Page.frame` and the store, so a
  re-map shows nothing until the first frame (the sheet is already hidden
  by `refresh` when the selection has no row).
- **N8**: `Property::Selected` moves from the row's box to the
  `gtk::ListItem` (`item.set_accessible_state`), with the box keeping
  its `selected` style class.
- **N4**: the expander is hidden while a search shows a group's matching
  members (the view model's `GroupLine.expandable` is false in that
  case).
- **N6** stays deferred to the plan that adds reconnects: `process_served`
  is read per selection; a daemon restart while a process is selected
  shows a flat graph until the next selection, and today's client has no
  reconnect event to hook.
- **Start times**: `Selection.args` for a process row is
  `{"pid", "start"}` (decision 8); `inspect` sends `{"pid", "start"}`
  and treats the `apps_process_gone` error as `APPS_ROW_GONE` in the
  sheet (decision 9).
- **The CLI**: `mirqab apps` prints the `idle` column when any row has
  the tag; `mirqab status` prints the focus source line
  (`strings::status_focus`).

### Rejected

- **Polling `j/activewindow` every 2 s from the sampler.** A request per
  tick for an event the compositor pushes; §9.2 rule 3.
- **Reading the title or class.** Not needed for the pid, and a title is
  the most private thing on a screen; the reader discards `activewindow`
  lines unparsed.
- **A `focus` stream.** Nothing draws it; the tag is the only consumer,
  and a stream would need a subscriber to keep the reader alive, which
  the `apps` subscription already does.
- **Tagging on the frame-to-frame rate alone** (one sample over 20 %).
  A build finishing is not "idle but heavy"; five minutes is the plan's
  definition.
- **Writing the top-apps row every minute.** 7–8 MB of the 40 MiB
  history budget for rows that say "nothing was busy".
- **Storing group ids in the top-apps row.** An id holds the unit name,
  which can hold the command a launcher ran.
- **Newest-wins for the `process` cap.** A ninth subscriber would evict
  the page's graph.
- **A committed 400-process fixture.** Some 3 500 generated files in git
  for one benchmark; the generator makes it in a second.
- **A notification for the tag.** Plan 018's job, and the owner's
  no-clutter rule.
- **Non-blocking connect on a helper thread.** A frozen compositor would
  leak a thread per probe.

### Tests

| Test | Where | Checks |
|------|-------|--------|
| Instance discovery | `crates/mirqab-core/tests/focus.rs` (new; `# mirqab:covers crates/mirqab-core/src/desktop/** crates/mirqab-core/src/samplers/apps/focus.rs`) | A scratch runtime dir with `hypr/<sig>/hyprland.lock` naming fixture pid 1050 and a live `UnixListener` at `.socket2.sock` is found without the environment variable; a lock naming a pid with no `proc/<pid>` is skipped; two directories pick the newer; no `hypr/` is `Err` and `apps` samples with `focus` absent |
| The reader | same, a fake Hyprland in the test (a thread with two `UnixListener`s: one writes scripted event lines, one answers `j/activewindow` with a fixed JSON holding a fixture pid) | `activewindowv2>>ADDR` asks once and caches; a second focus of the same address asks nothing; `closewindow` evicts; an empty address is no focus; `0x`-prefixed and bare addresses match; a request that hangs (the fake never answers) times out and the reader goes on; five failures stop it; `stop` ends the thread (joined within 1 s); an EOF ends it |
| Credit | `samplers/apps/focus.rs` unit tests on the fixture walk | Focus on pid 3000 (the terminal) credits `session:3030` (cargo) and the terminal's scope; focus on pid 2001 credits the browser's group only; an unknown pid credits nothing |
| The tag | `tests/apps.rs` with `Thresholds { window: 3 s, cpu: 20 }` and a fake `Focus` | Over a window of samples on `root-desktop` and `.t1` the cargo group (91 %) is tagged after the window fills and the browser is not; a focus on 3000 inside the window clears cargo's tag at once; with the fake `Focus` returning `Err`, no group is tagged and `focus` is absent; `idle()` drops the windows |
| The minute event | `tests/apps.rs` | `minute_events` on `root-desktop` then `.t1` gives one `top-apps` row with three names by CPU and three by memory, no id, pid or command line; the first minute gives none; a repeat of the same six names gives none; a leader under 10 % gives none; with a subscriber baseline, no second walk (the walk count from a counting `Root` wrapper, or the cache's size) |
| The config key | `config.rs` unit tests; `tests/test-cli.sh` | `trends.topApps` is in `keys()`, must be boolean, defaults true; `mirqab config set trends.topApps false` round-trips; the schema names it (`test-rust.sh` already covers the schema file) |
| `history_loop` writes events | `tests/protocol.rs` | A daemon with a history file and a sampler whose `minute_events` returns a row: `history.events {kind: "top-apps"}` returns it after the minute (the test moves the clock as the writer's test does, or uses the `Counted` stream pattern) |
| S4 | `tests/actions.rs`; `tests/test-cli.sh` | `{"pid": 7000, "start": <t0 start>}` planned on `root-desktop` and redeemed on `.t1` is `failed` "another process now", nothing recorded; `{"pid": 1300, "start": wrong}` on a service pid is refused the same way; `{"pid": N, "start": "x"}` and a third key are `bad-request`; the CLI still sends a bare pid |
| S5 | `tests/apps.rs` | `apps.inspect {pid: 7000, start: <t0>}` on `.t1` is `failed` with the gone message and no verdict; without `start` it inspects as today |
| D1 | `tests/protocol.rs` | A subscriber at 2000 ms and a `get apps` with no young baseline: the subscriber's next event arrives within 2.2 s of the previous (today 2.5 s); `get apps.inspect` with a stub pacman that sleeps 2 s: the subscriber's events keep their 2 s spacing |
| D2 (connect) | `sys.rs` unit test | Decision 11's test; `ENOENT` for a missing path; a live listener connects |
| D2 (GPU) | `tests/apps.rs` | A scratch copy with 1 100 `fd/` links: `gpu` is left out; with 3 links, one to `/dev/dri/renderD128`, only that `fdinfo` is read (a count through a fixture with a `fdinfo` that would parse but has no `/dev/dri` link) |
| D7 | `tests/apps.rs` | A FIFO at `root/.flatpak-info` in a scratch copy: the walk finishes and the group is not Flatpak |
| The bus by name | `act.rs`, `tests/actions.rs` (if the owner says yes) | A `dbus-daemon` row (uid 1000, `tree:`) is refused with the reason |
| Bench | `tests/test-bench.sh` | `--procs 400` on a 2 s run measures a scratch root and prints the clone count; `mkfixture.sh procs --clones 1 --out scratch` writes twice the rows with offset pids (`tests/test-fixtures.sh`) |
| 4b: view model | `crates/mirqab-view/tests/apps.rs` | `GroupLine.idle_heavy` and the label from a frame with `idleHeavy: true`; `Selection.idle_heavy`; `expandable` false under a member-only search (N4); gap 8: `rows()`'s `selected` flags for a group and a process; `args` and the inspect params carry `start` |
| 4b: widgets | `crates/mirqab-ui/tests/widgets.rs` | Gap 6: the `process` subscriber's params are `{"pid": 2001, "start": 5010}`; the inspector's `get` carries `start`; a `failed` gone reply shows `APPS_ROW_GONE` and no verdict; Escape with a row selected clears it, with none selected propagates; a press held across a frame keeps the store, the release writes it (S8); the tag caption is visible only when set; `Property::Selected` is on the list item (N8) |
| 4b: client | `crates/mirqab-ui/tests/widgets.rs` or a unit test in `client.rs` | Gap 5: the merge when one of two handles drops: the remaining handle's params are sent |
| 4b: end to end | `tests/test-ui-apps.sh` | With a fake Hyprland in the sandbox (`python3 -I`, as the S1 check already does) emitting one `activewindowv2` for the browser's pid and a daemon with `Thresholds` shrunk through `MIRQAB_TEST=1` and `MIRQAB_HEAVY_WINDOW_MS=4000`: the cargo row shows the tag within the test's pauses and the browser row does not; `mirqab status --json` has `.focus.source == "hyprland"`; after `close`, `status` has `focus: null` and no `focus-hyprland` thread (`/proc/<daemon>/task/*/comm`). Without the fake, `focus` is null and no row has the caption. N3: re-mapping the page shows no stale rows before the first frame |

`sampler.rs`, `server.rs`, `sys.rs` and `act.rs` are shared
infrastructure: say so in the PR and let `tests/run.sh --affected`
widen. `MIRQAB_HEAVY_WINDOW_MS` is read only under `MIRQAB_TEST=1` and
only on a fixture root (the `caller_for` pattern, `server.rs:1396`).

### Acceptance (part 4)

Headless, in CI: every row above, and `tools/bench-idle.sh --subscribe
apps --max-cpu 1.5 --procs 400` within budget (acceptance 4 of the plan).

Owner checks on the development machine:

1. With the Apps page open, `mirqab status` shows `focus: hyprland`; a
   `yes > /dev/null &` left for five minutes while working in other
   windows gets the "busy while not in use" caption; clicking its
   terminal clears it within a frame.
2. Window closed: `mirqab status` shows `focus: null`, and
   `ls /proc/$(pgrep -x mirqab)/task/*/comm` names no `focus-hyprland`.
3. Restart Hyprland (or log out and in) with the daemon running, open
   Apps: the source is found again.
4. `mirqab history` (or `history.events {kind: "top-apps"}`) after a
   busy ten minutes shows rows naming the busy app, and none for a quiet
   stretch.
5. `tools/bench-idle.sh` idle, on this machine, with `trends.topApps`
   on: within 0.3 % and 15 MiB (the number is recorded in As built).
6. Escape with the sheet open closes it; in popover mode a second Escape
   closes the popover.
7. Under the CPU sort with a build running, pressing a row that moves
   selects the row that was under the pointer at the press.

### One pull request or two

Two, in this order:

- **4a, daemon:** `desktop/{mod,hyprland}.rs`, `samplers/apps/focus.rs`,
  the tag and `focus` in the frame, `status.focus`; `minute_events`,
  the `top-apps` row, `trends.topApps`; S4, S5; D1 (`prime`, `Answer`),
  D2 connect, D2 GPU, D7; the bench's `--procs`, `mkfixture --clones`,
  the CI line; the measurements; architecture §5.2 (`status.focus`),
  §5.3 (`apps` fields, `minute_events`, `prime`, `Answer`), §5.4
  (`start` in a subject; the bus by name if agreed), §6.1 (`top-apps`),
  §9.1 (the `apps` row), §12; the CLI's `idle` column and status line;
  "As built (part 4a)". If the review of 4a runs long, D1 and the GPU
  cap can be split off as 4a′ without touching the rest: nothing else
  depends on them.
- **4b, interface:** decision 16 whole, the view-model and widget tests,
  `tests/test-ui-apps.sh`, architecture §7.2 (one sentence on the
  caption) and §7.5 (the fake Hyprland in a test), "As built (part 4b)",
  the plan's state to DONE and `plans/README.md`'s row.

### Owner questions (part 4)

1. **Refuse the session bus by command name too?** "Known limits
   (part 3a)" says a bus started by `dbus-run-session` or `dbus-launch`
   is in no bus unit and is not refused. **Recommendation: yes**, refuse
   a process of the user whose `comm` is `dbus-daemon`, `dbus-broker` or
   `dbus-broker-launch` (decision 15), as the compositor is refused by
   name. The loss is small: a stray private bus from a test harness
   cannot be quit from mirqab; `kill` from a shell still can. The gain
   is that the one process whose death ends the session is never
   signalled whatever unit it is in.
2. **An Escape binding that closes the inspector?** **Recommendation:
   yes**, as decision 16 describes: Escape clears the selection when a
   row is selected and otherwise does what it does today (hides the
   popover; nothing in window mode). It is the key every sidebar in GNOME
   answers to, it adds no visible element, and the modal dialog's Escape
   stays Cancel.
3. **Should "idle but heavy" notify, or only tag?** **Recommendation:
   tag only** in this plan (a dim caption on the row, a line in the
   sheet, the `idleHeavy` field). Alerts, their thresholds and their
   quiet hours are plan 018's, which can read the same field; a
   notification from the Apps sampler would be the first one in mirqab
   with no settings page behind it.
4. **The top-apps row only when the leader is at or above 10 % of a
   core?** Decision 7's rule keeps the history under its 40 MiB budget
   by not recording quiet minutes. **Recommendation: yes.** The
   alternative, a row every minute, is 7–8 MB of rows that name the
   idle leader at 0.3 %, and plan 018's rewind wants the busy minutes.

### Unverified (part 4)

- The exact `activewindowv2` line from a live Hyprland (no focus change
  happened during two captures on 2026-11-14); whether the address has a
  `0x` prefix. The reader accepts both; the fixture lines follow the
  wiki.
- That `j/activewindow`'s `pid` for a Flatpak app's window is the host
  pid (the compositor's `SO_PEERCRED` view). If it is the sandbox's pid,
  the credit misses and the Flatpak app can be tagged while focused; to
  check with one Flatpak app on the development machine.
- That a second `connect` on a Unix socket with a full backlog returns
  `EINPROGRESS` and then times out in `poll`, rather than being queued
  by the kernel beyond the backlog (decision 11's test asserts only the
  bound on time).
- The reader's and the minute walk's cost on the development machine:
  measured in 4a, recorded in As built; the `trends.topApps` default
  follows the idle number.
- Whether a `.socket2.sock` left by an exited Hyprland (no process behind
  it) answers `ECONNREFUSED` at connect, which the discovery of
  decision 3 treats as "not this instance" and moves on from.
- Deferred from the review of PR #20 (nit 2): `runtime_rel` on a
  non-UTF-8 `XDG_RUNTIME_DIR`, or one with `..`, gives no runtime
  directory, so the compositor probe goes quiet (no refusal by socket,
  the name list only) and the focus source finds nothing. A safety probe
  that goes quiet on a malformed runtime dir; fall back to
  `run/user/<uid>` in a later pass.

### Owner decisions (part 4, 2026-11-14)

1. **The session bus is refused by command name too.** Processes named
   `dbus-daemon`, `dbus-broker` or `dbus-broker-launch` are never
   signalled, whatever unit they are in; the reason is in words in
   `strings::refuse_session_bus_comm`, tested with a synthetic fixture
   process, and checked again through the pidfd like every other refusal
   (decision 15).
2. **Escape closes the inspector** (decision 16, owner question 2): 4b's
   work.
3. **"Idle but heavy" is a tag only**: no notification.
4. **The `top-apps` row** is written only when the CPU leader used at
   least 10 % of a core and the six names differ from the last row's;
   `config.trends.topApps` defaults on and flips off only if the idle
   benchmark breaks its 0.3 % budget (it does not: "As built (part 4a)").
5. **`top-apps` rows are kept 3 days** (owner, 2026-11-14, asked after
   the 30-day fill measured 49.2 MB against the 40 MiB budget). Every other
   series keeps 30 days; the history budget does not grow.

Confirmed for 4b (owner, 2026-11-15): Escape closes the inspector when a
row is selected (it clears the selection, closing the sheet) and
otherwise goes on, so in popover mode a second Escape still hides the
popover; a real window still never closes on Escape (plan 022). "Idle
but heavy" stays a tag only: the caption and the sheet's line, no
notification.

6. **Narrow focus credit** (owner, 2026-11-15, answering "Owner question
   for 4b", S4 of the review of PR #20). A focused window credits its own
   group, and its siblings only within its own `app-*.scope` unit (a
   terminal and the `session:` groups carved from it) and within one
   terminal `session:` split; apps that merely share the compositor's
   service, another service, or the session scope get no credit from each
   other's windows. As built: "As built (part 4b)", the first item.

## As built (part 4a)

Part 4a (2026-11-14) is the daemon side of part 4: checklist steps 7a–7g,
8a, 8b and 9a–9c, and 4a's share of 7i. D1 and the GPU cap stayed in this
pull request; nothing was split off as 4a′. What was built, with where it
lives:

- **`sys::connect_timeout`** (`crates/mirqab-core/src/sys.rs:181`). Not
  decision 11's non-blocking connect and `poll`: on Linux an `AF_UNIX`
  stream connect never reports EINPROGRESS. A non-blocking one fails with
  EAGAIN at once when the listener's backlog is full; a blocking one waits
  for a slot for the socket's send timeout (`sock_sndtimeo` in
  `unix_stream_connect`, net/unix/af_unix.c) and then fails with EAGAIN.
  So the socket is blocking with `SO_SNDTIMEO` set to the timeout
  (`UnixStream::set_write_timeout`, safe), connected, and the timeout
  cleared; EAGAIN is returned as ETIMEDOUT. Two `unsafe` blocks
  (`socket`, `connect`) and one `from_raw_fd`, each with its contract.
  The test (`:264`) fills a backlog of 0 (`listen(fd, 0)`) with one
  connection: the second `connect_timeout(…, 200 ms)` returns ETIMEDOUT
  after 150–1000 ms, a missing path ENOENT, and an `accept` frees the
  slot. This resolves the unverified item of decision 11: the kernel
  queues nothing past the backlog; it waits for the send timeout.
- **The compositor probe** uses it (`samplers/apps/actions.rs:443`,
  `PROBE_TIMEOUT` 500 ms); a timeout is "a compositor that cannot be
  told" and refuses, as any probe error (K7 of PR #17).
- **`desktop/`** (`crates/mirqab-core/src/desktop/mod.rs`): the `Focus`
  trait (`:43`: `start(root, own_uid)`, `focused(since) ->
  Option<FocusedSince>`, `source()`, `stop()`), `runtime_rel` (`:61`) and
  `runtime_dir`, moved from `actions.rs`. **How the Hyprland socket path
  fits `Root`:** the runtime directory is a path under the root: on the
  live machine `$XDG_RUNTIME_DIR` without its leading `/` (the root is
  `/`), on a fixture root `run/user/<uid>` inside the tree, and there the
  environment (`XDG_RUNTIME_DIR`, `HYPRLAND_INSTANCE_SIGNATURE`) is never
  read, so a test daemon finds a fake Hyprland in its own tree and never
  the live session's. The lock file, the directory listing and the
  sockets' paths all go through `Root` (`read_prefix`, `read_dir`,
  `modified`, `path`).
- **`desktop/hyprland.rs`**: `instance` (`:141`: the signature when its
  directory has `.socket2.sock`, else the newest directory whose
  `hyprland.lock` names a pid with a `proc/<pid>/`), `event` and
  `active_answer` (pure parsers), the reader thread `read_events`
  (`:252`; `focus-hyprland`, 64 KiB stack; lines capped at 64 KiB, an
  overlong one skipped whole; answers capped at 64 KiB), `start` (`:339`:
  connect, one `j/activewindow`, the thread), `stop` (`:411`: `shutdown`
  on the event socket, then `join`). The cache maps an address (lower
  case, `0x` stripped) to a pid; `closewindow` evicts; ten requests a
  second at most; five failed requests in a row, an end of file or a read
  error end the thread and `source()` turns None. No title or class is
  kept or logged: every line but `activewindowv2` and `closewindow` is
  dropped unparsed.
- **`samplers/apps/focus.rs`**: `credited` (`:50`: the group holding the
  pid, every group with a member in the same host unit, never a slice;
  for a pid outside the user manager, every group with a member in its
  session), `Thresholds` (300 s, 20 % of one core), `Windows` (`:76`:
  per group id, one point at or before the window's start kept, so a full
  window can be told; the mean is over the points inside it).
- **`Apps`** (`samplers/apps/mod.rs`): the focus source starts with the
  first sample (`:543`) and stops in `idle` (`:578`), which also drops the
  windows and the last frame; `tag` (`:323`) sets `idleHeavy` (never on
  System) and the frame's `focus`; windows are kept only while a source
  runs; `status` (`:681`) gives `focus`. `MIRQAB_HEAVY_WINDOW_MS` is read
  in `probe` only under `MIRQAB_TEST=1` on a fixture root (`:533`).
  `AppGroup.idle_heavy` and `AppsData.focus`
  (`crates/mirqab-proto/src/stream.rs:300`, `:260`) are left out when
  false or absent, so `tests/golden/apps.json` did not change.
- **The server tells a sampler at once when its last subscriber left**
  (`server.rs:596`), not before the next subscriber's first sample as
  before: the brief read `Sampler::idle` as called "after the
  subscribers left", but it ran only when the stream was subscribed
  again, so the reader would have outlived the page. It is still called
  once, so the existing protocol test (`idle` counted once) holds.
- **D1** (`sampler.rs:27`, `:67`; `server.rs:1050`, `:1078`): `Answer`
  (`Ready`, `Later`) and `Sampler::prime`. `get` calls `prime` under the
  lock, sleeps the returned gap without it, and calls `snapshot`;
  `Apps::prime` (`mod.rs:587`) and `Process::prime` (`process.rs:298`)
  take the first reading (`Process` answers a missing pid at once, K7 of
  PR #18). `apps.inspect` reads `proc/` under the lock (`inspect::read`,
  `inspect.rs:416`) and returns `Later` with the rest (`inspect::finish`,
  `:571`); `Owners` sits behind its own `Arc<Mutex<_>>`. A `snapshot`
  called without `prime` (tests) keeps the old inline wait.
- **S4** (`actions.rs:118`, `:678`): `Subject::Pid(pid, Option<start>)`;
  `{"pid", "start"}` parses, any other key or a `start` that is not a
  whole number is `bad-request` (`strings::ACT_ARGS` says so); a start
  that is not the leader's now is `failed` `apps_process_gone`, for a
  signal and a service, at plan time and at redeem; the plan's subject
  carries the start.
- **S5** (`mod.rs:626`, `inspect.rs:416`): `apps.inspect {pid, start?}`;
  a start that differs is `failed` before anything is read; the reads are
  checked against the start again when they end, so a pid reused in
  between is not explained with another process's files.
- **D2 (GPU)** (`inspect.rs:291`): `fd/` listed once, at most 1024
  entries (`MAX_GPU_FDS`; more leaves `gpu` out), and `fdinfo/<n>` read
  only where the link names `/dev/dri/` (`root::DRM_DIR`, in `root.rs`
  for the lint's literal-path rule).
- **D7** (`procfs.rs:159`, `root.rs:127`): `.flatpak-info` through
  `Root::read_regular_prefix`, 4 KiB: opened with `O_NONBLOCK |
  O_NOFOLLOW` and checked with `fstat` on the open descriptor, which is
  stricter than decision 14's `symlink_metadata` then `read_prefix` (no
  window between the check and the open); the inspector reads it the same
  way.
- **The bus by name** (`guard.rs:64`, `act.rs:485`): also the kernel's
  15-byte `dbus-broker-lau`, as `dbus-broker-launch` runs.
- **The `top-apps` row** (`mod.rs:353`, `top_row` `:410`;
  `server.rs:523`): `Sampler::minute_events(root, config)`
  (`sampler.rs:123`) runs in the minute writer's wakeup with the
  refreshed effective config, and each row is written with `add_event(ts
  − 30, …)`. `Apps::top_apps` reads the last sample while subscribed
  (its walk's baseline becomes the minute's), else walks and differences
  against the previous minute's walk (none past 150 s); the leader and
  repeat rules; `trends.topApps: false` skips the walk. `config.rs:24`,
  the schema and `trends.topApps` in `keys()`.
- **`top-apps` rows are kept 3 days** (`history.rs:18`,
  `KEEP_TOP_APPS_SECS`; pruned by the `(kind, ts)` index). See the
  deviations.
- **The CLI**: `mirqab apps` gains an `IDLE` column only when a row is
  tagged (`crates/mirqab/src/apps.rs:49`, `strings::APPS_HEADER_IDLE`,
  `apps_row_idle`); `mirqab status` prints `focus       hyprland` or
  `focus       none (…)` (`cli.rs:83`, `strings::status_focus`); MCP
  `top_apps` carries `idleHeavy: true` on a tagged group
  (`mcp/tools.rs:225`); `mirqab quit PID` still sends the bare pid.
  `docs/cli.md` regenerated (the `status` and `apps` help).
- **Tools**: `tools/mkfixture.sh procs --clones K` (`clone_ref`,
  `clone_cgroup`, `:163`); `tools/bench-idle.sh --procs N`; the CI line
  `bench --subscribe apps --max-cpu 1.5 --procs 400`
  (`.github/workflows/ci.yml:131`). **A bench bug found and fixed:**
  every `--subscribe` run without `--live` exited 1 with no message
  (under `inherit_errexit` the `$( ((live)) && … )` in the run's label
  failed); CI only ran `--subscribe system --live`, so it never showed.
  The brief's CI line would have failed without the fix.
- **Docs**: architecture §5.2 (`status.focus`, `apps.inspect`'s
  `start`), §5.3 (the `apps` tag and `focus`, `idle` when the last
  subscriber leaves, `prime`, `Answer`, `minute_events`, `status`), §5.4
  (`start` in a subject; the bus by name), §6.1 and §6.3 (`top-apps`, 3
  days), §9.1 (the 400-process row), §12 (`src/desktop/`).

Tests (2026-11-14, `cargo test -p mirqab-core`): the library's unit tests
60 → 68 (`connect_timeout`, `runtime_rel`, `FocusedSince::pids`, the two
parsers, `credited` and `Windows` on the fixture, `trends.topApps`;
`act.rs`'s bus test gains three comm rows, `guard.rs`'s the comm names,
`actions.rs`'s `args_are_strict` the start keys); `tests/focus.rs` 5
(new: discovery; the reader with a fake Hyprland: one request per new
address, the cache with and without `0x`, `closewindow`, no window, a
hung request timed out and the reader going on, five failures ending it,
`stop` within a second, an end of file, a restart; no source on the
fixture); `tests/apps.rs` 18 → 27 (the tag over a 3 s window: cargo
tagged once the window is full, the focused browser not, a focus on the
terminal clearing cargo at once, `get` carrying the tag, `idle` stopping
the source and dropping the windows; no source, no tag and no windows;
the `top-apps` row from the subscriber's frame with no walk, its exact
names, no ids or pids, the repeat rule, the 10 % rule over a minute, off
skipping the walk, walks a minute apart, a stale baseline; S5; `Later`
for pacman; GPU from DRM fds only and the 1024 cap; a FIFO and a symlink
at `.flatpak-info`; `prime`); `tests/actions.rs` 22 → 24 (S4; a
`dbus-daemon` in the login session's scope refused by pid and as its
`tree:` group); `tests/protocol.rs` 23 → 27 (the minute writer writes a
sampler's events and passes the config; back-to-back `get apps` keep the
subscriber's 2 s spacing; an `apps.inspect` whose stub `pacman` sleeps
2 s, asked a second after a frame, delays no frame: this test fails on
the old code, with the next frame 3.01 s late; `status.focus` null);
`tests/history.rs` 13 → 14 (the 30-day fill now writes a `top-apps` row
every minute; `top-apps` kept 3 days, `action` 30). `mirqab-proto` 18 →
19 (the idle column lines up), `mirqab` gains
`the_idle_column_shows_only_when_a_row_is_tagged`.
`tests/test-fixtures.sh` 7 → 14 checks (`--clones 1`), `tests/test-bench.sh`
gains the `--procs 400` run and `--procs 0`, `tests/test-cli.sh` the
`focus` checks.

Measured (2026-11-14, development machine, release build, fixture roots,
CPU from `utime + stime`):

| What | Number |
|------|--------|
| `tools/bench-idle.sh --subscribe apps --max-cpu 1.5 --procs 400 --runs 3` (416 processes, 10 clones of the 37 user processes) | 0.217 %, 0.200 %, 0.200 % of one core over 60 s; 10.3 MiB; within budget (1.5 %) |
| `tools/bench-idle.sh --runs 3` (idle, `trends.topApps` on, 46 processes) | 0.000 % ×3; 7.3–7.5 MiB |
| `tools/bench-idle.sh --procs 400 --runs 3` (idle, on, 416 processes) | 0.000 % ×3; 7.5–7.7 MiB |
| Idle at 416 processes, 180 s, `trends.topApps` on / off | 0.006 % (1 tick for three minute walks, so about 3 ms a walk) and 7.9 MiB / 0.000 % (0 ticks) and 7.4 MiB: the default stays on (owner decision 4) |
| `apps` subscribed, 60 s, with a fake Hyprland switching focus twice a second / with none | 0.017 % / 0.017 % (1 tick each); `status.focus` `{"source":"hyprland"}` and one `focus-hyprland` thread while subscribed; null and none 3 s after the subscription closed |
| `mirqab what` round trip, 20 calls: 1 100 DRM fds / 1 000 fds of which 8 DRM / the fixture's browser | before: 3.4 / 3.2 / 1.4 ms mean; after: 2.0 / 2.4 / 1.3 ms. Before, the 1 000-fd process reported 1 000 KiB of VRAM, its 992 non-DRM `fdinfo` counted; after, 8 KiB. Every inspect is under 4 ms, so N10's debounce (over 50 ms) is not added |
| The late frame: `apps` held by `socat` at 2000 ms, `mirqab apps` and `mirqab what 2001` every 3 s, 60 s | before and after: 29 gaps, mean 2000 ms, max 2000 ms. The brief expected 2.5 s before; see the deviations |
| 30-day `history.db` with 64 series and a `top-apps` row every minute (kept 3 days) | 41 394 176 bytes (39.5 MiB), under 40 MiB; with 30 days of rows it was 49 188 864 bytes |

Deviations from the brief, each with its reason:

- **`connect_timeout` uses `SO_SNDTIMEO`**, not a non-blocking connect
  and `poll` (above): Linux's `AF_UNIX` connect has no EINPROGRESS.
- **The focus source starts in `sample`, not in `wants`**: a `get`
  also calls `wants` (`server.rs`, the `get` path), so `mirqab apps`
  would have started a reader no `idle` would stop.
- **`idle` is called when the last subscriber leaves** (the server
  change above), which the brief assumed already happened.
- **`Focus::start` takes `own_uid`** (for `run/user/<uid>`), and the
  trait has `source()` for `status`; `focused` returns `Option` (None:
  no source) rather than a separate `Err`.
- **`top-apps` rows are kept 3 days, not 30.** Decision 7's estimate (7–8
  MB in a 40 MiB budget) missed that 64 series already take 38.4 MiB: a
  row a minute for 30 days measured 10.6 MB with its index (49.2 MB in
  all), and the leader and repeat rules do not bound the worst case (a
  busy machine with changing leaders). Three days of the worst case fit
  (39.5 MiB). Plan 018's spike rewind reads recent rows. The owner kept
  3 days (owner decision 5).
- **D1's "late frame" was not reproducible at 2 s** for `get apps`: the
  half-second wait came only when the subscriber's baseline was under a
  second old, so it ended within 1.5 s of the last tick and could never
  reach the next one. The inspector's `pacman` could (the protocol test
  shows a frame 3.01 s late on the old code); with the fixture's quick
  `pacman` neither showed in the measurement. D1 was fixed anyway, as
  decision 10 says: no sampler sleeps or asks another program while
  holding its lock (the focus source's bounded compositor waits are the
  exception, "Review of PR #20", item 3).
- **No debug log line for the inspector's cost**: it was measured from
  the client (`mirqab what`), which needs no new code.
- **`.flatpak-info` through `read_regular_prefix`** (O_NONBLOCK,
  O_NOFOLLOW, `fstat`), stricter than `symlink_metadata` then
  `read_prefix`.
- **No `mirqab config set` check in `tests/test-cli.sh`**: there is no
  `mirqab config` command; `trends.topApps` is checked in `config.rs`'s
  unit test, the schema test, and the protocol test's `config.set`.
- **`docs/cli.md` has no config list**; the schema documents the key.
- **The fake Hyprland in tests is Rust** (`tests/focus.rs`), two
  listeners in the test; `python3 -I` is 4b's, for `test-ui-apps.sh`.
- **`mkfixture --clones` clones the user's processes** (decision 12);
  the test-table row's "twice the rows" is "the user's rows twice".
- **The bench's `--subscribe` without `--live` fix** (above) is outside
  the brief and needed by its CI line.
- **`tests/test-bench.sh` gives its tray run and its `--procs 400` run
  a regression ceiling** (`--max-rss 18` and `21`, the measured debug
  RSS plus about 20 %: 15.2 and 17.1 MiB on 2026-11-14). It measures the
  debug build, whose RSS is about twice the release build's: on `main`
  its tray run already sat at 14.9–15.0 MiB against the 15 MiB budget.
  The 15 MiB budget is the release build's, gated by CI's `bench --tray`
  since the review of PR #20 (first `--max-rss 32` and no CI gate; S5).

Known limits (part 4a):

- Discovery takes the newest live instance; if its event socket refuses
  the connect, `start` fails and does not try an older one (the next
  page open tries again).
- A burst of more than ten new windows a second leaves the focus on the
  last answered one until the next event.
- The tag uses the time the reader saw each focus change (`Instant`), and
  the windows the sample's stamp; both are the daemon's monotonic clock.
- The bus-by-`comm` refusal is a guard rail for an honest
  `dbus-run-session` bus, not a boundary: a process may rename itself
  (`PR_SET_NAME` changes `comm`), so a bus named otherwise is not
  refused, and a process naming itself `dbus-daemon` cannot be quit from
  mirqab (review of PR #20, nit 9).
- ~~Deferred from the review of PR #20 (S4): a focus credits every group
  with a member in the focused pid's host unit.~~ Narrowed in 4b (owner
  decision 6): see "As built (part 4b)" and its Known limits. Under uwsm
  every app has its own `app-*.scope`, but apps a compositor `exec`s
  without `uwsm app` share its service or the session scope; in 4a one
  focused window there credited every group in that unit (and in the
  session scope every group of the login session), so nothing in it was
  ever tagged.
- ~~Deferred from the review of PR #20 (nit 3): the 1024-fd cap on the
  `fd/` listing leaves the GPU row out for the processes that matter
  most.~~ Fixed in 4b: the cap is on DRM `fdinfo` reads (64), and an
  1100-fd fixture of which 8 are DRM has its row.

Left for the owner (acceptance part 4, 1–5): the tag on a real
`yes > /dev/null &` after five minutes and its clearing on a click;
`focus: hyprland` and the `focus-hyprland` thread with the page open,
none closed; Hyprland restarted while the daemon runs; `top-apps` rows
after a busy ten minutes and none for a quiet stretch; the idle bench on
the development machine (above: 0.000 %). The owner answered the
retention question on 2026-11-14: three days of `top-apps` rows, and the
40 MiB budget stays (owner decision 5).

What 4b does: decision 16 whole (the caption and the sheet line,
Escape, S8, N3, N8, N4, `start` in `args` and the inspect params,
`APPS_ROW_GONE` on the gone error), its view-model and widget tests,
`tests/test-ui-apps.sh` with a fake Hyprland, architecture §7.2 and
§7.5, "As built (part 4b)", the plan's state and the README row; the
GPU row's cap on `fdinfo` reads instead of the `fd/` listing (review of
PR #20, nit 3).

Owner question for 4b (review of PR #20, S4): credit a focused window's
siblings only through `app-*.scope` units and the `session:` splits of
a terminal, so that apps sharing a compositor's service or the session
scope are not all credited by one window? **Owner, 2026-11-15: yes**
(owner decision 6 of part 4).

### Review of PR #20 (part 4a), 2026-11-14

The ruling (Fable 5.1, on Sonnet's review) kept 7 items, deferred 4 and
dropped the rest. What changed, with its test:

1. **B1, focus counts for any overlap with the window (S7 folded in).**
   The reader keeps every change of focus, "no window" included, as
   `(Instant, Option<pid>)`, the same focus again adding nothing, one
   entry older than 6 minutes kept as the anchor, at most 4096
   (`desktop/hyprland.rs:230`, `MAX_HISTORY` `:64`). One pure rule,
   `desktop::focused_since` (`desktop/mod.rs:50`), shared by the reader
   and the tests' `FakeFocus`: every pid whose time with focus overlaps
   `[since, now]`, including the last entry before `since`; a `None`
   anchor credits nothing. So a tag needs the whole window since focus
   was LOST (it tagged the browser two seconds after an alt-tab).
   Tests: `focus_counts_for_any_overlap_with_the_window`
   (`desktop/mod.rs:118`); `tests/apps.rs:863` rewritten (alt-tab at
   t0+7: the browser is not tagged at +8 or +10, tagged at +12);
   `no_window_in_focus_for_a_whole_window_tags` (`:943`);
   `tests/focus.rs:201` (2001 held into `s1` counts, "no window" gives
   nothing, three repeats of one address are one change, the hostile
   `>>` lines and a 200 000-byte line change and ask nothing).
2. **S1, one primed reading per `get`, oldest first (nit 4 folded
   in).** `prime` queues its reading (at most 16), `snapshot` takes the
   oldest whose age is 250 ms to 10 s and drops younger or older ones,
   `idle` clears the queue, in `Apps` and `Process` (`take_primed`,
   `samplers/apps/mod.rs:79`; `process.rs:325`). Tests:
   `each_get_is_served_its_own_primed_reading_oldest_first`
   (`tests/apps.rs:1270`: two primes 300 ms apart, both snapshots read
   cargo within 20 % of 364 %, the queue empty after, a 2 ms reading
   dropped for the inline path, `idle` clears) and
   `the_process_stream_serves_each_get_its_own_primed_reading`
   (`:1315`).
3. **S2 (reduced), the compositor waits are bounded and said.**
   `ask_active` (`hyprland.rs:135`) arms every read with what is left of
   one 500 ms deadline, so a trickled answer cannot hold the reader;
   `stop` (`:442`) joins within that deadline. Architecture §5.3 now
   says: no sampler sleeps or asks another program while holding its
   lock; the focus source may wait on the compositor up to 1.5 s once
   when the page opens and up to 0.5 s when it closes. Test:
   `a_trickled_answer_ends_in_time_and_stop_is_bounded`
   (`tests/focus.rs:351`: `stop` under 1 s during a one-byte-per-100 ms
   answer; a `start` whose request hangs ends within 1.2 s).
4. **S3, a dead signature does not win, and a dead reader is retried.**
   `live(root, dir)` (`hyprland.rs:163`: the lock's pid has a
   `proc/<pid>/` and the event socket is there) for the signature and
   the scan alike (`instance`, `:181`); `Apps` asks a source that is not
   running to start once per 30 s while subscribed (`FOCUS_RETRY`,
   `samplers/apps/mod.rs:579`), at once after `idle`. Tests:
   `tests/focus.rs:179` (a signature naming the dead instance yields the
   newest live one; a live signature itself) and
   `a_focus_source_that_is_not_running_is_asked_again_every_30_s`
   (`tests/apps.rs:966`).
5. **S5, the tray budget is gated on the release build.** CI's bench
   step runs `bench --tray` (`.github/workflows/ci.yml:131`; it fails
   without `dbus-daemon`); architecture §9.1 has the row. The debug
   test keeps ceilings of measured plus about 20 %
   (`tests/test-bench.sh:34`, `:51`: 18 and 21 MiB for 15.2 and
   17.1 MiB measured).
6. **The prompt `idle` is observed.** The `Counted` test sampler reports
   its idle count in `status` (`tests/protocol.rs:65`); the test polls
   it after the unsubscribe, before any re-subscribe (it stays 0 on the
   old code). The slow-pacman bounds widen to 2600 ms (`:1254`; the old
   code gave 3010).
7. **Docs and strings.** Architecture §5.3 (the lock sentence, the
   focus rule, the primed queue), §9.1 (the tray row; the 400-process
   row is a proxy: the fixture is files in the page cache and procfs on
   the live machine costs more per read); MCP `top_apps`'s description
   says when `idleHeavy` appears (`strings.rs:554`); the Known limits
   above (the bus guard rail, S4, nit 3).

Deferred: S4 and nit 3 (Known limits above, and 4b), S6 (plan 018's
Unverified), nit 2 (Unverified (part 4)). The fake-Hyprland bench
variant of S8 stays out.

Counts after the fixes (`cargo test -p mirqab-core`): library 69,
`tests/apps.rs` 30, `tests/focus.rs` 6, `tests/protocol.rs` 27 (the idle
test and the bounds changed), `tests/actions.rs` 24, `tests/history.rs`
14.

Measured after the fixes (2026-11-14, development machine, release
build, `--runs 3`, 60 s each): `--subscribe apps --max-cpu 1.5 --procs 400`
0.200 %, 0.200 %, 0.200 % of one core, 10.4–10.5 MiB; idle 0.000 % ×3,
7.3–7.4 MiB; idle with the tray registered (`--tray`, the new CI line)
0.000 % ×3, 7.4–7.6 MiB, within the 15 MiB budget. The debug build's
tray run 15.2 MiB and `--procs 400` run 17.1 MiB, the base of the test's
ceilings.


## As built (part 4b)

Part 4b (2026-11-15) is the interface side of part 4, checklist steps 7h,
8c and 4b's share of 7i, with the two items the review of PR #20 deferred
to it (S4 and nit 3). It closes plan 006. What was built, with where it
lives:

- **Narrow focus credit** (owner decision 6;
  `crates/mirqab-core/src/samplers/apps/focus.rs:68`, `credited`). A
  focused pid credits the group that holds it; when that group is a
  `session:` split, nothing more; otherwise, when the pid's unit is an
  `app-*.scope` (`app_scope`, `:46`), every group with a member in that
  scope; otherwise, when the pid is a terminal, the `session:` groups
  carved from its unit (a terminal server in a service, such as foot's
  `foot-server.service`); nothing else. The sid fallback outside the
  user manager is gone. Tests: the fixture test
  `a_terminal_credits_its_jobs_and_a_browser_only_itself` keeps 3000
  (foot) crediting `session:3030` (cargo), `session:3010` and its scope,
  and now asserts that 6202 in the login session's scope credits only
  `tree:6201`, no longer the compositor's tree (4a credited it through
  session 6200), and that 3041 (rustc, inside `session:3030`) credits its
  split alone; the new
  `apps_sharing_a_service_or_the_session_scope_credit_only_themselves`
  (`:259`, on a synthetic table, `table` `:229`) has two apps started by
  a compositor through `sh -c` in the session scope (two `tree:` groups
  in one session: each credits only itself), a non-app service holding a
  terminal server with `viewer` in one tab and `build` in another
  (`viewer`'s window credits `session:2010` and not `build`; the
  terminal's own window credits the service and both tabs), and a
  compositor's service whose apps form one group.
- **The GPU row's cap on DRM reads** (review of PR #20, nit 3;
  `inspect.rs:284`, `MAX_DRM_FDINFO` 64; `gpu` `:294`). `fd/` is listed
  whole and each link read, as the open files are; `fdinfo` is read only
  for DRM fds, at most 64, past which the row is left out. Test:
  `gpu_memory_comes_only_from_drm_fds_and_not_past_64_of_them`
  (`tests/apps.rs:1190`): 1 100 fds of which 8 are DRM (two clients) give
  `vramBytes` 1 MiB and `files.count` 1 100, the 1 092 others' parseable
  `fdinfo` never counted; exactly 64 DRM fds give the row (`:1230`, added
  by the review of PR #21); 65 leave it out. After the review the open
  files and the GPU row share one pass over `fd/` ("Review of PR #21",
  item 4). DRM fds per process on the development machine (counted by
  the reviewer, 2026-11-15): Hyprland 13; zen-bin, Xwayland and
  Firefox's RDD 6; the GPU processes of helium, Discord and t3code 5;
  nothing more than 13.
- **The tag on the page** (decision 16). `GroupLine.idle_heavy` and the
  label's last words (`crates/mirqab-view/src/tabs/apps.rs:381`, `:421`;
  `strings::apps_group_label` gains the flag, `strings.rs:1906`);
  `group_row.rs:64` adds `APPS_IDLE_HEAVY` ("busy while not in use",
  `strings.rs:1844`) as a `dim-label caption` after the process count,
  named `apps-idle-heavy`, shown only when set; `Selection.idle_heavy`
  (`apps.rs:659`) shows `APPS_IDLE_HEAVY_LINE` under the stats
  (`inspector.rs:172`, `apps-idle-heavy-line`). No colour, no icon.
- **Start times from the page** (decisions 8 and 9). `Selection.args` for
  a process row is `{"pid", "start"}` and `Selection.inspect_params` is
  `{"pid", "start"}` for a process and for a group's main process
  (`pid_params`, `apps.rs:663`); `inspect` sends them
  (`inspector.rs:427`), and `model::inspect_gone` (`apps.rs:818`: a
  `failed` reply naming the pid as another process now, or as no
  process, to a question that carried a start) makes the sheet say
  `APPS_ROW_GONE` with no verdict and no details (`gone`,
  `inspector.rs:461`).
- **Escape** (`page.rs:615`, `escape` `:307`). A `ShortcutController`
  with scope `Local` on the page's root; with a row selected Escape
  clears the selection (the sheet closes) and stops, with none it
  proceeds to the window's own controller, which hides a popover; in a
  window it then does nothing (plan 022). The modal dialog takes its own
  Escape first.
- **S8** (`page.rs:191`, `press` `:296`, the controller `:472`). A
  frame that arrives while a pointer is down on the list keeps its rows
  out of the store until the release, which writes the latest frame.
- **N3** (`unmapped`, `page.rs:322`): unmapping drops the subscription,
  the frame and the store's rows, and hides the sheet without clearing
  the selection (a `quiet` flag keeps the sheet's hide from counting as
  the user's).
- **N8** (`mark_selected`, `page.rs:53`): the selected state is set on
  the list item's widget once the row is mapped; the box keeps only its
  `selected` style class.
- **N4** (`apps.rs:518`): a group opened by a search (it matched only by
  members) has no expander.
- **Gap 5** (`client.rs:77`, `StreamSubs::next`, and `subscribe_params`
  `:89`, split out of `sync` so the decision is testable): the unit test
  `a_dropped_handle_leaves_the_others_params` (`:564`).
- **Gap 6** in `tests/test-ui-apps.sh`: a logging proxy (`python3 -I`,
  `:133`) in front of the daemon shows the graph's subscribe as
  `{"interval":1000,"live":true,"pid":2001,"start":5010}` and the
  inspector's `get` as `{"pid":2001,"start":5010}`.
- **The fake Hyprland** (`tests/test-ui-apps.sh:313`): a second daemon on
  a copy of the root under the short runtime directory, with
  `run/user/1000/hypr/fake/hyprland.lock` naming pid 1050 and a
  `python3 -I` fake (`:324`) whose event socket reports the browser's
  window and whose request socket answers `j/activewindow` with pid
  2001; the browser's and rustc's `stat` gain a core's worth of ticks a
  second; `MIRQAB_HEAVY_WINDOW_MS=4000`.
- **Docs**: architecture §5.3 (the narrowed focus rule), §7.2 (the
  caption in words only), §7.5 (the widget tests' `test` accessibility
  backend, the fake compositor in a fixture root, the two new widget
  names).

Tests (2026-11-15): `cargo test -p mirqab-core` library 69 → 70 (the
synthetic-table credit test), `tests/apps.rs` 30 (the GPU test
rewritten); `crates/mirqab-view/tests/apps.rs` 16 → 20
(`a_tagged_group_says_so_in_its_row_its_label_and_the_sheet` `:535`,
`a_group_opened_by_a_search_has_no_expander` `:582`, gap 8's
`only_the_selected_row_is_flagged` `:603`,
`an_inspect_error_for_a_reused_pid_means_the_row_is_gone` `:643`; the
selection test now checks `start` in `args` and `inspect_params`);
`mirqab-ui` unit tests 2 → 3 (gap 5); `crates/mirqab-ui/tests/widgets.rs`
10 → 16 under Broadway (`a_reused_pid_is_not_explained_and_a_tagged_group_says_why`
`:810`, `the_caption_shows_only_on_a_tagged_row` `:891`,
`escape_clears_the_selection_and_otherwise_goes_on` `:948` through the
page's real shortcut, `a_frame_during_a_press_waits_for_the_release`
`:980`, `the_list_item_carries_the_selected_state` `:1033`,
`a_remapped_page_shows_nothing_before_its_first_frame` `:1058`; the
existing inspector tests now expect `start` in the inspect params and
the redeem's args); `tests/test-ui-apps.sh` 67 → 78 checks (no tag and
`status.focus` null without a source; the two proxied requests; with
the fake: `status.focus` `hyprland` and one `focus-hyprland` thread while
the page is open, cargo tagged, the focused browser not, then `focus`
null and no reader thread once it closed).

Deviations from the brief, each with its reason:

- **S8 watches raw events, not a `GestureClick`.** An
  `EventControllerLegacy` in the capture phase on the `ListView` sees
  the press before the rows' own gesture and is not cancelled when that
  gesture claims the sequence (a `GestureClick` on the list could be);
  the release is handled in an idle callback after its event, because
  the row activates on that same release, and must read the held rows.
  A grab broken or a touch cancelled counts as a release; unmapping
  clears the press.
- **N8's state is on the list item's widget**, not on `gtk::ListItem`:
  `ListItem` is no widget and has no state setter (only an accessible
  label and description, GTK 4.12). It is set only once the row is
  mapped: at bind time the list item's widget may not be parented yet,
  and gtk-rs's `parent()` then sank its floating reference and dropped
  it, which tore the item down inside the bind (found by the widget
  test: "RefCell already borrowed" in `teardown`). A row bound off
  screen gets its state when it maps.
- **The widget tests run with `GTK_A11Y=test`** (`tests/test-ui-widgets.sh`),
  not `none`: with `none` there is no accessible context and
  `gtk::test_accessible_has_state` reads nothing. The test backend talks
  to no bus. GTK gives no getter for a state's value, and GTK itself
  writes `Selected(false)` on every list item under `NoSelection`, so
  "the state is set" proves nothing. Since the review of PR #21 the page
  records the value it wrote on each list item (`Item.applied`, through
  `write_selected`, which returns what it wrote), and N8's test reads it
  through `Handle::marked`: the selected row true, the others false,
  also after a reordering frame rebinds the items, and none true once
  cleared. Without the `mark_selected` calls the test fails; the
  `update_state` call itself is covered only by the Orca owner check
  (review of PR #21, delta). It proves the page's bookkeeping; what a screen reader reads is an owner check (Orca). A
  test that reads GTK's value needs `gtk_test_accessible_check_state`,
  variadic and unbound by gtk-rs, so `unsafe` in a test, which the
  repository keeps to `sys.rs`.
- **N3 is tested in the widget test**, not in `tests/test-ui-apps.sh`: on
  a re-map the daemon samples at once, so the first frame races the
  dump; the widget test hides and shows the page with no daemon and
  finds no rows until it feeds a frame.
- **The fixture for owner decision 6 is a synthetic table in
  `focus.rs`'s tests**, not rows in `procs.tsv`: new rows change the
  golden frame and the group counts five test files assert. And "two
  apps in one non-app service" cannot be two groups by itself: rule 1
  makes one group of a unit unless its main process is a terminal, so
  the compositor's own service with the apps it `exec`s is one row (the
  test asserts that). The fixture's two apps in one service are run
  from a terminal server's shells (two `session:` splits), and the
  session-scope pair is two `tree:` groups started through `sh -c`.
- **A terminal in a service credits its tabs**, beside the decision's
  `app-*.scope` clause: without it a footclient window's own jobs (a
  build in a tab the user is reading) would be tagged while in view. Its
  credit reaches only the `session:` splits of its own unit, never
  another app in that service.
- **`inspect_gone` also takes `no process N`** when a start was sent:
  the row named a process that is no longer anywhere, which is the same
  "that row is gone" for the person reading the sheet.
- **A process row never carries the tag line**: the tag is a group's
  (`AppGroup.idleHeavy`); a selected process inside a tagged group shows
  the group's caption in the list, not a line in the sheet.
- **Past 64 DRM fds the GPU row is left out**, not summed in part:
  counting some clients would understate VRAM with no sign of it.
- **`page::build_page` and `page::Handle`** are new public items for the
  widget tests (frames fed by hand, a press held, the rows, the written
  selected states and Escape read back); `build` is `build_page(..).0`.
- **The sheet is pinned** (`split.set_pin_sidebar(true)`, `page.rs:583`,
  review of PR #21): libadwaita's `set_collapsed` hides an unpinned
  sidebar, and the page took that for the user closing the sheet and
  cleared the selection. This was the Home test's flake since part 3b,
  and a product bug: a window shrunk under 60 em lost its selection.

Known limits (part 4b):

- Escape is the page's only while focus is inside it (scope `Local`):
  with focus in the sidebar it goes to the window, and a search entry
  with text takes Escape to clear itself first (GTK's own binding).
- A press held on the list freezes its rows until the release, or for
  two frames (some 4 s), after which the press is taken as lost and the
  third frame is written; frames are not lost, the latest is written.
- `inspect_gone` matches the error's message text
  (`strings::apps_process_gone`, `apps_no_process`). A `data.gone` flag or
  an error code of its own belongs to the plan that next changes the
  protocol (plan 007's `Ports` errors, or plan 018).
- Outside the user manager (no app scopes) a terminal and its jobs are
  `tree:` groups with different sessions; the terminal's window credits
  only its own group, as in 4a.
- N6 stays deferred to the plan that adds reconnects (decision 16).
- The Broadway checks cannot show the popover or real pointer presses;
  Escape in a popover and S8 under a moving CPU order are owner checks
  (acceptance part 4, 6–7).

Left for the owner, all on the development machine: acceptance part 3,
1–6, and part 4, 1–4, 6 and 7 (listed in the State line above), the
interface's cost with Apps open on Wayland, and the Flatpak window pid.

### Review of PR #21 (part 4b), 2026-11-15

The ruling (Fable 5.1, on Sonnet's review) kept 6 items, deferred 3 and
dropped 3. What changed, with its test:

1. **The Home flake was the collapse clearing the selection.**
   `adw_overlay_split_view_set_collapsed` hides an unpinned sidebar; the
   60 em breakpoint applied after the first frame showed the sheet, and
   `connect_show_sidebar_notify` read the hide as the user's and cleared
   the selection (the reviewer's instrumented failing run shows that
   sequence; 3 failures in 8 runs on the PR head). Fix:
   `split.set_pin_sidebar(true)` (`page.rs:583`). Test:
   `collapsing_the_page_keeps_the_selection` (`widgets.rs:1141`): with a
   row selected, `set_collapsed(true)` and then a 300 px window under
   the breakpoint keep the selection and the sheet; an explicit hide
   still clears it. Without the pin it fails ("still selected").
   `tests/test-ui-home.sh` repeated: see the counts below.
2. **N8's test now discriminates** (see the deviation above):
   `Item.applied` and `write_selected` (`page.rs:54`, `mark_selected`
   `:65`), `Handle::marked` (`:400`); `the_list_item_carries_the_selected_state`
   (`widgets.rs:1033`) asserts true for the selected row, false for the
   others, true again after a reordering frame, none once Escape
   cleared it. With the `mark_selected` calls removed it fails (checked);
   removing only the `update_state` line leaves it green, so that call
   is covered by the Orca owner check alone.
   The ruling's preferred test (GTK's value through a test-only
   `#[allow(unsafe_code)]`) was not used: `unsafe` stays in `sys.rs`.
3. **A lost release cannot hold the rows for long**: a press holds at
   most two frames (`HOLD_FRAMES`, `page.rs:143`, `:217`); the third is
   written and the press taken as lost; `press` and `unmapped` reset the
   count. Test: `a_press_the_list_never_releases_lets_frames_through`
   (`widgets.rs:1109`).
4. **One pass over `fd/`**: `fd_targets` (`inspect.rs:247`) lists `fd/`
   once and reads each link once; `open_files` (`:265`) and `gpu`
   (`:304`) take its list (`:550`). The GPU test gains the exact-64 case
   (`tests/apps.rs:1230`). Re-measured (2026-11-15, development machine,
   release build, fixture daemon, `mirqab what --json`, 20 calls, three
   runs, CLI start-up included): the 1 100-fd process with 8 DRM fds
   1.43–1.44 ms a call, the fixture's browser 0.83–0.86 ms.
5. **Test helpers die with the sandbox**: `tests/lib.sh` (shared harness)
   gains `HELPER_PIDS`, killed by `_cleanup_scratch`; the fake
   Hyprland, the proxy and the tick loop in `tests/test-ui-apps.sh`, the
   `--watch` reader in `tests/test-bar.sh` and the `--background`
   interfaces in `tests/test-ui-shell.sh` and `tests/test-ui-home.sh`
   join it. Checked by hand: with a `false` put after the loop's start
   in `tests/test-ui-apps.sh`, the file aborts and no `hypr.py`,
   `proxy.py` or root-reading process is left (then removed).
6. **README**: plan 006 is done in the status paragraph, and the
   popover's Escape says the first Escape clears an Apps selection.

Counts after the fixes (2026-11-15): `crates/mirqab-ui/tests/widgets.rs`
16 → 18 under Broadway (two new, N8's rewritten); `tests/apps.rs` 30
(the GPU test gains the 64 case); `tests/test-ui-home.sh` repeated 38
times: 36 runs 55/55, and the "with that group in the inspector" check
never failed; 2 runs aborted with every interface check failing from
the first `--selftest` on (the interface never drew a page, so `mirqab
open` then found no interface and tried the stubbed `systemctl`). That
failure did not come back in 16 further runs of a copy that prints the
selftest's output, nor in 6 rounds of a bare Broadway, daemon and
`--selftest home` loop; its cause is not known (the machine was shared
with other builds), and it is not the collapse. The same whole-file
failure came once in `tests/test-ui-shell.sh` during a full
`--affected` run (that file builds no Apps page), then passed in 12
runs alone and in the next full run (699/699): a harness flake to find
when it is caught with the selftest's output.

Deferred: real key and pointer dispatch for Escape and S8 (owner checks,
in the State line, with the search entry's Escape); `inspect_gone`'s
text match (Known limits (part 4b), for the next protocol plan); an
AT-SPI readback of the selected state (owner check, Orca). Dropped: a
real-machine basis for the 64 cap (counted: 13 at most, recorded above),
"freezes for good" (the next click or an unmap clears a press), and a
longer wait in the Home test (the cause was item 1).


## Unverified

- Hyprland's event socket path and the `activewindow` event format:
  `hyprctl` could not reach the compositor from the planning shell. Check
  https://wiki.hypr.land/IPC/ and a live session; record a sample line as a
  fixture. 2026-11-14: the socket
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`
  exists on the development machine **[machine]**; no event was captured
  yet. Part 4a: an 8 s read of the live socket from the build shell caught
  no event either; the reader is written to the wiki's format, accepts
  the address with or without `0x`, and is tested on synthetic lines
  ("Unverified (part 4)").
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
- ~~Whether an `adw::AlertDialog` presented on the
  `adw::ApplicationWindow` is reached by `first_child` / `next_sibling`
  from the window~~ Resolved 2026-11-14 under Broadway (GTK 4, libadwaita
  1.6 bindings): it is. The dump of `--selftest apps --do "…; activate
  apps-quit; wait apps-dialog"` holds a node named `apps-dialog` whose
  labels are the heading `This will quit:` and the body naming pids 2000,
  2001, 2010, 2011 and 2012 (`tests/test-ui-apps.sh`). Nothing is added
  to the dump by hand; `respond` takes the dialog from the window's
  `visible_dialog()`.
- ~~Whether `gtk::Widget::activate` on a `ListView` item under
  `NoSelection` emits `activate` for its position~~ Resolved 2026-11-14
  under Broadway: the row's own box has no activate signal, so
  `activate()` on it returns false, and its parent, the list item widget,
  activates its position (`list.activate-item`), which the page's
  `connect_activate` receives. The `activate` step walks up from the named
  widget to the first that activates (`selftest.rs`, `step`). A
  `gtk::Switch` animates its toggle, so the step sets it directly.
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
  owner check. 2026-11-14, under Broadway on the development machine (the
  sandbox unsets `XDG_DATA_DIRS`, so GTK reads `/usr/share`): the
  fixture's `example-browser` is in no theme and shows the glyph, which
  the test asserts; the fixture's Foot entry (`Icon=foot`) showed the
  theme's own `foot` icon, because the machine has foot installed. So the
  theme is read under Broadway; CI's runner may have no such icon, and
  the test asserts only the fallback.
- The quoting of `systemctl show` list values on systemd 257 (checked on
  262.1 only; review of PR #18).
- That a distribution shipping the reference bus (dbus-daemon) names its
  user unit `dbus-daemon.service` with `dbus.service` as an alias (said
  of Fedora by the review of PR #18 from memory; the development machine
  runs dbus-broker). Both names are refused either way.
