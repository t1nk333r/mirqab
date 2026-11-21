# Plan 007: Ports tab

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MEDIUM. Stop kills processes; "exposed" is a security claim
  and must not overstate or understate.
- **Depends on**: 001, 005, 006
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: IN PROGRESS (2026-11-20). Six parts (below), each its own
  pull request. Part 1a (the `ports` stream, who listens where, `mirqab
  ports` and `mirqab port`) is built; parts 1b, 2a, 2b, 3 and 4 are not.

## Owner decisions (2026-11-20)

The four questions of the implementation brief (2026-11-20), each
answered with its recommendation:

1. **Stop on a Docker-published port** runs `docker stop -t 5 -- <id>`
   when `docker ps` works for the user: the Docker daemon the user may
   already use does it, and mirqab signals nothing of root's. A user who
   cannot run `docker ps` sees `denied` and no Stop (part 2a).
2. **The tray's attention for exposed ports is on by default**: lite
   (counts only), your own processes only (root's `sshd` and Docker's
   proxies never trigger it), and gated by the idle bench (part 4).
3. **The MCP tools give a port's project name and framework only**,
   never its path (part 1b).
4. **Other agents' left-behind servers are left to plan 008**, which adds
   their markers; until then the page's empty attention state says so
   (part 3).

## Decisions that differ from the text below (2026-11-20)

The implementation brief changed the design in these ways; the sections
below are the plan as written on 2026-11-13.

