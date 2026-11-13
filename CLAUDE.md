# mirqab — agent guide

Read `docs/architecture.md` first: it is the contract between the parts.
Then `plans/README.md`, the index of the engineering record, and the plan
you are implementing. `AGENTS.md` is a link to this file.

mirqab is a Linux system monitor in the spirit of the macOS app Stat
Space: a Rust daemon, CLI and MCP server in one binary, a small opt-in
root helper for fans, and a Rust GTK4/libadwaita window in its own
binary (`mirqab-ui`) that exists only while it is open. The repository is
`github.com/t1nk333r/mirqab`. It is developed and tested on an Arch-based
machine.

## Ground rules

- **Copy nothing from Stat Space.** It is proprietary. It is a reference
  for behaviour and layout only: no code, no text, no icons, no images,
  no trademarked names in the product. No licences, no "Pro", no
  "Unlock", no Team planner (`docs/architecture.md` §1.2 lists every
  paid item and its decision).
- **Dry-run contract.** Every state change goes through
  `crates/mirqab-core/src/act.rs`, and every mutating command and action
  takes `--dry-run` / `dryRun`. Destructive actions need `confirm`.
  Commands run only through `exec.rs`. `sudo` and `pkexec` appear only in
  `crates/mirqab/src/elevate.rs`. Check with
  `grep -rnE '\b(sudo|pkexec)\b' crates dist | grep -v elevate.rs`
  (the two fixed helper scripts under `dist/` are the listed exceptions
  in `tools/lint.sh`).
- **Root only through the opt-in helper.** The daemon and the interface
  run as the user. `mirqab-fand` ships disabled; only
  `mirqab helper enable` turns it on, with one prompt. Never add a
  capability, a setuid bit, a udev rule or a second privileged path. The
  helper accepts no paths and runs no commands (§8.1).
- **Read system state through `Root`.** No code opens `/proc`, `/sys` or
  `/etc` by a literal path. Tests run against the fixture trees in
  `tests/fixtures/`, with a stub `PATH`, a temporary `HOME` and a private
  D-Bus, never against the live machine.
- **Resource rules** (§9.2). No polling under 1 s, and 1 s only for a
  stream whose live graph is on screen (the `live` flag, set only by the
  `Graph` widget; §5.3). Everything else runs at 2 s or slower. Sample
  only what is visible, pinned, in a bar or needed by an enabled rule.
  Prefer an event to a poll. No blur, no shaders, no frame-clock redraw
  loops. Pages are built lazily and dropped on hide. Every
  `glib::timeout_add*` carries a `// mirqab:ui-timeout` or
  `// mirqab:sample` marker. The idle budget is under 0.3 % of one core
  and 15 MiB with the window closed; `tools/bench-idle.sh` gates it.
  Measure before claiming a number.
- **The daemon links no GTK.** `mirqab`, `mirqab-core`, `mirqab-proto`,
  `mirqab-view` and `mirqab-fand` must not depend on `gtk4`, `glib`,
  `libadwaita` or `cairo-rs`; lint checks `cargo tree`. Logic that needs
  no widget goes in `mirqab-view`, so it is tested without a display.
- **CSS variables, not hex.** Widgets use libadwaita's style classes and
  named colours (`var(--accent-bg-color)` and the like). Colour literals
  exist only in the token block of `crates/mirqab-ui/resources/style.css`
  and in `crates/mirqab-view/src/theme.rs` (§7.3). Strings come from
  `crates/mirqab-proto/src/strings.rs` (English only for now; one place,
  so translation is possible later). Icons are Nerd-font glyphs named in
  `crates/mirqab-view/src/icons.rs`.
- **The repository is public.** Never commit a real hostname, address,
  interface name, MAC, SSID, serial number, coordinates more precise
  than a city, the owner's location, a token, a key, or anything copied
  from a live `~/.config`. Fixtures are synthetic: documentation address
  ranges, made-up names. `tools/secrets.sh` checks this.
- **Privacy in the product** (§10). No telemetry, no update check. A
  network request happens only when the user started it or turned it on.
  History, events, logs and reports never hold command lines,
  environment values, interface names, SSIDs or serials. Process
  environments are read for marker variable *names* only.
- **Agents' files are read-only.** The Agents tab reads session files and
  credentials of coding agents. It never writes them, never refreshes a
  token, never changes how any agent signs in. Usage limits are opt-in
  and use the agent's own stored token, which is never copied or logged.
- **No endpoint ships for the speed test or the public address.** Both
  are empty settings. Do not add a default host. Weather uses Open-Meteo
  by default, with the endpoint as a setting.
- **Never edit the user's files** outside `~/.config/mirqab`,
  `~/.local/state/mirqab` and `~/.cache/mirqab`. Bar, compositor and
  haseen integration are documented examples the user installs.
- **No visual clutter.** A new on-screen element needs a reason in its
  plan. Default to a card the user pins, not something always visible.
- **Licence: MIT.** You may adapt MIT code (haseen, Omarchy,
  DankMaterialShell): keep the upstream notice in the file header and
  add a row to `NOTICE.md`. GPL projects are reference only. Never paste
  their code.
- **Dependencies are few.** A new crate needs a line in the plan that
  adds it: what it replaces and what it costs in binary size (§13).
  `mirqab-fand` depends on `libc` and `serde_json` only.

