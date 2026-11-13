# Plan 001: foundation

## Status

- **Priority**: P1
- **Effort**: XL, in five phases (A to E). Each phase is one pull request.
- **Risk**: MEDIUM. Every later plan builds on the protocol, the `Root`
  abstraction and the gates decided here; a mistake is cheap to fix now and
  expensive after ten tabs.
- **Depends on**: —
- **Category**: core
- **Planned at**: 2026-11-13, from the owner
- **State**: IN PROGRESS. Phase A (skeleton and gates) implemented
  2026-11-13: `tools/lint.sh`, `tools/check-docs.sh`, `tools/secrets.sh`
  pass on the branch, and `tests/run.sh` 93/93. Phase B (daemon and
  protocol) implemented 2026-11-13: `tests/run.sh --affected` 140/140.
  Phase C (history) implemented 2026-11-13: 152/152. Phases D and E open.

## Problem

The repository is empty (`git status`: "No commits yet", 2026-11-13). The
owner asked for a standalone Linux counterpart of Stat Space. Before any tab
can exist there must be: a daemon that serves streams over a socket and
keeps 30 days of history; an interface shell with the sidebar, the footer
and theming; and gates that hold every later change to the rules in
`docs/architecture.md`.

Decisions this plan builds to (owner, 2026-11-13): the name is `mirqab`;
the repository is public at `github.com/t1nk333r/mirqab` with a protected
`main`; the interface is Rust with GTK4 and libadwaita; a live graph on
screen samples at 1 s; the licence is MIT; the first release is English
only.

When this plan is done, `mirqab open` shows a window with every tab's tile,
placeholder pages, a working footer with the app's own cost, and Settings.
No tab has content yet; plan 002 adds the first.

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Own CPU time | `/proc/self/stat` fields 14, 15 (utime, stime), `sysconf(_SC_CLK_TCK)` | no |
| Own memory | `/proc/self/status` `VmRSS` | no |
| Interface's cost | `/proc/<ui pid>/stat`, `/proc/<ui pid>/status`; the pid comes from the `ui` hello | no |
| CPU jiffies (first history series) | `/proc/stat`, line `cpu ` | no |
| Memory (first history series) | `/proc/meminfo` `MemTotal`, `MemAvailable`, `SwapTotal`, `SwapFree` | no |
| Machine line in the footer | `/proc/cpuinfo` `model name`, `/proc/uptime` | no |
| Peer identity on the socket | `getsockopt(SO_PEERCRED)` | no |
| Session environment for the interface unit | `systemctl --user show-environment` has `WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE` **[machine]** | no |
| Toolkit libraries | `gtk4 1:4.24.1-1`, `libadwaita 1:1.10.0-1`, `gtk4-layer-shell 1.3.0-1`, `rust 1.99.0` installed **[machine]**; crates `gtk4` 0.11.5, `libadwaita` 0.9.2, `gtk4-layer-shell` 0.8.1 **[crates.io]** | no |

## Not possible on Linux

Nothing in this plan. Differences from Stat Space that start here:

- Live graphs sample at 1 s, as Stat Space does, but only while the graph
  is on screen; everything else runs at 2 s or slower (owner, 2026-11-13;
  architecture §5.3).
- Gauges in history are point samples unless a live sampler ran
  (architecture §6.3).
- The interface is a separate process that exists only while the window
  is open; Stat Space's popover lives in one 19 MB process (its own
  footer, in the owner's screenshots). The interface's memory is not
  known yet: step 31 measures it. The footer shows daemon and interface
  separately.

## Design

Everything is specified in `docs/architecture.md` §3 to §9 and §12 to §16.
This plan adds only what that file leaves open.

### Crates

- `mirqab-proto` (library; depends on `serde` and `serde_json` only).
  Modules: `frame` (request, response, event, hello), `code` (the closed
  list of error codes), `stream` (one data struct per stream, added by
  each tab's plan), `strings` (every user-visible string; architecture
  §7.2). Both the daemon and the interface depend on it, which is why it
  is not a module of `mirqab-core` (architecture §12).
- `mirqab-core` (library). Re-exports `mirqab_proto` as `proto`. Modules: `root`
  (`Root::new(path)`, `root.read("proc/stat")`, `root.read_dir(…)`,
  `root.readlink(…)`; the only place a `/` path is built), `exec` (run a
  fixed argv with a timeout), `act` (the mutation helpers, each taking
  `dry_run`), `server` (accept loop, subscriptions, merged intervals),
  `sampler` (the trait below), `history` (schema, writer, query, prune),
  `config`, `selfcost`, `samplers::system_core`.
- `mirqab` (binary). Modules: `main` (argument dispatch), `daemon`,
  `cli` (a client: connect, one request, print), `elevate` (empty until
  plan 015; exists so the lint rule has its one allowed file).
