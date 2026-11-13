# Plan 007: Ports tab

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MEDIUM. Stop kills processes; "exposed" is a security claim
  and must not overstate or understate.
- **Depends on**: 001, 005, 006
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

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
6. CLI `ports`, `port`, `kill`; MCP tools; bar item.
7. `crates/mirqab-view/src/tabs/ports.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/ports/{mod,page,port_row,port_detail,qr_sheet,system_section,list_card}.rs`;
   `crates/mirqab-ui/src/widgets/qr.rs`, `crates/mirqab-view/src/qr.rs`.
8. Sidebar badge and tray attention.
9. Append `ports.list` to the Home seed list.

## Unverified

- `/proc/net/tcp6` address byte order (four host-order 32-bit words):
  confirm with a listener on `::1` and record the row as a fixture.
- Whether `environ` of a re-parented process stays readable (it should:
  same uid): confirm with a process started by an agent that then exits.
- `docker ps` JSON field names on the installed Docker version: record a
  scrubbed sample.
- PolicyKit shows `pkexec /usr/bin/ss` with a sensible prompt without a
  custom policy file; if the prompt is poor, ship
  `dist/polkit/org.mirqab.ports.policy` with a description.