1. **Six pull requests** (the part list below), not one.
2. **Open, Copy URL and Reveal folder belong to the interface**, through
   GTK (`gtk::UriLauncher`, the display's clipboard, `gtk::FileLauncher`):
   the daemon is a user service with no display. `ports.open`,
   `ports.copy` and `ports.reveal` are not actions; `mirqab port N`
   prints the URL.
3. **The QR code is a query**, `get ports.qr {port}`: it changes nothing,
   so it has no `dryRun` and no `confirm`.
4. **Stop is two actions, `ports.stop` and `ports.kill`**, as
   `apps.quit` and `apps.kill` are (a plan is bound to its action's
   name); `mirqab kill PORT --force` picks `ports.kill`.
5. **Stop resolves a port to processes and hands them to plan 006's
   engine** (`actions::Subject::Port`): the refusals, the guard, the
   plans and the `action` event are the existing code paths.
6. **A container is stopped through its runtime**, `docker stop` or
   `podman stop`, never by signalling inside it.
7. **"What probably restarted it" is told by the client after the
   fact**: the CLI reads `get ports` once 3 s after a confirmed stop and
   the page reads the next frame; the daemon's reply returns at once.
8. **`family` became `families`** (`[4]`, `[6]` or `[4, 6]`); an
   owner's `0.0.0.0` and `::` sockets (or `127.0.0.1` and `::1`) fold
   into one row.
9. **Addresses are classified with `getifaddrs`** (`sys::ifaddrs`) on
   the machine and `run/mirqab-test/ifaddrs.json` under a fixture root,
   the interface's kind from `sys/class/net/<name>/`. Rejected:
   `proc/net/fib_trie` (no interface per address), netlink (more
   `unsafe`), `ip -j addr` (a process per refresh).
10. **`docker ps` and the system snapshot's `ss` are read through the
    action `Sender`**: the real one runs them, a fixture root's answers
    from files under the root. No stub binaries (the "Tests and
    fixtures" text names stub `docker`, `pkexec`, `ss`, `xdg-open` and
    `wl-copy`; none is needed).
11. **The lite view**: `params.lite: true` gives counts only, and while
    every subscriber is lite the sampler reads the socket tables alone.
    The tray, the sidebar badge and the bar item use it.
12. **`exposedCount` counts your own sockets only** (rows whose owner is
    you, including one whose process was not found). A root listener
    carries the `exposed` pill in its row but does not colour the badge
    or the tray: you cannot act on it.
13. **The MCP `Ctx` is small** (the socket and the `via` tag): plan 006
    part 2 moved plans, the ancestry walk, the clock and every `Root`
    read into the daemon (checklist step 6).
14. **The new fixture processes join `root-desktop`**, not a new tree,
    so one imagined machine serves every tab.

## Parts

| Part | What | State |
|------|------|-------|
| 1a | The `ports` stream: the socket tables, the owner map, scopes, the lite view; `mirqab ports`, `mirqab port N` (basic) | built (below) |
| 1b | Project, framework, launcher, containers (`docker ps`); `mirqab port N` in full; MCP `list_ports`, `whats_on_port` | planned |
| 2a | Stop and force stop a port (`ports.stop`, `ports.kill`, `mirqab kill`); the system snapshot (`ports.system`, `mirqab ports --system`) | planned |
| 2b | `kill_port`, the first MCP tool that acts: `Ctx`, strict arguments, `via: "mcp"`, the intent row | planned |
| 3 | The page, its rows and the `ports.list` card; the shared act flow | planned |
| 4 | QR for the phone, the system section, the sidebar badge, the tray's attention, `mirqab bar ports` | planned |

## Problem

"What's on :3000, and who started it?" Stat Space lists every listening
port with its process, the project folder it runs in, the framework, what
launched it (a terminal, an editor, an agent, the service manager, Docker),
how long it has been up, and whether other devices on the network can
reach it; with Open, Copy URL, Reveal folder, QR for phone and Stop
(https://statspace.app/features/ports, read 2026-11-13). The owner asked
for "listening sockets traced to project and launcher".

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Listening TCP sockets | `/proc/net/tcp`, `/proc/net/tcp6`: rows with state `0A`; columns local address:port (hex), uid, inode **[machine]** | no |
| Bound UDP sockets | `/proc/net/udp`, `/proc/net/udp6` (off by default; a switch) | no |
| Socket → process | `/proc/<pid>/fd/*` links reading `socket:[<inode>]`, for the user's own processes | own processes only |
| Root-owned listeners' processes | Not readable as the user. `ss -Hltnp` shows `users:(…)` only for the caller's sockets **[machine]** | **yes**, opt-in snapshot: `pkexec ss -Hltnp` |
| Unix listening sockets | Out of scope (not "ports") | — |
| Process details | Plan 006's process cache | no |
| Project folder | `/proc/<pid>/cwd`, then walk up to the nearest directory containing one of `.git`, `package.json`, `Cargo.toml`, `go.mod`, `pyproject.toml`, `Gemfile`, `composer.json`, `deno.json`, `mix.exs`, `pom.xml`, `build.gradle`; stop at `$HOME` | no |
| Framework | The command line (`next dev`, `vite`, `rails s`, `manage.py runserver`, `uvicorn`, `flask`, `php artisan serve`, `hugo server`, `astro dev`, `webpack serve`, `jupyter`…) and, for Node, the `dependencies` keys of the project's `package.json` (names only) | no |
| Launcher | Parent chain from plan 006, and marker variable **names** in `/proc/<pid>/environ` that survive re-parenting. Claude Code children carry `CLAUDECODE` and `CLAUDE_CODE_SESSION_ID` **[machine]**; others in plan 008 | own processes |
| Uptime | Process start time (`stat` field 22) against boot time | no |
| Local addresses | `getifaddrs`, to classify a bind address | no |
| Docker mapping | `docker ps --format '{{json .}}'`: `Names`, `Image`, `Ports`; the user is in the `docker` group **[machine]**; skipped when not | no (group) |
| Open in browser | `xdg-open` (present **[machine]**) | no |
| Copy | `wl-copy` (present **[machine]**) | no |
| QR code | Generated in the daemon; `qrencode` is present **[machine]** but must not be required | no |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| "Terminal window 2, tab 1" | Replaced: the terminal's name and the shell's pid. No terminal exposes window and tab portably |
| Stop a third-party launch daemon with a password | Replaced: for a **user** service, `systemctl --user stop`. System services are shown in the System section and never stopped by this app; the row gives the `systemctl stop <unit>` command to copy |
| Firewall awareness | Not attempted: "exposed" means the bind address, and the text says so |

## Design

### Stream `ports`

Every 5 s while subscribed (ports change slowly; a "Refresh" control
forces one). The inode-to-pid map is rebuilt only when the set of
listening inodes changes.

```json
{"ports":[{"port":3000,"proto":"tcp","bind":"127.0.0.1","family":4,"exposed":false,
           "pid":4242,"process":"node","label":"next dev","rssBytes":0,"uptimeSec":2460,
           "project":{"name":"acme-web","path":"~/code/acme-web","framework":"Next.js"},
           "shell":{"name":"zsh","pid":4200},
           "launcher":{"kind":"agent","name":"Claude Code","sessionEnded":false},
           "container":null,"unit":null,"url":"http://localhost:3000"}],
 "exposedCount":1,
 "system":{"loaded":false,"at":null,"ports":[]}}
```

- **Exposed.** `exposed` is true when the bind address is `0.0.0.0`, `::`,
  or an address of a non-loopback interface. Addresses on container
  bridges and VPN interfaces count as exposed to *that* network and are
  labelled by interface kind (`bridge`, `vpn`, `lan`), never by interface
  name in fixtures or docs. The warning text: "Listening on every
  interface, so other devices on your network can connect, unless a
  firewall blocks it. Bind it to 127.0.0.1 to keep it on this machine."
  This app does not inspect firewall rules.
- **Dual-stack.** A `::` listener with `bindv6only=0` also accepts IPv4;
  one row, marked as both families.
- **Launcher kinds**: `terminal` (known terminal names in the parent
  chain), `editor` (code, cursor, zed, jetbrains…), `agent` (plan 008's
  markers), `service` (a `--user` unit that is not an app scope),
  `container`, `unknown`. Markers beat the parent chain, because a server
  left behind by an ended agent session has been re-parented to the user
  manager. `sessionEnded: true` makes the row "left behind".
- **URL**: `http://localhost:<port>`; `https` when the port is 443 or 8443.
  No probing of the port.
- Paths are shown with `~`; the JSON carries the same form.

### System section

Off by default (`config.ports.system`). When on, the page gets a "System"
section with a Load button. Load runs `pkexec /usr/bin/ss -Hltnp` once
through `elevate.rs` (a fixed argv, nothing interpolated), parses it, and
shows a snapshot with its time. It never refreshes by itself. Rows are
read-only. Without Load, root-owned listeners still appear, from
`/proc/net/tcp`, with port, bind address and "owned by root", but no
process.

### Actions

- `ports.stop {port}`: SIGTERM to the listening process (SIGKILL with
  `force`), or `systemctl --user stop` for a unit, or `docker stop
  <container>` for a container. Needs `confirm`. If the port is listening
  again within 3 s, the reply says what probably restarted it (a unit with
  `Restart=`, a process manager in the parent chain, a container restart
  policy).
- `ports.open {port}`, `ports.copy {port}`, `ports.reveal {port}`
  (`xdg-open` on the project folder): not destructive, no confirmation.
- `ports.qr {port}`: returns the QR matrix for
  `http://<LAN address>:<port>`. Offered only for exposed ports; for a
  loopback port the control explains why not.

### QR

A Rust QR encoder in the daemon returning a bit matrix. The interface
draws it in a custom widget (`crates/mirqab-ui/src/widgets/qr.rs`) as
**one** filled `gsk::Path` holding a square per dark module, on a light
quiet zone, with `GtkSnapshot` (architecture §7.4); the module rectangles
come from `mirqab_view::qr` and are tested there. Nothing is adapted from
haseen's QML drawing. The encoder is either a small crate with no
dependencies (`qrcodegen` 1.8.0 **[crates.io]** is the candidate; named
and justified in step 1) or a port of haseen's.

### Page

- Header: count, exposed count, the UDP switch, Refresh.
- A "Needs attention" group first: exposed ports and left-behind servers.
- One row per port: `:3000`, the label, project and launcher, uptime, and
  an "exposed" pill. Expanding shows Process, Project, Shell, Launcher
  lines and the five buttons.
- The sidebar tile shows the port count; it turns to the warning colour
  when a port is exposed. The tray icon takes the attention state when
  `config.ports.warnExposed` is on (default on).
- Pinnable card `ports.list`: the first six rows.

### CLI and MCP

`mirqab ports [--system] [--udp] [--json]`, `mirqab port <n>`,
`mirqab kill <n> [-y] [--force] [--dry-run]`. MCP: `list_ports`,
`whats_on_port`, `kill_port` (plan 005's confirm flow). Bar item `ports`.

## Rejected

- **Shelling out to `ss` or `lsof` for the normal list.** A process every
  refresh; `/proc/net/tcp` plus the fd scan is cheap and testable against
  a fixture.
- **`sock_diag` netlink for the list.** Faster on machines with tens of
  thousands of sockets, but it cannot be faked by a fixture root. Revisit
  only if the benchmark fails.
- **Probing ports to detect HTTP or a framework.** The app would be making
  requests to the user's servers.
- **Checking nftables.** Needs root to read, and a wrong "safe" is worse
  than an honest "bound to every interface".
- **Killing by port without showing the process.** The contract is to name
  what will stop.

## Tests and fixtures

Add to `root-desktop`: `proc/net/{tcp,tcp6,udp,udp6}` with listeners on
loopback, `0.0.0.0`, `::`, a LAN-range documentation address and a bridge
address, owned by uid 1000 and uid 0; `proc/<pid>/fd/` links for the
user's processes; `proc/<pid>/cwd` links into `home/user/code/<project>/`
trees with marker files and small `package.json` files; `proc/<pid>/environ`
with marker names and dummy values; a stub `docker`, a stub `pkexec` that
only records, a stub `ss`, stub `xdg-open` and `wl-copy`. An interface
table fixture for address classification (a JSON file the sampler takes in
place of `getifaddrs` when under a fixture root).

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/ports.rs` | Hex address and port parsing for both families; only state `0A`; inode-to-pid; a root listener has no pid and is not guessed; dual-stack is one row |
| same | Project discovery stops at the marker and at home; framework from command line and from `package.json`; launcher from markers beats parent chain; "left behind" |
| same | `exposed` for each bind kind; the count |
| same | Actions: `wouldDo` for process, unit and container; refusal for a root-owned port; restart detection text |
| same | System snapshot: the exact `pkexec` argv; parsing of `ss -Hltnp` |
| `crates/mirqab-core/tests/qr.rs` | Known URL to known matrix (a reference vector from the QR specification's examples), sizes for 20 to 80 characters |
| `tests/test-ui-ports.sh` (`mirqab-ui --selftest ports`, plan 001) | Rows, the attention group, the dialog, the QR widget reports a non-empty module count |
| `tests/test-cli.sh` | `ports`, `port 3000`, `kill 3000 --dry-run` |

## Acceptance

1. A dev server started in a project folder from a terminal shows with its
   project, framework, terminal and uptime within 5 s.
2. One started by a coding agent shows the agent as launcher, and "left
   behind" after the agent exits.
3. `python -m http.server --bind 0.0.0.0` is flagged exposed with the
   explanatory text; `--bind 127.0.0.1` is not.
4. Stop asks, names the process and ends it; a root-owned port offers no
   Stop.
5. The QR opens the URL on a phone on the same network (an owner step).

## Checklist

1. Resolve Unverified; choose the QR encoder and record why.
2. `crates/mirqab-core/src/samplers/ports/{mod,procnet,owner,project,framework,launcher,exposed,docker}.rs`.
3. `…/ports/{actions,system}.rs`; the fixed `pkexec ss` in
   `crates/mirqab/src/elevate.rs`.
4. `crates/mirqab-core/src/qr.rs`.
5. `tests/golden/ports.json`; tests.
6. CLI `ports`, `port`, `kill`; MCP tools; bar item. `kill_port` is the
   first MCP tool that acts, so it takes on what plan 005 deferred
   (review of PR #13) and plan 006 part 2 did not already land:
   - a `Ctx` for tool handlers (plans, the ancestry, `Root`, a clock), a
     per-tool `input_schema`, and strict arguments on acting tools;
   - plan 005's rule 4 for the MCP server's own ancestry: the mechanism
     (the socket peer's pid, walked strictly, refused) is in the daemon
     since plan 006 part 2; the MCP server's connection and `kill_port`
     use it here, and every act through MCP adds `"via": "mcp"` to its
     `action` event;
   - Deferred from the review of PR #17 (D3): with that change of the
     `action` row's shape, write an intent row before the act (so a crash
     mid-batch leaves a trace) and put each target's `start` and the plan
     id in the row;
   - plan 005's acceptance 3 repeated through MCP (`kill_port` called
     twice, a recording sender, no real signal);
   - a unit test that every tool's description leads with its side
     effect and every acting tool sets `destructiveHint`.

   Moved to plan 006 part 2 (2026-11-14; see "What moves in from plan 007
   step 6" there): targets as `{pid, start}` compared again at redeem and
   before the signal; rule 2, the daemon re-computing the targets at
   `confirm: true`; rule 5, every act written to the `event` table (kind
   `action`); acceptance 3 for the daemon and the CLI; `confirm` a strict
   boolean on the daemon side; a cap of 64 live plans; the ancestry walk
   failing closed (`guard::ancestry_strict`); plan expiry on
   `CLOCK_BOOTTIME`.
7. `crates/mirqab-view/src/tabs/ports.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/ports/{mod,page,port_row,port_detail,qr_sheet,system_section,list_card}.rs`;
   `crates/mirqab-ui/src/widgets/qr.rs`, `crates/mirqab-view/src/qr.rs`.
8. Sidebar badge and tray attention.
9. Append `ports.list` to the Home seed list.

## Unverified

Resolved on the development machine on 2026-11-20 (read-only, except one
throwaway `python3 -m http.server --bind ::1` the planner started and
ended by its own pid; addresses are loopback, names scrubbed):

- **`/proc/net/tcp6` byte order: resolved.** The listener on `[::1]:48811`
  printed as
  `00000000000000000000000001000000:BEAB 00000000000000000000000000000000:0000 0A … 1000 0 13212575 …`:
  four 32-bit words in address order, each the in-memory `u32` printed
  with `%08X` (little-endian here, so the last word `01000000` is
  `00 00 00 01`); the port is big-endian hex (`BEAB` = 48811); the
  process's fd link `socket:[13212575]` matched the inode column.
  `0000000000000000FFFF00000100007F` is `::ffff:127.0.0.1`; in
  `/proc/net/tcp`, `0100007F:1F90` is 127.0.0.1:8080 and `00000000:0016`
  is 0.0.0.0:22. The header line is
  `  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode`
  (`udp` adds `ref pointer drops`), and a bound UDP socket is state `07`
  (the stub resolver, `3500007F:0035`). `/proc/sys/net/ipv6/bindv6only`
  is 0. The fixture's tables follow this layout
  (`tests/fixtures/root-desktop/proc/net/`).
- **`environ` of a re-parented process: resolved.** All 12 children of
  the user manager were read: every same-uid process answered (22 to 197
  variable names) except the manager's own `(sd-pam)`, which is
  non-dumpable (its `exe` link is refused too). The reader treats an
  unreadable `environ` as "no markers" (part 1b). The names the plan
  relies on, `CLAUDECODE` and `CLAUDE_CODE_SESSION_ID`, are present in an
  agent's environment.
- **`docker ps --format '{{json .}}'`: resolved.** Docker 29.9.2, the
  user in the `docker` group; fields `Command, CreatedAt, HealthStatus,
  ID, Image, Labels, LocalVolumes, Mounts, Names, Networks, Platform,
  Ports, RunningFor, Size, State, Status`; `ID` is 12 hex characters;
  `Ports` reads `0.0.0.0:8088->80/tcp, [::]:8088->80/tcp`, or empty for a
  container that publishes nothing (part 1b).
- **`ss -Hltnp`'s shape** (for part 2a's parser): five columns and a
  sixth `users:(("NAME",pid=N,fd=M))` only for the caller's own sockets.
- **Interface kinds under `sys/class/net`** (names scrubbed): bridges
  have `DEVTYPE=bridge` and a `bridge/` directory; a bridge's ports a
  `brport/`; loopback is `type` 772; physical interfaces have `device/`;
  a tun interface is `type` 65534 with `tun_flags`; a wireless one has
  `DEVTYPE=wlan`, `device/` and `wireless/`.
- **Still open, for part 4's bench (review of PR #22, ruling 7):** UDP
  autobind churn forces a rebuild per tick while a `udp` subscriber
  exists; `get {udp}` followed by a TCP-only tick rebuilds twice. Part
  4's bench row also runs `--subscribe ports --udp` and decides the
  staleness rule if the churn shows.
- **Still open, an owner check after part 2a:** whether PolicyKit shows
  `pkexec /usr/bin/ss` with a sensible prompt. No policy for `ss` exists
  under `/usr/share/polkit-1/actions`, so it will be `pkexec`'s generic
  prompt, and the daemon has no login session of its own (polkit's
  `sd_uid_get_display` should tie it to the owner's); if the prompt is
  poor, `dist/polkit/org.mirqab.ports.policy` is the next step.

## As built (part 1a)

Part 1a (2026-11-20, with the review of PR #22 below): the `ports`
stream, who listens where, and the read commands, as the brief's §3.1,
§3.2, part 1a and §9 describe, with the owner's decisions above. What was
built, with where it lives:

- **The stream** (`crates/mirqab-proto/src/stream.rs:30`, added to
  `STREAMS` at `:12`): `PortsData` (`:383`) with `ports`, `exposedCount`,
  `leftBehindCount`, `total`, `udp`, `docker` and `system`; `PortRow`
  (`:405`), with `families` in place of the plan's `family` and `pids`
  (`:428`); the part 1b shapes `PortProject`, `PortShell`,
  `PortLauncher` and `ContainerRef`, always null in 1a; `SystemPorts`
  (`:489`), never loaded in 1a; and `PortsLite` (`:511`). A row is
  `(port, proto, bind pair, owner)`, the owner a tree of the user's
  processes or a uid. Golden: `tests/golden/ports.json` (`get ports
  {"udp": true}` on `root-desktop`, 13 rows).
- **The sampler** (`crates/mirqab-core/src/samplers/ports/mod.rs`),
  registered after `process` in its own slot (`server.rs:391`). `listen`
  builds the sender once (`server.rs:388`) and hands the same `Arc` to
  `Ports::new` and `Actions::new` (`:433`); part 1a keeps it unused
  (`#[expect(dead_code)]`, `mod.rs:239`, so part 1b must remove the
  attribute when it reads `docker ps`). Every 5 s, never `live`. Params
  (`accepts`, `mod.rs:48`): `udp` and `lite`, booleans, nothing else.
  `wants` (`:569`) records whether anyone asked for UDP and whether every
  subscriber is lite. Each tick (`measure`, `:373`): the tables; with a
  subscriber that is not lite, the owner map, rebuilt (`rebuild`, `:333`)
  only when the user's listening inodes differ from the last build,
  the `stat` of any mapped pid (every member of every tree) is gone or
  has another start time, or the map is 30 s old (`OWNER_MAX_AGE`, added
  by the audit below) (`stale`, `:304`, which otherwise refreshes the
  trees' summed `statm`); then the interface table (`table`, `:283`) and
  the fold (`fold`, `:423`). A `get` that is not lite rebuilds
  (`snapshot`, `:580`); `idle` (`:595`) drops the process cache, the map
  and the table; `status` (`:602`) adds `ports: {owners, rebuilds}`,
  `owners` counting the mapped pids. The view (`:502`) takes the UDP rows
  out for a subscriber that did not ask, counts per view, and shapes the
  lite view; `total` counts every row of the view, lite or full.
- **The tables** (`samplers/ports/procnet.rs`): `parse` (`:83`) keeps
  state `0A` (TCP) or `07` (UDP) and skips a line it cannot read;
  `address` (`:62`) reads each 32-bit word with `to_ne_bytes`, as the
  kernel prints it, and gives a v4-mapped IPv6 address as the IPv4 one;
  `read` (`:106`) reads `udp` and `udp6` only when asked.
- **Scopes** (`samplers/ports/ifaces.rs`): loopback, unspecified and
  link-local by the address (`intrinsic`, `:59`), anything else by the
  interface that holds it (`Table::read`, `:131`: `sys::ifaddrs` on the
  host, `run/mirqab-test/ifaddrs.json` under a fixture root; an alias
  label such as `lan0:1` is stripped to its interface, `:144`) and that
  interface's kind (`kind`, `:95`, from `sys/class/net/<name>/`). There
  is no arm for the loopback interface: an address on `lo` that is not
  127/8 or `::1` is `other`, exposed (Linux's weak host model lets others
  reach it). The table is read only while some bind address needs it,
  again when that set changes, and at most once a minute otherwise.
  Interface names stay in the daemon.
- **`sys::ifaddrs`** (`crates/mirqab-core/src/sys.rs:245`): `getifaddrs`,
  IPv4 and IPv6 only, the list freed by a guard (`IfAddrs`, `:232`) on
  every path; three `unsafe` blocks, each with its `// SAFETY:`. Unit
  test `ifaddrs_lists_the_loopback` (`:319`).
- **The owner map** (`samplers/ports/owner.rs`): `socket_inodes` (`:12`)
  reads a process's `fd/` links; `owners_of` (`:42`) reads every one of
  the user's processes, in pid order. `trees` (`mod.rs:160`) groups one
  socket's holders by their parent links: a holder whose parent does not
  hold the socket heads a tree, and the others join the tree their chain
  of holding parents leads to. Each tree is a row: `pid`, `start`,
  `process`, `label` and `uptimeSec` are the root's, `pids` lists the
  root and then the rest by pid, and `rssBytes` sums the members
  (`sum`, `:148`). Independent holders (`SO_REUSEPORT` siblings, a second
  server whose parent holds nothing) are rows of their own, in pid order.
  Root's and other users' rows have `pids: []`. The walk is
  `procfs::walk` with the sampler's own `procfs::Cache`, so the `apps`
  lock is never taken.
- **Rows**: one per port, protocol, bind pair and owner; an owner's
  `0.0.0.0` and `::` sockets, or `127.0.0.1` and `::1`, are one bind pair
  and fold into one row named by the IPv4 address (`bind_key`,
  `mod.rs:204`); one process on two other addresses of a port is two
  rows. A lone `::` is `[4, 6]` unless `proc/sys/net/ipv6/bindv6only` is
  1 (`families_of`, `:224`). `label` (`:74`): an interpreter's script or
  `-m` module stands for the program, joined by the next argument when it
  is a word (`next dev`, `http.server`, `manage.py runserver`, `vite`),
  cut to 40 characters and made `plain`. The script is found by the apps
  grouping's `group::script_and_rest`, so an option's value
  (`-W ignore`, `-r dotenv/config`) is skipped and inline code
  (`perl -E`, `python3 -c`) leaves the interpreter's name alone (audit
  2026-11-21, L18). `uptimeSec` is
  `procfs::age_secs` (`samplers/apps/procfs.rs:259`), which the
  inspector now uses for `ageSec` too (`inspect.rs:466`). `url`
  (`mod.rs:121`) is `http://localhost:<port>`, `https` for 443 and 8443.
- **The CLI** (`crates/mirqab/src/ports.rs`, dispatched at `main.rs:114`):
  `mirqab ports [--udp] [--json]` (`ports_lines`, `:55`: columns PORT,
  PROTO, BIND, REACH, PROCESS, PROJECT, LAUNCHER, UP, then one `exposed:`
  line per exposed row of yours) and `mirqab port PORT [--proto tcp|udp]
  [--json]` (`port_lines`, `:86`: Port, Reach in words, Process, Label,
  Up, URL; part 1b's fields appear once the daemon fills them; exit 1
  "nothing listens on port N/tcp"). Every value printed is made `plain`,
  a container's name too (`process_cell`, `:24`). The strings are in
  `strings.rs`'s "Ports (plan 007)" section (`:2014`) and two
  `CLI_COMMANDS` entries (`:106`, `:114`); `docs/cli.md` is regenerated.
- **Fixtures**: ten processes in `tests/fixtures/procs.tsv` (four
  terminal sessions, an agent, its dev server, an agent that listens,
  the MCP server as a future caller), socket `fd=` links added to 1300
  and 3020, and `tools/mkfixture.sh`'s new `env=NAME,NAME` extra, which
  writes `environ` as `NAME=MARKERVALUE` (`tools/mkfixture.sh:89`,
  `:163`); the hand-written tables, `bindv6only`, the interface table
  (with a veth and an alias label that no committed socket binds),
  `sys/class/net/` and three `package.json` files in both trees
  (`tests/fixtures/README.md`). The `apps` golden and the group counts
  went from 24 to 28 groups (four `session:` groups) and 44 to 54 (t0) or
  53 (t1) processes: `crates/mirqab-core/tests/apps.rs`,
  `crates/mirqab-view/tests/apps.rs`, `crates/mirqab/tests/mcp.rs`,
  `tests/test-ui-apps.sh`. The new processes use no CPU between the two
  trees, so no rate assertion moved.
- **Docs**: architecture §5.2 (the params and `status.ports`) and §5.3
  (the `ports` paragraph: rows, trees and `pids`, scopes, the
  `IPV6_V6ONLY` limit, "exposed" as a claim about the bind address
  only); `README.md`.

Tests (2026-11-20): `crates/mirqab-core/tests/ports.rs` 13
(`tcp_rows_parse_both_families_and_only_listeners` `:181`,
`udp_rows_are_bound_sockets_and_only_when_asked` `:229`,
`a_dual_stack_listener_is_one_row_marked_both` `:260`,
`each_bind_address_gets_its_scope` `:293`,
`inodes_map_to_the_processes_that_hold_them_and_root_has_no_pid` `:361`,
`the_owner_map_is_rebuilt_only_when_the_inode_set_changes` `:398`,
`the_lite_view_reads_no_fd_and_gives_counts` `:451`,
`uptime_rss_and_label_come_from_the_owner` `:482`, and from the review
`a_forked_servers_workers_join_the_parents_row` `:528`,
`independent_holders_of_one_port_are_rows_in_pid_order` `:553`,
`one_process_on_two_binds_of_one_port_is_two_rows` `:566`,
`the_interface_table_is_read_again_when_the_bind_addresses_change`
`:586`; `the_stream_matches_its_golden` `:606`); `tests/protocol.rs`
27 → 28 (`ports_params_are_strict`; the hello test names `ports`);
`mirqab-core` unit tests 70 → 76 (the params, the label, the URL, the
address words, the intrinsic scopes, `ifaddrs`); `mirqab` unit tests
20 → 23 (`a_containers_name_cannot_reach_the_terminal` `ports.rs:247`
from the review); `tests/test-cli.sh` 231 checks (new: `ports --json` 11
TCP rows, `--udp` 13, the table's port, process, reach and root row, the
exposed line for 8000 and none for root's 22, `port 3000`'s and
`port 8000`'s Reach, root's port with no process, `--proto udp`, exit 1
for port 1, 64 for `abc`, 70000 and `--proto sctp`, `ports` in `status`
with no subscriber before and after, `ports` and `port` in `--help`,
`help` and the completions). Each new test was checked by breaking the
code it covers (the PR lists the mutations).

Cost (2026-11-20, release build, the development machine,
`tools/bench-idle.sh --subscribe ports`, before the review's every-process
fd scan): 0.000 % of one core over 60 s, 7.8 MiB, 233 context switches;
with `--procs 400` (432 processes, 8 clones of the 47 user processes)
0.033 %, 8.1 MiB, 268 context switches. Both under the brief's 1 % (and
the 0.3 % idle row). The scan now reads every process of the user's on a
rebuild, which happens only when the inodes or a mapped pid change (and,
since the audit below, every 30 s).
After the review (same build settings, same day): 0.000 %, 7.6 MiB, 211
context switches; with `--procs 400` 0.067 %, 8.5 MiB, 230 context
switches. The CI line and the §9.1 row come
with part 4, with the tray's subscription.

Deviations from the brief, each with its reason:

- **A scope `other`**: the brief's interface kinds include `other` (a
  veth, a dummy, a bridge's port), which its scope list and its
  sentences left out. It is a scope of its own, exposed, with its own
  sentence and REACH word (kept by the review, ruling 13).
- **`mirqab port` reads TCP unless `--proto udp`**, as `kill` will.
- **No `.git/HEAD` in the fixture projects**: git refuses to track a
  path with a `.git` component (deferred to part 1b, ruling 12).
- **The project trees' `package.json` files are added now** (the brief
  allowed it), so the trees change once.