- `mirqab-fand`: a stub `main` that prints its version. Plan 015 fills it.
- `mirqab-view` (library; depends on `mirqab-proto` only, no GTK).
  Modules here: `format` (bytes, rates, percent, durations, temperatures
  by unit), `theme` (the token table and the generated stylesheet text),
  `icons` (glyph names), `graph` (points to path geometry: scaling, gaps,
  the fill outline), `cards` (the registry plan 003 fills), `shell` (the
  sidebar's tab list from `capabilities` and `tabs.hidden`, the footer
  text), `tabs` (one module per tab, added by that tab's plan).
- `mirqab-ui` (binary `mirqab-ui`; `gtk4` 0.11.5 with feature `v4_16`,
  `libadwaita` 0.9.2 with feature `v1_6`, `gtk4-layer-shell` 0.8.1).
  Modules listed under "Interface shell" below.

The daemon must not link the toolkit. Lint checks that
`cargo tree -p mirqab -e normal` names none of `gtk4`, `glib`, `gio`,
`libadwaita`, `cairo-rs`, and the same for `mirqab-core`, `mirqab-proto`,
`mirqab-view` and `mirqab-fand`.

```rust
pub trait Sampler: Send {
    fn stream(&self) -> &'static str;
    /// None when this machine cannot serve the stream; the reason goes to `status`.
    fn probe(&mut self, root: &Root) -> Result<(), String>;
    fn sample(&mut self, root: &Root, now: Instant) -> serde_json::Value;
    /// Called once a minute, subscribers or not. Returns (series key, scaled value) pairs.
    fn minute(&mut self, root: &Root) -> Vec<(&'static str, i64)> { Vec::new() }
}
```

Threads: one accept thread, one reader thread per client, one thread per
*active* sampler (parked while it has no subscriber), one history thread.
All output to a client goes through a per-client channel and a writer, so a
slow client blocks nobody. A client whose queue passes 256 frames is
dropped.

### Streams added here

`self`, every 2 s while subscribed (the footer is text, not a live graph;
the stream accepts `live` for a pinned self-cost graph):

```json
{"daemon":{"cpu":0.1,"rssBytes":3145728,"threads":4},
 "ui":{"cpu":0.4,"rssBytes":0,"pid":4321},
 "subscribers":{"system":2},"uptimeSec":3600}
```

`ui`, events only: `{"event":"show","tab":"system","mode":"popover"}`,
`{"event":"hide"}`, `{"event":"toggle","tab":null}`,
`{"event":"config-changed"}`. A client that subscribes to `ui` with
`params.role = "ui"` and its pid is the interface; the daemon remembers one.

`system` (core only; plan 002 defines the full shape):
`{"cpu":{"percent":…},"mem":{"totalBytes":…,"availableBytes":…}}`.

### Series added here

`cpu.total` (percent ×100), `mem.used` (bytes), `mem.available` (bytes),
`swap.used` (bytes). Plan 002 adds the rest of §6.1.

### Config keys added here

```json
{"window":{"mode":"popover","anchor":"top-right","marginEm":0.5,"widthEm":58,"heightEm":40},
 "live":{"intervalMs":1000},
 "theme":{"follow":null,"accent":null,"scheme":"dark"},
 "units":{"temperature":"c","clock":"auto"},
 "home":{"pinned":[]},
 "tabs":{"hidden":[]}}
```

`live.intervalMs` is the interval of a live graph on screen: 1000 by
default, settable from 1000 to 5000 for a user who wants less work. It
does not change any other interval.

### The `live` flag (architecture §5.3)

- `subscribe` takes `"live": true`. With it the floor is
  `live.intervalMs` (1000 by default); without it the floor is 2000 and a
  shorter request is raised.
- Only streams whose sampler declares `LIVE = true` honour the flag; the
  others ignore it and apply the 2000 floor. In this plan: `system`,
  `self`.
- The server merges subscribers to the shortest interval as before. When
  the last `live` subscriber of a stream leaves, the sampler's interval
  goes back to the shortest remaining one in the same tick.
- In the interface the flag is set in exactly one place: the `Graph`
  widget, when it is mapped, its range is "2m" and its window is visible.
  It releases the live subscription on unmap, on a range change and on
  window hide. A page never passes `live` itself; lint greps
  `crates/mirqab-ui/src/tabs` for `live` in a subscribe call.

### Interface shell

`crates/mirqab-ui/src/`:

- `main.rs`: parses `--socket`, `--window`, `--tab`, `--selftest <tab>`;
  builds the `adw::Application` with id `io.github.t1nk333r.mirqab`. The
  application is single-instance: a second start activates the first.
- `app.rs`: start-up (stylesheet, `adw::StyleManager` colour scheme,
  the client), the `ui` event handler (show, hide, toggle,
  config-changed), and the exit timer: 60 s after the window hides, with
  nothing pinned open, the process quits (`// mirqab:ui-timeout`).
- `client.rs`: the socket client on GLib's main loop:
  `gio::SocketClient` to the Unix socket, a `gio::DataInputStream` read
  line by line with `read_line_future`, writes queued on the output
  stream. `subscribe(stream, interval, live, callback)` returns a handle
  that unsubscribes when dropped; subscriptions are reference-counted per
  (stream, live); reconnects with a one-shot back-off (2 s, 4 s, 8 s,
  capped at 30 s, each `// mirqab:ui-timeout`); exposes `connected`,
  `capabilities`, `unavailable`. Socket path from `MIRQAB_SOCKET` or the
  default. No thread and no async runtime: `glib::MainContext::spawn_local`.
- `window.rs`: one `adw::ApplicationWindow` holding a horizontal box of
  `Sidebar` and a column of header, page slot and `Footer`. In popover
  mode it is made a layer surface before it is first presented
  (`gtk4_layer_shell`: `init_layer_shell`, layer `Top`, anchors and
  margins from `window.anchor` and `window.marginEm`, keyboard mode
  `OnDemand`, namespace `mirqab`); in window mode it is an ordinary
  toplevel. Switching mode destroys the window and builds the other kind,
  because a surface cannot change role. When
  `gtk4_layer_shell::is_supported()` is false the popover request falls
  back to window mode and says so once in the footer.
- `pages.rs`: the page slot. It holds a table of tab id to constructor
  and builds a page when its tab is first shown; all pages are dropped
  when the window hides.
- `style.rs`: loads `resources/style.css` (embedded with
  `include_str!`), generates the token stylesheet from
  `mirqab_view::theme` plus `theme.json` plus the followed file, loads it
  into a second `gtk::CssProvider` at
  `STYLE_PROVIDER_PRIORITY_APPLICATION + 1`, and watches both files with
  `gio::FileMonitor`.
- `selftest.rs`: `--selftest <tab>` (architecture §7.5): window mode,
  build the page, wait for its first event or 5 s, print the widget tree
  as JSON on stdout, exit 0; exit 1 with the reason on stderr otherwise.
- `widgets/`: `card.rs`, `graph.rs`, `range_picker.rs`, `stat.rs` (label
  and value), `pill.rs` (a coloured status word), `meter.rs` (a
  horizontal bar), `badge.rs`, `confirm_dialog.rs` (an `adw::AlertDialog`
  showing a `wouldDo` list), `empty_state.rs` (an `adw::StatusPage`),
  `icon.rs` (the glyph label), `sidebar.rs`, `footer.rs`. Switches and
  buttons are GTK's own (`gtk::Switch`, `gtk::Button` with the `flat`
  class); no wrapper.
