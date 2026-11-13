# Plan 003: Home tab and pinned cards

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW. Interface only, plus one config key.
- **Depends on**: 001, 002
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: DONE (2026-11-13). Acceptance 1 to 3 are tested by
  `tests/test-ui-home.sh` (seeded Home, pin and order stored in the
  config, and no `system` subscriber after `close`). Acceptance 4 in part:
  CI builds and tests from source in a clean `archlinux` container on every
  push; showing the window there was not run (it has no display). M1 is
  complete with this plan.

## Problem

Stat Space opens on a "pinnable Home": every card in every tab has a "Pin
to Home" control, and Home shows the pinned ones
(https://statspace.app/features/system-monitor, "The popover"; the
"Pin to Home" buttons are visible in the owner's screenshots). Home is the
first tab and the default when the window opens.

This plan completes milestone 1: after it the app opens on a useful page.

## Linux data sources

None of its own. Home shows cards that belong to other tabs and subscribes
to the streams those cards declare. No root.

## Not possible on Linux

Nothing. Dropped by choice: desktop widgets (Stat Space has WidgetKit
widgets; a Wayland desktop has no equivalent surface worth the always-on
cost, and the owner's rule is no visual clutter).

## Design

### The card registry

Every pinnable card is registered once, as data, in its tab's view model
(no GTK there, so the registry is tested without a display):

```rust
// crates/mirqab-view/src/tabs/system.rs
pub const CARDS: &[CardSpec] = &[
    CardSpec { id: "system.cpu",    title: Msg::CardCpu,    streams: &["system"], size: Size::Wide },
    CardSpec { id: "system.memory", title: Msg::CardMemory, streams: &["system"], size: Size::Half },
];
```

`mirqab_view::cards::all()` joins every tab's list. The widget side is
one table in `crates/mirqab-ui/src/tabs/mod.rs` from card id to a
constructor function; a test fails when an id is in one table and not the
other. A card widget takes two properties: `compact` (true on Home: no
range picker, shorter graph) and `pinned`. The same widget type renders
in its tab and on Home; there is no second implementation. A compact
card's graph has no range picker, so it shows the live range and follows
the same rule as any graph: 1 s while it is on screen, nothing while Home
is not.

### Pinning

- `config.home.pinned` is an ordered list of card ids. The "Pin to Home"
  control on a `Card` calls `config.set`; the daemon stores it and sends
  `config-changed`.
- First run (`pinned` empty and `home.seeded` false): seed with
  `system.cpu`, `system.memory`, `system.overview`. Later plans append
  their headline card to the seed list in their own checklist
  (Ports: listeners; Agents: sessions; Weather: now). Seeding happens once;
  an owner who unpins everything keeps an empty Home with a hint.
- A pinned id whose card is unknown (a plan removed it, or the stream is
  unavailable on this machine) is skipped, not deleted.

### Page

- A two-column flow: `wide` cards span both, `half` cards take one.
- Reorder with a drag handle shown on hover, or with Alt+Up and Alt+Down on
  the focused card. An unpin control on each.
- Empty state: one sentence and a button that jumps to System.
- Home subscribes only to the union of its cards' streams, and only while
  Home is the visible page. The exception is the tray tooltip and bar items
  (plan 004), which hold their own subscriptions.

### Opening

`mirqab open` with no tab opens Home. `window.rememberTab` (default false)
reopens the last tab instead.

## Rejected

- **A separate "summary" implementation per tab for Home.** Two versions of
  every card drift apart.
- **A free-form grid with resizable tiles.** Much more code, and the
  popover is narrow.
- **Keeping pinned cards' samplers alive while the window is closed.** It
  breaks the idle budget for nothing on screen.

## Tests and fixtures

No new fixture tree.

| Test | Checks |
|------|--------|
| `crates/mirqab-view/tests/home.rs` | seed list, order after move up and down, unknown ids skipped, the union of streams |
| `tests/test-ui-home.sh` (`mirqab-ui --selftest home`, plan 001) | `mirqab-ui --selftest home` against `root-desktop`: three seeded cards; pinning `system.gpu` from the System page adds it; unpinning removes it; `config.json` in the sandbox `HOME` has the list; with Home hidden and System shown, the daemon still has exactly one `system` subscriber |
| `crates/mirqab-core/tests/config.rs` | `config.set home.pinned` validates ids as strings, keeps order, writes atomically |

## Acceptance

1. A fresh install opens on Home with CPU, memory and the overview strip
   live.
2. "Pin to Home" on any System card puts it on Home; the order survives a
   restart of the interface and the daemon.
3. Closing the window drops every subscription Home held (`mirqab status`).
4. M1 as a whole: on a clean Arch container, building from source,
   starting the socket and running `mirqab open` shows Home and System
   working. The exact commands go in `README.md`.

## As built

Where it differs from the plan (2026-11-13):

- The registry entry is `cards::Card { id, tab, streams, size }`; the
  title stays with the widget, since the strings module is the only place
  for text. The widget table is `tabs::card(id, ctx, compact)` with
  `CARD_IDS`; a unit test in `crates/mirqab-ui/src/tabs/mod.rs` checks it
  against `cards::all()`.
- Each card is a `CardWidget` (`card()`, `update(stream, data)`) and
  reads the event itself, so the System page and Home feed their cards the
  same way. The System page builds its cards from `system::CARDS` through
  the same table.
- The GPUs are one card, "Graphics", with a section per GPU and one range
  picker for all of them, so `system.gpu` pins every GPU at once. Plan 002
  had one card per GPU.
- Seeding happens when the first config arrives from a daemon
  (`Ctx::seed_home`), not when Home is built, so a first run that opens on
  another tab is seeded too. The seed order is overview, CPU, memory.
- Reordering: a drag handle (`md-drag_vertical`) shown on hover or focus
  and dropped onto another card (`home::dropped`), and Alt+Up / Alt+Down
  anywhere inside a card (`home::moved`); the moved card keeps focus after
  the rebuild. A rebuild keeps the widgets of cards that stay pinned.
- `window.rememberTab` needs the last tab across restarts of the
  interface, which quits 60 s after it is hidden, so the config gains
  `window.lastTab`, written only while `rememberTab` is true.
- `--selftest` now builds its page once the config is known, as a real
  open does, and the dump's `visible` is false under a hidden parent
  (Home's hidden empty state counted as shown).
- The config test is in `crates/mirqab-core/src/config.rs`'s own tests,
  beside the other config tests, not in `crates/mirqab-core/tests/`.
- The pin control is not a `pinned` property set by the page: it reads
  `home.pinned` at each press and follows every config change.

From the review of PR #9 (2026-11-13):

- A config change moves the cards that stay pinned through the grid's
  layout children (`tabs::home::page::place`) instead of removing and
  re-attaching every card, which unmapped each graph and cleared its live
  history; a moved card keeps focus. A widget test checks that a swap and
  an unpin leave the other graphs live with their points.
- Something pinned from a tab before Home ever opened marks Home seeded
  (`home::mark_seeded`), so emptying Home later never refills it; the
  seeding writes `home.seeded` before `home.pinned`.
- An id listed twice shows once. A drop is written only when the dropped
  id is pinned.
- `tests/test-ui-home.sh` checks the range pickers by their toggle buttons,
  with System as the positive control, and the subscribers through
  `mirqab status`: one while Home is shown, one with System shown instead,
  none after `close`. The interface shares one daemon subscription per
  stream among its pages, so "one" is what the daemon can tell; that Home
  dropped its own handle is shown by the count reaching zero after
  `close`.

## Checklist

1. `crates/mirqab-view/src/cards.rs` (`CardSpec`, `all()`; built as
   `Card`, see As built); `CARDS` in
   `crates/mirqab-view/src/tabs/system.rs`; the constructor table in
   `crates/mirqab-ui/src/tabs/mod.rs`; give each System card widget the
   `compact` and `pinned` properties (built: `compact` at construction;
   the pin control follows the config, see As built).
2. `crates/mirqab-ui/src/widgets/card.rs`: the pin control, wired to
   `config.set`.
3. `crates/mirqab-view/src/tabs/home.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/home/{mod,page,pinned_card}.rs`.
4. `crates/mirqab-core/src/config.rs`: `home.pinned`, `home.seeded`,
   `window.rememberTab`; the schema file.
5. Tests above.
6. `README.md`: the "Try it" commands; set plans 001 to 003 DONE with
   counts; note M1 complete in `plans/README.md`.