Known limits: a socket that set `IPV6_V6ONLY` itself shows as both
families (the table does not say; overstating reach is the safe
direction); when one address sits on two interfaces (a misconfiguration),
`Table::read` keeps the last interface it sees, as exposure is not
totally ordered across kinds.

### Review of PR #22 (2026-11-20)

A Sonnet review asked for changes; the planner (Fable 5.1) ruled, and
the brief's §9 records the amendments for later parts. Built in this PR:

1. **One row per holder tree** (KEEP). The first build stopped the fd
   scan once every wanted inode was found, so a forked server's later
   workers were missed and a stop could not name them. Now `owners_of`
   reads every process, the holders of a socket are grouped by parent
   links into trees, each tree is a row with `pids` (root first),
   `stale` checks every pid of a tree, `rssBytes` sums them and
   `status.owners` counts pids. Independent holders stay separate rows.
2. **No loopback arm in `ifaces::kind`** (KEEP): an address on `lo` that
   `intrinsic` did not place is `other`, exposed. The tie-break for an
   address on two interfaces was dropped and is a known limit.
3. **Alias labels are stripped** (`lan0:1` → `lan0`) before the sysfs
   lookup (KEEP), with `veth0` and `lan0:1` fixtures.
4. **`total` counts every row** in the lite view as in the full one
   (KEEP); there is no `yourCount`, and part 4's badge shows `total`,
   coloured by `exposedCount`. (The first build counted your rows there.)
   *Corrected by the audit of 2026-11-21, L32:* the rows are not the full
   view's while every subscriber is lite. The sampler then skips the
   owner walk (`measure`, `samplers/ports/mod.rs`), so a row folds by
   (port, proto, uid, bind) instead of by process: `total` and
   `exposedCount` can be lower than the full view's (two processes of
   one user on one port are one row), and `leftBehindCount` is 0, since
   no row has a launcher. With a full subscriber as well, the lite counts
   are the full view's. The `PortsLite` doc comment says so.