- `tabs/settings/page.rs`: window mode and anchor, units, colour scheme,
  theme follow path, live interval, hidden tabs, and a "Daemon" section
  (version, socket, history size, helper status). Built from
  `adw::PreferencesGroup` rows.
- `tabs/<tab>/page.rs` for each of the sixteen tabs, containing one
  `EmptyState` naming the plan that will fill it. Tabs whose stream is not
  in `capabilities` are hidden once that tab's plan lands; until then all
  show.

`Graph` (`widgets/graph.rs`) is a `gtk::Widget` subclass. Properties:
`points` (avg), `peaks` (max, optional), `min`, `max`, a series token.
`snapshot()` asks `mirqab_view::graph` for the geometry at the widget's
size and appends one stroked `gsk::Path` and, when asked, one filled
path; a `None` point breaks the line. It owns the live-subscription rule
above. It is written new: nothing is adapted from haseen's QML graph.

Keyboard: Escape closes, Ctrl+1…9 and Ctrl+Tab move between tabs, `/`
focuses a page's search field when it has one (one
`gtk::ShortcutController` on the window). Every control is reachable by
Tab and has an accessible label (architecture §7.2).

The popover closes on Escape, and when the window's `is-active` property
turns false unless Pinned is on.

### The repository, CI and branch protection

The order is fixed (architecture §16.1):

1. **Coordinator, before this plan starts.** Creates the public
   repository `t1nk333r/mirqab`. Runs `gitleaks detect --no-git` and the
   privacy patterns of step 9 by hand over the design files, because the
   gate does not exist yet. Pushes the design (`docs/`, `plans/`,
   `CLAUDE.md`, `AGENTS.md`, `README.md`) as the first commit on `main`.
2. **Phase A is the first pull request.** It adds the gates and exactly
   one workflow file, `.github/workflows/ci.yml`, whose four jobs are
   named `lint`, `test`, `docs` and `secrets`.
3. **Coordinator, once those four checks have reported on that pull
   request.** Turns on branch protection for `main`: required status
   checks `lint`, `test`, `docs`, `secrets`, with "require branches to be
   up to date"; require a pull request; linear history; no force pushes;
   no deletion; administrators included. Repository settings: squash
   merge only, auto-merge allowed, delete the branch on merge. GitHub
   cannot require a check that has never reported, which is why this
   comes after step 2 and not before.
4. Phase A's pull request is then the first to merge through protection
   (`gh pr merge --squash --auto --delete-branch`). Every later change
   does the same.

This plan adds no other workflow file. Plan 020 adds
`.github/workflows/release.yml`.

## Rejected

- **Async Rust (tokio).** A runtime and its threads for fewer than ten
  clients. Blocking I/O on std threads measured 2.7 MiB RSS in haseen's
  sidecar (haseen plan 167).
- **Protobuf, MessagePack, D-Bus as the app's own IPC.** JSON lines can be
  read by `jq`, a shell, a bar and an agent with no library; the daemon
  sends a few hundred bytes a second at most.
- **Quickshell/QML for the interface.** The owner chose GTK4
  (2026-11-13; architecture §2.3).
- **`.ui` files, Blueprint, Relm4.** Widgets are built in Rust
  (architecture §2.3).
- **A thread for the interface's socket.** `gio` reads it on the main
  loop; a thread would need a channel back to the widgets.
- **`GtkDrawingArea` for the graph.** Cairo for the whole widget
  (architecture §7.4).
- **Enabling branch protection before CI exists.** A required check that
  never reports blocks every merge, including the one that would add it.
- **The interface starting the daemon as a child.** The history, tray and
  CLI must outlive the window.
- **TOML or YAML config.** One more crate; `serde_json` is already
  linked.
- **One crate.** The root helper must not link the daemon's dependencies,
  and the daemon must not link GTK.
- **A feature flag on `mirqab-core` instead of `mirqab-proto`.** Cargo
  unifies features across the workspace build, so the interface would
  still link SQLite.
- **`clap`.** The CLI has about twenty commands with one or two flags each;
  a hand-written parser keeps the binary small, as in haseen's sidecar.

## Tests and fixtures

Fixtures (all synthetic, written by hand or by `tools/mkfixture.sh` from a
template, never copied wholesale from a live machine):

- `tests/fixtures/root-desktop/`: a desktop: `proc/stat` (16 CPUs),
  `proc/meminfo`, `proc/cpuinfo`, `proc/uptime`, `proc/self/stat`,
  `proc/self/status`. Later plans add to this tree.
- `tests/fixtures/root-laptop/`: the same with 8 CPUs; later plans add a
  battery.
- `tests/fixtures/root-minimal/`: only `proc/stat` and `proc/meminfo`, to
  prove every sampler survives missing files.
- `tests/fixtures/root-desktop.t1/`: the same files a moment later, so
  counter deltas have a known answer.
- `tests/stubs/`: `systemctl`, `curl`, `notify-send` scripts that record
  their argv to `$STUB_LOG` and print canned output.

Tests:

