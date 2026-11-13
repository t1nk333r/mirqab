# Plans

The numbered engineering record. Every change of substance has a plan file;
this table is the index, and `tools/check-docs.sh` keeps the two in sync.
Read `docs/architecture.md` first: it is the contract the plans build to.

| Plan | Title | Milestone | Priority | Effort | Depends on | Status |
|------|-------|-----------|----------|--------|------------|--------|
| 001 | Foundation: skeleton, daemon, IPC, history, GTK4 app shell, theming, gates, CI | M1 | P1 | XL | — | IN PROGRESS (phases A–C) |
| 002 | System tab | M1 | P1 | L | 001 | PLANNED |
| 003 | Home tab and pinned cards | M1 | P1 | M | 001 002 | PLANNED |
| 004 | Tray icon and bar items | M2 | P1 | M | 001 002 | PLANNED |
| 005 | CLI and MCP server | M2 | P1 | M | 001 002 | PLANNED |
| 006 | Apps tab and process inspector | M3 | P1 | L | 001 005 | PLANNED |
| 007 | Ports tab | M3 | P1 | L | 001 005 006 | PLANNED |
| 008 | Agents tab | M3 | P1 | L | 001 005 006 007 | PLANNED |
| 009 | Awake tab | M3 | P2 | M | 001 005 | PLANNED |
| 010 | Disk tab | M4 | P2 | L | 001 | PLANNED |
| 011 | Network tab | M4 | P2 | L | 001 | PLANNED |
| 012 | Devices tab | M4 | P2 | L | 001 | PLANNED |
| 013 | Battery tab | M4 | P3 | S | 001 012 | PLANNED |
| 014 | Volume tab | M4 | P2 | M | 001 | PLANNED |
| 015 | Fans tab and the root helper | M5 | P2 | L | 001 002 | PLANNED |
| 016 | Time tab | M6 | P2 | L | 001 | PLANNED |
| 017 | Weather tab | M6 | P2 | M | 001 016 | PLANNED |
| 018 | Trends, spike rewind, alerts and "why is it slow" | M7 | P2 | L | 002 006 010 011 | PLANNED |
| 019 | Support tab and the health report | M7 | P3 | M | 010 012 013 018 | PLANNED |
| 020 | Packaging and release | M8 | P2 | S | 001–005 | PLANNED |

## Milestones

Each milestone can be implemented, reviewed and merged by itself, and
leaves `main` in a state a user could install. The toolkit decision of
2026-11-13 changed no milestone's plans (see "What the toolkit change
moved" below).

| Milestone | Plans | What the user has afterwards |
|-----------|-------|------------------------------|
| **M1 Running app** | 001, 002, 003 | `mirqab open` shows the window with sidebar and footer, a System tab whose graphs update every second while on screen, with 30 days of history, and a Home tab of pinned cards. Gates and CI exist, and `main` is protected |
| **M2 Outside the window** | 004, 005 | A tray icon, bar items for Waybar and haseen, the CLI's read commands and the MCP server |
| **M3 Developer tabs** | 006, 007, 008, 009 | Apps, Ports, Agents and Awake, each with its CLI commands and MCP tools |
| **M4 Machine tabs** | 010, 011, 012, 013, 014 | Disk, Network, Devices, Battery and Volume |
| **M5 Fans** | 015 | Fan readings for everyone; curves for those who enable the root helper |
| **M6 Everyday** | 016, 017 | Time and Weather |
| **M7 Insight** | 018, 019 | Trends, spike rewind, alerts, "why is it slow", the health report |
| **M8 Release** | 020 | A checked PKGBUILD, a tagged release, the AUR package (owner step) |

Order inside a milestone is the plan number. M3 to M6 do not depend on each
other and can run in parallel by different agents once M2 is merged; M7
needs M3 and M4 for the series and events it reads.

Plan 001 is XL on purpose: it is split into five phases (A to E in its
checklist), and each phase is one pull request. Phase A's pull request
is the one that adds `.github/workflows/ci.yml`; the coordinator turns
on branch protection when its four checks have reported, before it
merges.

## Conventions

- A plan file is `plans/NNN-slug.md` and starts with a Status block
  (Priority, Effort, Risk, Depends on, Category, Planned at, State).
- Sections, in this order: Problem, Linux data sources (with "root?" for
  each), Not possible on Linux, Design (stream shape, actions, UI), Rejected,
  Tests and fixtures, Acceptance, Checklist, Unverified.
- Every claim about the system cites evidence: a file and line, a command's
  output, a commit, or a URL. **[machine]** and **[unverified]** mean what
  they mean in `docs/architecture.md`.
- A plan's "Unverified" items are its first checklist steps. The implementer
  checks each, writes the result into the plan, and stops to ask if the
  result changes the design.