5. **A row is `(port, proto, bind pair, owner)`** (KEEP the words and
   the test). DEFER: part 3's `RowKey` gains `bind`.
6. **The interface table's refresh is tested** (KEEP).
7. DEFER to part 4's bench: see Unverified.
8. The test counts above are corrected (`tests/protocol.rs` 27 → 28).
9. DROP: `proc/uptime` is still read once per row.
10. DEFER to part 3: `strings::PORTS_UDP_NOTE` under the UDP switch (UDP
    includes sockets programs opened to talk out, and anything can send
    to them).
11. **The CLI makes a container's name `plain`** (KEEP). DEFER to part
    1b: make it plain at the source in `docker.rs`.
12. DEFER to part 1b: `package.json` stays committed; part 1b's test
    makes `.git/HEAD` in a scratch copy and asserts that a directory
    holding only `.git` is a project.
13. The `other` scope and its sentence stay as written, with no
    firewall clause.

### Audit of the samplers (2026-11-21)

- **The owner map is built again once it is 30 s old** (`OWNER_MAX_AGE`
  in `samplers/ports/mod.rs`, checked in `stale`). A worker forked after
  a build (a respawn after a worker died, a prefork server scaling up)
  holds a mapped socket without changing the inode set or any mapped
  pid, so no tick added it and the row's `pids` and `rssBytes` stayed
  short until a `get` (ruling 1 wants every worker named for a stop).
  Test: `a_tick_maps_a_worker_that_joined_a_mapped_socket_within_30_s`.
  Cost: one walk of the user's processes every sixth 5 s tick, only
  while a subscriber is not lite; part 4's bench measures it with ruling
  7. Rejected: reading `proc/<pid>/task/<pid>/children` of each member
  every tick. It needs `CONFIG_PROC_CHILDREN`, misses children forked by
  another thread and holders that are not children (a socket passed
  over `SCM_RIGHTS`), and a child that holds no socket (a dev server's
  esbuild) would force a rebuild on every tick.
- For part 2a: `ports.stop` and `ports.kill` resolve the port's
  processes from a forced rebuild (the `get` path), never from the
  cached map, so a stop never works from a `pids` list up to 30 s old.