| File | Covers | Checks |
|------|--------|--------|
| `crates/mirqab-proto/src/*.rs` (unit) | proto | Every frame round-trips; unknown fields ignored; error codes closed |
| `crates/mirqab-core/tests/protocol.rs` | server | hello first; subscribe, event, unsubscribe; two subscribers merge to the shorter interval; interval below 2000 is raised; with `live` 1000 is accepted on a stream that declares it and raised to 2000 on one that does not; when the live subscriber leaves the interval returns to the other's; unknown stream; a peer flood is dropped |
| `crates/mirqab-core/tests/history.rs` | history | Schema created; counters difference correctly between `root-desktop` and `.t1`; gap stores nothing; each range returns the bucket count of architecture §6.4; prune removes rows older than 30 d; 30 days × 64 series stays under 40 MB |
| `crates/mirqab-core/tests/root.rs` | root | A path with `..` or an absolute path cannot escape the root |
| `tests/test-rust.sh` | `crates/mirqab-{proto,core,view,fand}/**`, `crates/mirqab/**` | Runs `cargo test --locked -p <crate>` for the crates a change touched and the crates that depend on them. No display |
| `tests/test-cli.sh` | `crates/mirqab/src/{main,cli}.rs` | Exit code 2 with no daemon; `status --json` parses; `open system` produces a `ui` event; `--help` on every command |
| `tests/test-units.sh` | `dist/**` | `systemd-analyze verify --user` on the user units; the helper unit has no `[Install]` enabling it by preset; `.desktop` validates |
| `crates/mirqab-view/tests/{format,theme,graph,shell}.rs` | `crates/mirqab-view/**` | Formatting against golden input; every token has a default and the generated stylesheet sets each variable once; an override file changes only its keys; graph geometry for a known series, a gap and an empty series; the sidebar list for a `capabilities` set |
| `crates/mirqab-ui/tests/widgets.rs` (`#[gtk::test]`) | `crates/mirqab-ui/src/widgets/**` | `Graph` renders one stroke node for a series and two for a series with a gap, and no blur node; `Graph` takes a live subscription only while mapped with range 2m (a fake client records the calls); `ConfirmDialog` lists every `wouldDo` line |
| `tests/test-ui-widgets.sh` | `crates/mirqab-ui/**`, `crates/mirqab-view/**` | Starts `gtk4-broadwayd` on a private display through `tests/lib.sh` and runs `cargo test --locked -p mirqab-ui` under it |
| `tests/test-ui-shell.sh` | `crates/mirqab-ui/src/{main,app,client,window,pages,style,selftest}.rs`, `crates/mirqab-ui/src/widgets/{sidebar,footer}.rs` | Starts the daemon on `root-desktop`, runs `mirqab-ui --selftest home` under Broadway, and with `jq` checks the sidebar's sixteen tiles, the footer text and that no page but Home was built; then hides the window and checks the process exits (with `MIRQAB_UI_EXIT_MS=500`, honoured only when `MIRQAB_TEST=1`) |
| `tests/test-lint-rules.sh` | `tools/lint.sh` | Each grep rule fails on a planted violation in a scratch copy |
| `tests/test-affected.sh` | `tools/affected-tests.sh`, `tests/run.sh` | A changed path selects exactly the files that cover it; an uncovered source file fails lint |

`tests/lib.sh` gains `start_broadway` and `stop_broadway`: pick a free
display number, start `gtk4-broadwayd :N` bound to a socket under the
sandbox, export `GDK_BACKEND=broadway`, `BROADWAY_DISPLAY=:N`,
`GSK_RENDERER=cairo`, `GTK_A11Y=none`, and kill it on exit.

## Acceptance

1. `tools/lint.sh`, `tests/run.sh`, `tools/check-docs.sh` and
   `tools/secrets.sh` pass on a clean checkout, and CI runs the four jobs.
2. `systemctl --user start mirqab.socket && mirqab status` prints the
   version, socket and zero subscribers.
3. `mirqab open` shows the popover with sixteen tiles, the footer with a
   live self-cost and the machine line. `mirqab open --window` shows the
   same content in a normal window. Escape closes; the `mirqab-ui` process is
   gone 60 to 70 s later (`pgrep -x mirqab-ui`).
4. After ten minutes, `mirqab history cpu.total --range 1h --json` returns
   about ten points, and they match `/proc/stat` deltas within 1 %.
5. `tools/bench-idle.sh` reports under 0.3 % of one core and under
   15 MiB RSS for the daemon with the window closed and no subscribers,
   measured as architecture §9.1 describes, and
   `tools/bench-idle.sh --subscribe system --live` reports under 1 %.
   Both numbers are written into this plan's State line with the date.
6. `grep -rn 'sudo\|pkexec' crates tools dist | grep -v elevate.rs`
   prints nothing.
7. Changing `theme.json` recolours the open window without a restart; no
   colour literal exists in `crates/mirqab-ui` outside the token block of
   `resources/style.css`.
8. With the System placeholder replaced by a test graph on the `system`
   stream, `mirqab status` shows the stream at 1000 ms while the graph is
   on screen at range 2m, at 2000 ms after the range is changed to 1h,
   and no subscriber after the window closes.
9. `ldd target/release/mirqab` names no `libgtk-4`, `libadwaita-1` or
   `libglib-2.0`.
10. Branch protection is on, and Phase A's pull request merged through
    it.

## Checklist

### Phase A — skeleton and gates (PR 1)

1. `LICENSE` (MIT, "Copyright (c) 2026 t1nk33r"), `NOTICE.md` (table:
   file, upstream, licence, copyright; rows are added as code is adapted).
2. `Cargo.toml` workspace with the six crates (`resolver = "3"`,
   versions of shared dependencies in `[workspace.dependencies]`); `[profile.release]` as in
   architecture §14; `rust-toolchain.toml` pinning the stable channel;
   `.gitignore` (`target/`); `deny.toml` (licences allowed: MIT,
   Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib,
   Unlicense; `bans`: `multiple-versions = "warn"`, and deny a second
   version of `zbus`; `sources`: crates.io only).
3. Each crate: `Cargo.toml` and a `lib.rs` or `main.rs` that compiles and
   has one trivial test.
4. `tests/lib.sh`: sandbox (`HOME`, `XDG_*`, `TMPDIR` under a per-run
   scratch directory outside the checkout, removed on exit), stub `PATH`,
   `ok`/`not_ok`/`is`/`like` helpers, `start_daemon ROOT` and
   `stop_daemon`. Model: haseen `tests/lib.sh`.
5. `tests/run.sh`: no arguments runs all `tests/test-*.sh`; file arguments
   run those; `--affected [--committed] [BASE]` asks
   `tools/affected-tests.sh`. One subshell per file, a `RESULT` line at the
   end. Model: haseen `tests/run.sh` (89 lines).