- Interface files in a plan are always two sets: the view model with no
  GTK in `crates/mirqab-view/src/tabs/<tab>.rs`, tested by
  `crates/mirqab-view/tests/<tab>.rs` with no display, and the widgets in
  `crates/mirqab-ui/src/tabs/<tab>/`, tested once per page by
  `tests/test-ui-<tab>.sh` through `mirqab-ui --selftest <tab>`
  (architecture §7.5).
- A plan that samples states its interval twice: with a live graph on
  screen (1 s) and without (2 s or slower).
- A row here changes in the same commit as the plan file.
- States: PLANNED, IN PROGRESS, DONE (with the date and the test count),
  DROPPED (with the reason).

## Owner decisions (2026-11-13)

The ten questions this file used to ask are answered. Each answer is
written into the plan it affects, with the date.

| # | Question | Decision | Where |
|---|----------|----------|-------|
| 1 | Name | **mirqab**, final | everywhere |
| 2 | Publish | Public GitHub repository `t1nk333r/mirqab` now. `main` protected as haseen's is: checks `lint`, `test`, `docs`, `secrets` on an up-to-date branch, linear history, no force pushes, administrators included, pull requests only. The coordinator creates the repository; protection is turned on once plan 001's `ci.yml` has reported the four checks | architecture §16.1, plan 001 |
| 3 | Live interval | 1 s while a live graph is on screen. Everything not on screen keeps its own slower interval; history stays per-minute; the idle budget stays under 0.3 % of one core | architecture §5.3 and §9, plan 001, every tab plan with a graph |
| 4 | Interface toolkit | Rust with GTK4 and libadwaita (gtk4-rs), not Quickshell/QML | architecture §2, §4, §7, plan 001, every plan's interface files |
| 5 | Distribution | Testing happens on the development machine, which reports `ID=omarchy` and is Arch-based. The docs say "Arch-based" | architecture, plan 020 |
| 6 | Speed test and public address | No endpoint ships; both are empty settings | architecture §10, plan 011 |
| 7 | Agent usage limits | Build them: opt-in, read-only, with the agent's own stored token, which is never copied or logged | architecture §10, plan 008 |
| 8 | haseen plugin | In this repository, under `contrib/haseen/` | plan 004 |
| 9 | Licence | MIT | architecture §15 |
| 10 | Languages | English only in the first release; strings kept in one module for later translation | architecture §7.2, plan 001 |
| — | Weather provider | Open-Meteo by default; the endpoint is a setting; its non-commercial terms are stated in `README.md` | plan 017 |

### What the toolkit change moved

No plan was added or removed and no plan changed milestone. Work moved
inside plans:

| Plan | Change |
|------|--------|
| 001 | Two more crates (`mirqab-proto`, `mirqab-view`) and the `mirqab-ui` binary replace the QML tree. The graph widget is written new, not adapted from haseen. Headless tests run under Broadway. The gates gain `cargo deny` and lose `qmllint`. The plan also carries the `live` flag, the CI workflow file and the branch-protection hand-over. Still XL, still five phases |
| 004 | The tray is decided, not left open: `ksni` in the daemon, which holds `zbus` on 5.x. The haseen plugin is the only QML left |
| 007 | The QR code is a custom widget, written new |
| 014 | The largest change: the Volume page uses the PulseAudio client API (`libpulse-binding`) in place of Quickshell's PipeWire module. Risk raised from LOW to MEDIUM; effort stays M |
| 016 | The dotted map is one path in a custom widget |
| 019 | PDF export and the full-screen display test use GTK calls, both unverified |
| 020 | Installs `/usr/bin/mirqab-ui` instead of a QML tree; depends on `gtk4`, `libadwaita`, `gtk4-layer-shell`; adds `release.yml` |

## Open owner questions

1. **The repository owner's spelling.** The decision says
   `t1nk333r/mirqab` (three 3s); the git user on the development machine
   is `t1nk33r` (two). The application id
   (`io.github.t1nk333r.mirqab`), the issues link and the README follow
   whichever is right. Which is it?
2. **`zbus` held on 5.x.** The tray crate `ksni` 0.3.7 needs `zbus` 5,
   so the daemon uses 5.19.0 while 6.3.0 exists. The alternative is to
   write the tray's D-Bus interfaces by hand (plan 004 keeps it as the
   fallback). Accept the pin?
3. **An Unlicense dependency.** `ksni` is under the Unlicense
   (public-domain dedication). It is compatible with MIT; it has to be
   on `cargo deny`'s allow-list. Accept?
4. **`libpulse` required or optional.** The Volume page links
   `libpulse`. Simplest is to make it a hard dependency of the package
   (plans 014 and 020 list the alternative). Accept a hard dependency?
5. **Hardware for acceptance.** Fan curves (plan 015) and the Battery
   tab (plan 013) need a machine with controllable fans and one with a
   battery. Testing is on one Arch-based desktop; which machine does the
   owner use for those two acceptance steps, or do they ship marked
   "tested on fixtures only"?