These haseen rules do **not** apply here: the default-plugin approval
list and the package-source order.

## Gates (run before every commit)

```sh
tools/lint.sh             # cargo fmt --check, clippy -D warnings, cargo deny, bash -n, shellcheck, jq, the greps above, coverage
tests/run.sh --affected   # hermetic; fixtures; only the tests your change affects
tools/check-docs.sh       # plans/README.md in step with plans/, links, a golden file per stream
tools/secrets.sh          # before every push: gitleaks (history) + the privacy patterns
```

**Test the affected code, not the whole suite.** While iterating, run the
one file you are changing (`tests/run.sh tests/test-<name>.sh`, or
`cargo test -p mirqab-core --test <name>`, or
`cargo test -p mirqab-view --test <tab>`, which needs no display).
Widget and page tests need a headless GTK display: `tests/lib.sh` starts
`gtk4-broadwayd` for them (§7.5). The local gate is
`tests/run.sh --affected [BASE]`: it runs the test files whose
`# mirqab:covers` globs match a path changed since the merge base with
BASE (default `origin/main`), staged, unstaged and untracked included;
`tools/affected-tests.sh` prints why. A change to the test machinery
(`tests/lib.sh`, `tests/run.sh`, `tools/affected-tests.sh`,
`tools/coverage-exempt.txt`, `.github/workflows/`) runs every test, and a
change to shared infrastructure (`crates/mirqab-proto/`,
`crates/mirqab-core/src/{root,exec,act,server}.rs`, `Cargo.toml`,
`Cargo.lock`, `deny.toml`, `rust-toolchain.toml`) runs every test that
declares `# mirqab:covers-shared`: say so, and do not fall back to the
whole suite by hand. The other gates under `tools/` select only the test
that covers them. The full run is CI's job on
`main` and on a release tag. A new test declares what it covers in its
leading comments; lint fails on a file no test covers.

Tests that need scratch trees create them under `${TMPDIR:-/tmp}` and
remove them on exit, never inside the checkout.

`main` on GitHub (`t1nk333r/mirqab`, public) is protected:
- The CI checks `lint`, `test`, `docs` and `secrets` must pass on an
  up-to-date branch. They are the four jobs of `.github/workflows/ci.yml`.
- History stays linear: no force pushes and no deletion.
- Admins are included.

Direct pushes to `main` are rejected. Every change lands through a pull
request:

```sh
git switch -c <topic> && git push -u origin <topic>
gh pr create --fill && gh pr merge --squash --auto --delete-branch
```

The one exception is the bootstrap in plan 001: the coordinator pushes
the design as the first commit and turns protection on once plan 001's
first pull request has made the four checks report. Implementers never
push to `main` and never change repository settings.

## How work is divided

A planner writes `docs/architecture.md` and `plans/`. An implementer
builds one plan, or one phase of it, per pull request. A reviewer reviews
it; a second reviewer reviews the review. A documentation writer updates
`README.md` and `docs/`. Whoever you are:

- A plan's **Unverified** items are its first checklist steps. Check
  each on a real machine, write the result into the plan, and stop to
  ask if the result changes the design.
- Do not widen a plan. A new idea is a new plan or an owner question.
- Security-sensitive parts are their own pull request with tests written
  first: the root helper (plan 015 phase 2), file deletion (plan 010
  phase 3), signalling processes (plans 006 to 008), the MCP confirm
  flow (plan 005).
- Hardware acceptance steps (fans, a laptop battery) are the owner's.

## Style

- Rust: edition 2024, `cargo fmt`, clippy clean, no `unsafe` outside
  `root.rs`, `sys.rs`, `uevent.rs` and the helper's syscalls, each with a
  `// SAFETY:` comment (GObject subclassing in `mirqab-ui` uses the
  `glib` macros, not hand-written `unsafe`). No async runtime of our
  own: the daemon uses std threads, the interface GLib's main loop. Errors are values with one of
  the protocol's codes, not panics.
- Shell: `set -Eeuo pipefail`, 4-space indent, functions `snake_case`.
  Comments say *why*.
- Interface: widgets are built in Rust (no `.ui` files), one widget type
  per file. A page is `crates/mirqab-ui/src/tabs/<tab>/page.rs`, with its
  logic in `crates/mirqab-view/src/tabs/<tab>.rs`, which has no GTK and
  is tested without a display. Custom drawing is `snapshot()` with
  `GtkSnapshot` and paths, not `GtkDrawingArea` (§7.4). Data comes from
  the daemon through `client.rs` (the Volume page's use of the
  PulseAudio client API is the one recorded exception). The only QML in
  the repository is `contrib/haseen/`.
- Streams: adding or changing a field changes `tests/golden/<stream>.json`
  in the same commit. Field names are `camelCase`; units are in the name
  (`Bytes`, `PerSec`, `Percent`, `Unix`, `Celsius`).
- Commits: `scope: lowercase imperative` (for example
  `fand: restore the saved enable value on disconnect`).
- Plans: `plans/NNN-slug.md` with a Status block. Change its row in
  `plans/README.md` in the same commit. Every claim cites evidence (a
  file and line, a command's output, a commit, a URL). Record rejected
  options.