6. `tools/affected-tests.sh`: read `# mirqab:covers <glob>` lines from each
   test file, match against `git diff --name-only $(git merge-base BASE
   HEAD)` plus staged, unstaged and untracked paths, print the selection
   and, with `--why`, the reason for each. Shared infrastructure globs
   select everything that declares `# mirqab:covers-shared`.
7. `tools/lint.sh`: `cargo fmt --all --check`; `cargo clippy --workspace
   --all-targets --locked -- -D warnings`; `${CARGO_DENY:-cargo deny}
   check licenses bans sources` (plus `advisories` when `CI` is set);
   the `cargo tree` rule under "Crates"; `bash -n` and
   `${SHELLCHECK:-shellcheck} -x --severity=warning` on every shell file;
   `jq empty` on every JSON file; the grep rules of architecture §5.3,
   §5.4, §7.2, §7.3 and §7.4; the coverage rule (a file under `crates/`,
   `dist/`, `tools/` that no `covers` glob matches fails unless listed in
   `tools/coverage-exempt.txt`). There is no QML gate.
8. `tools/check-docs.sh`: every `plans/NNN-*.md` has a row in
   `plans/README.md` and the reverse; every relative link in `*.md`
   resolves; every stream named in `crates/mirqab-proto/src/stream.rs` has
   `tests/golden/<stream>.json`.
9. `tools/secrets.sh`: `${GITLEAKS:-gitleaks} detect` over history; then
   patterns over tracked files: IPv4 addresses outside documentation ranges
   (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24, loopback, 0.0.0.0), MAC
   addresses, `100.64.0.0/10` addresses, coordinates with more than two
   decimals, `sk-…`/`ghp_…` shapes, and a `tools/secrets-deny.txt` of
   literal strings (kept empty in the repository; the owner's machines add
   their hostnames locally through `MIRQAB_SECRETS_DENY`).
10. `.github/workflows/ci.yml`, the only workflow this plan adds. Triggers:
    `pull_request` and `push` to `main`. `permissions: contents: read`.
    Container `archlinux:latest`; install `base-devel rust pkgconf gtk4
    libadwaita gtk4-layer-shell sqlite dbus shellcheck jq git systemd
    cargo-deny gitleaks`; create and switch to an unprivileged user. Four
    jobs named exactly `lint` (`tools/lint.sh`), `test`
    (`tests/run.sh --affected origin/main` on a pull request, `tests/run.sh`
    on `main`), `docs` (`tools/check-docs.sh`), `secrets`
    (`tools/secrets.sh`, checkout with `fetch-depth: 0`). `actions/checkout`
    and the cargo cache action pinned by commit SHA. Model: haseen
    `.github/workflows/ci.yml`.
10a. Stop and hand over to the coordinator for step 3 of "The repository,
    CI and branch protection". Do not merge this pull request before
    protection is on.
11. `tests/test-lint-rules.sh`, `tests/test-affected.sh`.

Phase A as built (2026-11-13), where it differs from the steps above:

- `start_daemon` and `stop_daemon` (step 4) move to phase B, which adds
  the daemon they start; phase A's `tests/lib.sh` has the sandbox, the
  stubs and the assertions only.
- `tests/test-rust.sh` (from the Tests table) lands in phase A, because
  the crates exist from step 3 and their trivial tests need a runner.
- Shared infrastructure is a `SHARED` glob list in
  `tools/affected-tests.sh`; a test opts in with `# mirqab:covers-shared`.
  The exempt list is `tools/coverage-exempt.txt` (step 7), read by the
  selector, so `--check` is the coverage rule `tools/lint.sh` runs.
- `tools/affected-tests.sh --paths` prints the changed paths;
  `tests/test-rust.sh` narrows `cargo test` to the touched crates and
  their dependents with it.
- The Broadway check in CI (Unverified) moves to phase D: phase A has no
  widget to run under it. `gtk4-broadwayd` is at `/usr/bin/gtk4-broadwayd`
  on the development machine.
- All four CI jobs run in the `archlinux:latest` container; `docs` and
  `secrets` install only `git` (and `gawk`, `gitleaks`) and, since they
  only read the checkout, run as the container's root.
- Step 6's `--why` is not a flag: the selector prints its reasons to
  stderr unless `--quiet`. `--paths-for TEST` prints the changed paths that
  select one test (or `all`); `tests/test-rust.sh` uses it, so any path
  outside a crate that selects it (a fixture, a golden file, the manifest,
  the lock, the test itself) runs every crate.
- `tools/` is not shared infrastructure as a whole (CLAUDE.md said it was;
  review of PR #1): a change to `tools/lint.sh` runs the lint-rule test,
  not every cargo test. The test machinery is global and the Rust
  workspace files are shared; CLAUDE.md and architecture §16 now say so.

Review of PR #1 (Sonnet, checked by Fable, 2026-11-13). Fixed in phase A:
the Rust tests skipped when only a fixture changed; host addresses in CIDR
form passing the secrets scan; a last deny-list or exempt line without a
newline being ignored; lint exemptions matching a file name in any crate;
`cargo tree` errors hidden; `TMPDIR` left pointing at `/tmp`; `--check`
stopping silently. Each has a regression test (`tests/test-secrets.sh` is
new). Deferred, each to the step that adds the code it guards:

- Phase D, steps 25–26: the colour rule fires on `#123` in a comment and
  on a CSS id selector (`#cafe {`); narrow it to value positions once
  `theme.rs` and `style.css` exist.
- Phase D, step 27: the `live` rule scans only `tabs/` and one line at a
  time; widen it to all of `crates/mirqab-ui/src` but `widgets/graph.rs`,
  multi-line, once `client.rs` fixes the subscribe call's shape. Widen
  the toolkit list of the link rule (`glib-sys`, `gtk4-sys`, `pango`,
  `gdk-pixbuf`) when GTK enters `Cargo.lock`, and make it see build and
  dev dependencies and renamed (`package =`) ones: `-e normal` sees
  neither today.
- Phase D, step 28: the timer rule accepts `add_tick_callback` with a
  `mirqab:sample` marker (§7.2 allows only `ui-timeout`), flags a `use`
  line and a trailing same-line marker, and does not check the 1000 ms
  minimum.
- Plan 011: dash-separated MAC addresses in the secrets scan.
- Plan 015: `mirqab-fand` needs literal `/sys` paths; the literal-path
  rule needs its exemption then.
- Plan 017: the coordinate rule is case-sensitive and key-based
  (`Lat: 24.71361` passes).

The first version of `tests/test-secrets.sh` wrote its sample addresses,
MAC and coordinate out literally, and that commit (`e72bfef`) reached the
pull request branch before the gate caught it: the push ran because the
gate's status went through a pipe to `tail`. The values are synthetic.
Rewriting the pushed branch was refused, so `tools/secrets.sh` gained
`tools/secrets-allow.txt`: full hashes of reviewed commits whose history
the scan skips, each with its reason. The test now builds its samples at
run time, and the history is read once instead of once per pattern. The
allow list is scanned like any other file; the entry becomes dead once the
squash merge leaves `e72bfef` out of `main`, and is removed then.

### Phase B — daemon and protocol (PR 2)

12. `crates/mirqab-proto/src/{frame,code,stream,strings}.rs`: the frames
    and error codes of architecture §5.2, the `live` flag, with the DMS and haseen notice in the header and a
    NOTICE row.
13. `root.rs`, `exec.rs`, `act.rs` (with `signal`, `remove_path`,
    `write_file`, each a no-op returning a description under `dry_run`).
14. `sampler.rs` (the trait), `server.rs` (accept, peer-credential check,
    hello, subscribe/unsubscribe/get/status/shutdown, merged intervals,
    the `live` floor, per-client queue). Model: haseen PR #128 `core/sidecar/src/server.rs`.
15. `selfcost.rs` and the `self` stream; `config.rs` with
    `config.get`/`config.set` and atomic write (temporary file, `rename`).
16. `crates/mirqab/src/{main,daemon,cli}.rs`: `daemon [--root] [--socket]`,
    socket activation (`LISTEN_FDS`), `sd_notify` ready by writing to
    `NOTIFY_SOCKET` (no library), the `ready <path>` line; `status`,
    `open`, `toggle`, `close`, `--help`, `--version`.
17. `dist/mirqab.socket`, `dist/mirqab.service`, `dist/mirqab-ui.service`
    (`ExecStart=/usr/bin/mirqab-ui`, `Restart=no`,
    `PartOf=graphical-session.target`).
18. `share/mirqab/config.schema.json`; `tests/golden/self.json`,
    `tests/golden/ui.json`.
19. Tests: `proto` units, `tests/protocol.rs`, `tests/root.rs`,
    `tests/test-cli.sh`, `tests/test-units.sh`, the fixtures.

Phase B as built (2026-11-13), where it differs from the steps above:

- Three system calls std does not wrap live in
  `crates/mirqab-core/src/sys.rs`, the one `unsafe` file of the daemon
  besides `root.rs`: the peer's uid (`SO_PEERCRED`), `getuid`, and
  adopting systemd's socket (`LISTEN_FDS`). Each block has a `SAFETY`
  comment. CLAUDE.md's `unsafe` list gains `sys.rs`.
- `act::signal` runs `kill -s SIG -- PID` through `exec.rs` instead of
  calling kill(2), so it needs no `unsafe`; a signal is a rare,
  user-started action. It refuses pid 0 and 1, the daemon itself and a
  process of another uid now; the cgroup and compositor refusals of
  architecture §5.4 come with the actions that need them (plans 006–008).
- `selfcost.rs` reads `CLK_TCK` from `proc/self/auxv` (`AT_CLKTCK`)
  through `Root`, falling back to 100, instead of calling `sysconf`.
- `open`, `toggle` and `close` are the actions `ui.show`, `ui.toggle`
  and `ui.hide`, so they take `--dry-run` like every other action. Without
  an attached interface they reply `unavailable` until phase D starts one.
- CLI exit codes: 0 done, 1 refused or failed, 2 no daemon, 64 usage.
- "`open system` produces a `ui` event" is tested in Rust
  (`crates/mirqab/tests/cli.rs`), which can hold a socket as the
  interface; a shell has no Unix-socket client without adding one.
  `tests/test-cli.sh` covers exit codes, help and `status --json`.
- `tests/test-units.sh` runs `systemd-analyze verify --user
  --recursive-errors=no` with a short runtime directory (a long one
  overflows the socket path). The helper unit and the `.desktop` file
  arrive with plan 015 and phase D, and their checks with them.
- The config's keys are checked by hand in `config.rs`; a test checks
  that `share/mirqab/config.schema.json` names the same keys and defaults.

Review of phase B (Sonnet, checked by Fable, 2026-11-13). Fixed before
merge, each with a test:

- `bind()` removed whatever sat at the socket path, recursively. Now it
  replaces only a socket that refuses connections; a file, a directory or
  a symlink makes the daemon refuse to start. `shutdown` uses the same
  socket-only removal.
- `config.set` wrote from what the daemon read at start, so a file broken
  or edited by hand while it ran was overwritten. `set` now re-reads the
  file first and refuses a broken one; other readers re-read it when its
  modification time or length changed.
- A `dryRun` that is not a boolean (`"true"`, `1`) ran the real action;
  now it is `bad-request`.
- A client that half-closed after a request lost the reply: the socket was
  shut while the writer was still flushing. The writer is joined first,
  with a 10 s write timeout so a client that never reads cannot hang it.
- An `accept` error (EMFILE, ENFILE, ENOBUFS, ENOMEM, ECONNABORTED) ended
  the daemon; now it logs and retries after 100 ms.
- Tests put sockets under `/tmp` so a long `TMPDIR` cannot pass the
  108-byte socket path limit.

Deferred, each to where the code it guards is written:

- Phase C: two daemons starting together can both run (the stale-socket
  check races); a lock file, once both would write `history.db`. Phase C
  or plan 020: `mirqab.service` has no `WantedBy`, so history waits for a
  first client.
- Phase D, step 27: re-subscribing sets "sample now" each time and can beat
  the floor; clamp in `sampler_loop` to at most one immediate sample a
  second (clearing it only for a new subscriber is not enough: unsubscribe
  then subscribe gets past). (The event that could arrive before its
  subscribe reply was fixed in phase C, after it failed CI on PR #3: the
  reply is queued in the step that adds the subscription.) `config.set live.intervalMs` does not
  re-floor live subscriptions already open.
- Phase D: an earlier interface is not restored when the latest one
  disconnects (`Client.ui_pid` is unused); a `config.json` that is a
  symlink is replaced by a regular file on write.
- Phase D, step 30, the first command the daemon runs: mark fd 3 from
  socket activation close-on-exec.
- Plans 002 and 005: `get` shares the subscription's sampler, so rate
  baselines can have short windows. Plan 005 (MCP, the first caller that is
  not ours): reject other malformed params instead of defaulting them; a
  cap on clients.
- Plans 006–008: `act::signal` checks the uid through `Root` but signals
  the real pid, and a pid can be reused between check and signal.
- Plans 008 and 017, the first callers that pass stdin: `exec.rs` writes
  stdin before draining the pipes, so more than 64 KiB deadlocks.

### Phase C — history (PR 3)

20. `samplers/system_core.rs`: CPU percent from `/proc/stat` deltas and
    memory from `/proc/meminfo`; `minute()` returns the four series.
21. `history.rs`: open with the pragmas of architecture §6, create or
    migrate the schema by `meta.schema`, `write_minute`, `query(series,
    range)` with avg and max per bucket, `events`, `prune`. A history that
    fails to open is renamed `history.db.broken-<unix time>` and a new one
    is made; `status` reports it.
22. The history thread: sleep until the next minute boundary on
    `CLOCK_REALTIME`, call every sampler's `minute()`, one transaction;
    prune hourly.
23. `history.query` and `history.events` methods; CLI `mirqab history
    <series> [--range R] [--json]` and `mirqab history --list`.
24. `tests/history.rs`; `tests/golden/system.json` (core shape).

Phase C as built (2026-11-13), where it differs from the steps above:

- **The 40 MB budget is tight, and holds only with two storage changes.**
  Measured by `crates/mirqab-core/tests/history.rs` on the development
  machine (SQLite 3.54.0, 4 KiB pages), 64 series × 43,200 minutes:
  - 43.4 MB written minute by minute with small values. Each series' run of
    rows grows in the middle of the `(series, ts)` key, so pages split half
    full; architecture §6.2's 35 MB assumed packed pages.
  - 44.7 MB with half the series holding byte counts in the tens of
    gigabytes (6-byte integers), packed except for the last day.
  - 42.0 MB after storing minutes from 2026-01-01 instead of 1970
    (`history::EPOCH`: a 3-byte integer for 15 years, not 4 bytes).
  - **39.1 MB** with one series in four holding byte counts, which is
    §6.1's mix (memory, swap, VRAM, network and disk bytes; the rest
    percentages, temperatures, speeds and counts). The test fills that mix.
  The two changes: the epoch, and `History::compact` (`VACUUM`) once a day
  from the minute writer, which packs the split pages.
  **Budget: 40 MiB** for the file and its write-ahead log (owner,
  2026-11-13, choosing it over 48 MB without compaction and over byte
  series in KiB). Measured with the log and no manual checkpoint, the last
  day written one transaction a minute: 40,295,008 bytes (38.4 MiB).
- `history.query` returns unscaled values, `[unixSeconds, avg, max]` per
  bucket, and omits empty buckets. Buckets align to `EPOCH`, which is on a
  2-hour boundary, so every range's buckets start on the hour.
- A memory minute stores the mean of the live samples taken during it when
  there were any, else one point sample (architecture §6.3).
- A minute whose counter baseline is more than 90 s old stores no CPU
  value: the machine slept or the daemon stalled.
- One daemon per socket: `<socket>.lock`, taken with `File::try_lock`
  before binding and held for the process's life. Two daemons started
  together both found a dead socket and both ran (review of phase B,
  deferred here because both would write `history.db`).
- `mirqab.service` is `WantedBy=default.target`, so the minute writer runs
  from login, not from the first client (review of phase B).
- `history.list` (the series history keeps) backs `mirqab history --list`;
  `mirqab history SERIES… [--range R] [--json]` prints one bucket a line.
  Plan 005 owns the rest of the read commands.

Review of phase C (Sonnet, checked by Fable, 2026-11-13). Fixed before
merge, each with a test:

- The newest minute was missing from every range for 59 seconds of each
  60: rows were labelled with the minute the writer woke in. They are now
  labelled with the minute their values cover (wake time − 30 s), so `1h`
  keeps 60 points and a slightly early wake lands in the same minute.
- `VACUUM` in WAL mode left a log as large as the database (about 72 MB
  on disk in all). `compact` now ends with a truncating checkpoint, the
  log is checkpointed every 256 pages and cut back to 1 MiB, and the size
  test measures without a manual checkpoint.
- Compaction ran only after 24 h of uptime, so a machine shut down nightly
  never compacted (43.3 MB). The last compaction's time is kept in `meta`.
- A history file with a newer schema was moved aside as broken and
  replaced by an empty one. Only a file SQLite calls not a database or
  corrupt is moved aside now; a newer schema leaves history off with the
  reason in `status`.
- Architecture §5.2, §6.2, §6.3 and §9.1 now describe the epoch, the
  label, the compaction, `history.list`, the reply shape and the budget.
- From the phase B review: `status` no longer lists a config problem the
  last `config.set` fixed, and the socket-path test fails after 5 s
  instead of hanging if that bug returns.
- CI on PR #3 lost a first event: the sampler could send it before the
  `subscribe` reply was queued, and a client skipping events while waiting
  for that reply dropped it (the ordering race the phase B review
  deferred). The reply is now queued in the same locked step that adds the
  subscription, before the sampler is woken; a test checks the order 100
  times.
- The writer sleeps on the monotonic clock, recomputed from the wall clock
  each minute (step 22 said `CLOCK_REALTIME`); after a suspend one write
  lands mid-minute and the next realigns.

Deferred:

- Phase E, step 32: the lock is per socket, so a second daemon on another
  socket shares `history.db`; key the lock on the history path when the
  benchmark first runs a second daemon on purpose. Also there: `mirqab
  daemon --root <fixture>` writes fixture values into the real history
  unless `XDG_STATE_HOME` is set.
- Plan 002: series outside `history::SERIES` are not reloaded at open
  (a restart loses `net.rx` until it is registered again); the `system`
  sampler's first minute has no CPU value, `swap.used` is a point sample,
  `live_prev` never expires and `cpu_percent` is not clamped; prune scans
  the whole table (37 ms an hour on a full file).
- Plan 005: clamp `history.query`'s `from` and `to` (a debug daemon panics
  the client thread on `i64::MIN`) and cap series and points; the CLI's
  exit codes for an unknown series or range, and `--dry-run` on `history`.
- The first plan that writes events: create `history.db`, its directory
  and the lock file 0600/0700.
- Plan 020: a socket-activated service that finds the lock held by a
  hand-run daemon exits 1 and restarts until that daemon stops.

### Phase D — interface shell (PR 4)

25. `crates/mirqab-view/src/{format,theme,icons,graph,shell,cards}.rs`
    and their tests. `theme.rs` holds every colour, radius, spacing and
    font role as a token with its default and its CSS variable name.
26. `crates/mirqab-ui/resources/style.css` and `src/style.rs`
    (architecture §7.3).
27. `crates/mirqab-ui/src/client.rs`, then `widgets/*.rs` as listed above,
    `Graph` last.
28. `src/{main,app,window,pages}.rs`: single instance, the layer-shell
    popover and the window mode, the hide timer, the keyboard shortcuts.
29. `src/tabs/<tab>/page.rs` placeholders for the sixteen tabs and
    `src/tabs/settings/page.rs`; `src/selftest.rs`.
30. The daemon side of `open`: if no client holds the `ui` role, run
    `systemctl --user start mirqab-ui.service` through `exec.rs`, queue the
    event, deliver it when the interface subscribes (5 s timeout, then an
    error to the CLI). `dist/mirqab.desktop`
    (`Exec=mirqab open --window`, `StartupWMClass` and icon name equal to
    the application id).
31. `crates/mirqab-ui/tests/widgets.rs`, `tests/test-ui-widgets.sh`,
    `tests/test-ui-shell.sh`, `start_broadway` in `tests/lib.sh`. Then
    measure the open interface's RSS and CPU on the development machine
    with the default renderer and with `GSK_RENDERER=cairo`, record both
    here, and set the renderer in `mirqab-ui.service` only if the
    difference is large enough to matter (state the numbers). The owner
    looks at one page on a monitor at scale 1.25.

### Phase E — budget and handover (PR 5)

32. `tools/bench-idle.sh [--subscribe STREAM [--live]]`, exactly as
    architecture §9.1 describes: private socket and temporary `HOME`,
    daemon on `root-desktop`, wait 5 s, utime plus stime from
    `/proc/<pid>/stat` before and after 60 s, divided by `60 × CLK_TCK`;
    `VmRSS`; context switches; refuse to run if a `mirqab-ui` is attached
    to that socket; exit non-zero at 0.3 % or 15 MiB (1 % with
    `--subscribe … --live`). Add it to the `test` job in `ci.yml` for
    changes under `crates/`, best of three runs.
33. Record the measured numbers in this plan's State line, with the date
    and the machine class. Update `README.md` ("Status") and
    `plans/README.md`.

## Unverified

Each is a first step of the phase that needs it.

- **Layer-shell link order** (phase D). `gtk4-layer-shell` must be loaded
  before `libwayland-client`. Check that the `gtk4-layer-shell` crate's
  link order is enough in a release build with LTO; if the popover comes
  up as a normal window, add
  `Environment=LD_PRELOAD=/usr/lib/libgtk4-layer-shell.so` to
  `mirqab-ui.service` and record that
  (https://github.com/wmww/gtk4-layer-shell, "Linking").
- **Closing the popover on focus loss** (phase D). Whether `is-active`
  turns false on a layer surface with keyboard mode `OnDemand` when the
  user clicks another window on Hyprland. If it does not, close on the
  `leave` of a `gtk::EventControllerFocus` on the window, or keep Escape
  and the tray click as the only ways and say so.
- **Broadway in the CI container** (phase A). `gtk4-broadwayd` starts as
  an unprivileged user with no display and `#[gtk::test]` runs against
  it. If it does not, fall back to `xorg-server-xvfb` with
  `GDK_BACKEND=x11` (installed here **[machine]**), and record which.
- **The popover under a headless Wayland compositor** (phase D,
  optional). Whether `cage` or `weston --backend=headless` (both in
  `extra`, neither installed **[machine]**) supports layer-shell well
  enough to run `--selftest` in popover mode in CI. If not, the popover
  stays a manual check.
- **GTK CSS variables in a provider loaded from a string** (phase D).
  That variables set on `:root` in the generated provider override
  libadwaita's own at priority application + 1, in dark and light.
- **Idle cost of an open, unchanged window** (phase D). GTK should draw
  nothing when no widget is invalidated; confirm with
  `GDK_DEBUG=frames` that an open window with no live graph produces no
  frames between events.
- **`mirqab-ui.service` environment.** `WAYLAND_DISPLAY` is in the user
  manager's environment on the development machine **[machine]**; on a
  session that does not import it, the unit fails. `mirqab open` then
  falls back to spawning `mirqab-ui` itself with the caller's
  environment, and says so.
- **The repository owner's spelling.** The owner wrote `t1nk333r/mirqab`;
  the git user on the development machine is `t1nk33r`. The application
  id `io.github.t1nk333r.mirqab` follows the repository; the coordinator
  confirms the spelling before the first push, and if it differs, every
  occurrence changes in one commit.
- **zbus.** Not needed until the first plan that speaks D-Bus; do not add
  the dependency here. When it is added it is 5.19.0, not 6.x
  (architecture §5.6).
