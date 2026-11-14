# 022 — A real window by default

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW–MEDIUM. The default is one value in three places and the
  window mode already exists and is what the tests run. The risk is in what
  only Hyprland can show: whether an open window on another workspace comes
  to the user, and how the window controls look.
- **Depends on**: 001, 004
- **Category**: interface
- **Planned at**: 2026-11-14, from the owner
- **State**: DONE (2026-11-14). The owner's checks (U1–U4, acceptance
  1–6) stay open.

## Problem

mirqab opens as a layer-shell popover by default. On the owner's Hyprland
(`follow_mouse = 1`) the popover closes as soon as the pointer moves over
another window: keyboard focus follows the pointer, the layer surface's
`is-active` turns false, and the popover hides. This answers plan 001's
open item "Closing the popover on focus loss"
(plans/001-foundation.md:987-991): `is-active` does turn false, and too
eagerly.

The owner first chose an `xdg_popup` with a grab, then overruled it
(2026-11-14): "i need a window a real window not a popup". The owner's
config already carries the workaround, `window.mode = "window"`.

**Decision (owner, 2026-11-14): the default is a real window.** The popover
stays as an opt-in mode.

## Evidence

| # | Fact | Source |
|---|------|--------|
| E1 | The default is `"popover"` in the daemon's defaults, the schema and the view's `Mode`. | crates/mirqab-core/src/config.rs:16, :46; share/mirqab/config.schema.json:11; crates/mirqab-view/src/config.rs:9-14, :130 |
| E2 | The config file stores only overrides, so changing the default changes every user who never set the key. | owner's brief, 2026-11-14; architecture §config |
| E3 | Both modes are one `adw::ApplicationWindow` titled `strings::APP_NAME` with `set_hide_on_close(true)`; the application id is `io.github.t1nk333r.mirqab`, the desktop entry's `StartupWMClass`. | crates/mirqab-ui/src/window.rs:57-62; crates/mirqab-ui/src/app.rs:21; dist/mirqab.desktop |
| E4 | The window mode has no window controls: the close and Keep-open buttons are added only in popover mode, and the content is set with `set_content` with no header bar. | window.rs:100-106, :117 |
| E5 | The size is `widthEm` × `heightEm` (58 × 40) times the font's em, clamped to monitor 0, set with `set_default_size`. A config change re-places only a popover; "a window keeps the size the user dragged it to". | window.rs:157-168; app.rs:384-392; plans/001-foundation.md:973-974 |
| E6 | Hiding does not destroy: `hide()` is `set_visible(false)`, the window is reused if the mode is the same, and the process exits 60 s after a hide. | window.rs:323-328; app.rs:214-282, :285-302 |
| E7 | `toggle` hides when the window `is_visible()`, wherever it is, and shows otherwise. | app.rs:176-191 |
| E8 | A compositor close request hides instead of closing. | app.rs:255-260 |
| E9 | Escape is bound to hide in both modes. | window.rs:239 |
| E10 | A popover closes when `is-active` goes from true to false, unless Keep open is on. Keep open does not survive a hide. | window.rs:266-284; plans/001-foundation.md:915-921 |
| E11 | The popover is a layer surface: layer `Top`, keyboard mode `OnDemand`. | window.rs:144-150 |
| E12 | `mirqab open` sends no mode unless `--window` is given; without one the UI takes the visible window's mode, else the config's. | crates/mirqab/src/cli.rs:307; app.rs:214-282; tests/test-cli.sh:57 |
| E13 | The selftest already asks for window mode, and the Broadway tests open with `--window`. | crates/mirqab-ui/src/selftest.rs:21; tests/test-ui-shell.sh |
| E14 | A popover asked for without layer shell (Broadway) becomes a window and sets the footer notice `POPOVER_UNSUPPORTED`. | window.rs:55-56, :131-133; strings.rs:705-706 |
| E15 | The `"popover"` values in the stream tests and the golden file are examples of the `show` event's shape, not the default. | crates/mirqab-proto/src/stream.rs:241, :245, :270; tests/golden/ui.json:2 |
| E16 | GTK 4 removed the window position API: "This includes gtk_window_set_position(), … gtk_window_move(), gtk_window_parse_geometry()". | https://docs.gtk.org/gtk4/migrating-3to4.html (read 2026-11-14) |
| E17 | `gtk_window_present`: "This may mean raising the window in the stacking order, unminimizing it, moving it to the current desktop and/or giving it the keyboard focus (possibly dependent on the user's platform, window manager and preferences)." | https://docs.gtk.org/gtk4/method.Window.present.html (read 2026-11-14) |
| E18 | The docs and help that say the popover is the default. | docs/architecture.md:223-242, :265-271, :550, :555; README.md:59-67; strings.rs:42, :849; docs/cli.md:28; plans/001-foundation.md:162, :256-262, :367-370 |
| E19 | Hyprland's Lua rule form is `hl.window_rule({ name, match = { class = … }, float = true })`; a class pattern is matched against the whole string. | haseen `share/haseen/default/hypr/windowrules.lua:1-3`, `:28-30` |
| E20 | Versions on the owner's machine: gtk4 4.24.1, libadwaita 1.10.0, gtk4-layer-shell 1.3.0, hyprland 0.56.2. | `pacman -Q`, 2026-11-14 **[machine]** |

## Design

### D1. The default mode is `"window"`

Change the default in the three places of E1. `Mode`'s `#[default]` moves
to `Window`. Settings lists Window first
(crates/mirqab-ui/src/tabs/settings/page.rs:17). No migration: a config
that sets the key keeps its value, everything else follows the new default
(E2). The schema's enum and the validator are unchanged.

`--window` stays: it still overrides a config that says popover, and the
desktop entry keeps using it. No `--popover` flag is added (Rejected).

### D2. What the window is

- **Title** `mirqab`, **application id** `io.github.t1nk333r.mirqab`: both
  already true (E3), so a compositor rule and the desktop entry match it.
- **Size**: `widthEm` × `heightEm` on first map, as today (E5). On a tiling
  compositor the compositor decides the size; the default matters when the
  window floats.
- **Size is remembered while the process lives**, because a hidden window
  is reused (E6). After the 60 s exit the next open starts from
  `widthEm`/`heightEm` again. Those two keys are the remembered size; the
  dragged size is not written to the config in this plan (Rejected).
- **Position is never remembered, and cannot be.** GTK 4 has no call to
  place or read a toplevel's position (E16), and a Wayland client is not
  told where its toplevel is. Where the window goes is the compositor's
  decision; on Hyprland the README's rule does it (D7).
- **Window controls.** Today a window-mode mirqab has none (E4), which is
  survivable on Hyprland (a key closes it) and a defect on a desktop that
  expects client-side controls. In window mode the header row becomes a
  `gtk::WindowHandle` (so it drags the window) and ends with a
  `gtk::WindowControls`. The close button raises the normal close request,
  which hides (E8). The popover's header is unchanged.

### D3. Tray click and `mirqab toggle`: show or hide, never destroy

Unchanged from E6–E7, now stated as the contract: `toggle` hides a visible
window and presents a hidden one; the window object and its size survive;
the process exits 60 s after a hide. The docs' "left click toggles the
popover" becomes "toggles the window".

### D4. A window on another workspace

- **`toggle` hides it** (one press), and the next press shows it on the
  current workspace: a hidden window that is shown is a new map, and a
  compositor maps new windows where the user is. So the tray click always
  ends with the window either gone or in front of the user in at most two
  presses, and it never moves the user to another workspace.
- **`open` / `show` (the tray's secondary click, `mirqab open TAB`)
  presents it**: `Window::present()` as today. What that does is the
  compositor's choice (E17).

Why not "raise if visible but not focused" for `toggle`: the UI cannot tell
"on another workspace" from "visible here, focus elsewhere" — both are
visible and inactive — and plan 001 already records that a bar click may
take focus before the toggle arrives (plans/001-foundation.md:915-921). A
toggle that raised on "inactive" could then never hide from the tray. Hide
on visible is the one rule that is always predictable.

If the owner's check shows `present()` does nothing useful on Hyprland for
a window on another workspace (U1), `show` on a visible, inactive window
re-maps it (`set_visible(false)` then `present()`, without dropping pages
or the subscription). That fallback is in this plan's scope; it applies to
`show` only, never to `toggle`.

### D5. Escape

Escape hides a **popover** and does nothing in a **window**. A normal
window that vanishes on Escape is a surprise (Escape is also "leave this
search field"). In window mode the ways to close are the window's close
button, the compositor's close key, the tray click, `mirqab close`, and
`Ctrl+W` (added, window mode only). The rule lives in the headless view
crate — `Mode::closes_on_escape()` — so a unit test covers it, and
window.rs:239 asks it.

### D6. The popover stays, opt-in, with its limit written down

The popover is still reachable from Settings ("Opens as"), the footer menu
("Open as popover") and `window.mode`. Its code does not change. The docs
gain one honest paragraph: with focus-follows-mouse (Hyprland's
`follow_mouse = 1`) a popover closes when the pointer moves onto another
window; turn on Keep open (the pin in its header) or use the window. Keep
open still does not survive a hide (E10); that stays plan 001's deferred
item. Plan 001's Unverified entry is closed with the owner's result.

### D7. README: one optional Hyprland rule

After the bind lines, an optional rule that floats the window, in the Lua
form (E19):

```lua
hl.window_rule({
  name = "mirqab_float",
  match = { class = "io\\.github\\.t1nk333r\\.mirqab" },
  float = true,
})
```

A size or position line is added only after U3 confirms the key names on
Hyprland 0.56; an unchecked rule in a README is worse than none. The
existing bind lines are in the older syntax (README.md:263-268); bringing
them to Lua is part of the same README edit if U3 confirms it.

### D8. Words

All in `crates/mirqab-proto/src/strings.rs`: the `open` help and the UI
usage line (":42", ":849") become "--window opens a normal window even when
the config asks for a popover"; `docs/cli.md` is regenerated. No new
user-visible string except the `Ctrl+W` entry if the shortcuts list shows
one. `POPOVER_UNSUPPORTED`, `PINNED`, the Settings labels stay.

## Rejected

| Option | Why not |
|--------|---------|
| `xdg_popup` with a grab (the owner's first choice) | Overruled by the owner on 2026-11-14 in favour of a real window. It also needs a parent surface to hang from, which mirqab does not have — the reason the architecture already rejects `GtkPopover` (docs/architecture.md:262-263); the bar's surface belongs to another process. Deferred, not planned. |
| Keep the popover default and stop closing on focus loss | Then nothing closes it but Escape and the tray; a click outside would leave a panel on the `Top` layer over the user's work. |
| Close the popover on a click outside instead of on `is-active` | A layer surface is not told about clicks on other surfaces. A full-screen transparent catcher surface would do it; that is a new surface and new failure modes, not a small fix. |
| A grace delay before the popover closes | Guesswork about how long the pointer may wander; still closes under the owner's setting. |
| Remove the popover | It works for click-to-focus users and is asked for by name in Settings; removing it is a larger, separate decision. |
| Save the dragged window size to the config | Needs a write on resize or hide, and on a tiling compositor it would record the tile's size. `widthEm`/`heightEm` already exist. Revisit if the owner floats the window and wants it. |
| Remember the position | Impossible from the client (E16, D2). |
| `toggle` raises a visible, unfocused window | Cannot tell "elsewhere" from "unfocused here"; could make the tray click unable to hide (D4). |
| Escape hides the window too | D5. |
| A `--popover` flag | The popover is opt-in by config, Settings and menu; a second flag doubles the cases in `cli.rs` and test-cli.sh for no asked-for use. |
| Change the `"popover"` examples in stream tests and the golden file | They test the event's shape (E15), not the default. |

## Files

| File | Change |
|------|--------|
| `crates/mirqab-core/src/config.rs` | default `"window"` |
| `share/mirqab/config.schema.json` | default `"window"` |
| `crates/mirqab-view/src/config.rs` | `#[default]` on `Window`; `closes_on_escape()`; tests |
| `crates/mirqab-ui/src/window.rs` | window-mode header (handle, controls); Escape by mode; `Ctrl+W` |
| `crates/mirqab-ui/src/app.rs` | only if U1 fails: re-map on `show` |
| `crates/mirqab-ui/src/tabs/settings/page.rs` | Window first |
| `crates/mirqab-ui/src/selftest.rs` | open with the config's mode (`None`) instead of forcing window |
| `crates/mirqab-proto/src/strings.rs`, `docs/cli.md` | D8 |
| `docs/architecture.md` | §4: Window is 1 and the default, Popover is 2 and opt-in with the D6 paragraph; tray wording (:239); :550, :555 |
| `README.md` | :59-67 rewritten for the window default, Escape, the popover's limit; the rule (D7) |
| `tests/test-ui-shell.sh` | Tests |
| `plans/001-foundation.md` | as-built lines: default (:162), Escape (:256-262), acceptance 3 (:367-370), the Unverified entry closed (:987-991) |
| `plans/README.md` | row 022 |

## Tests and fixtures

Affected scope only: the unit tests of mirqab-core and mirqab-view,
`tests/test-ui-shell.sh` (Broadway), and the docs check for `docs/cli.md`.

- **mirqab-core**: the default of `window.mode` is `"window"`; the existing
  test that the Rust defaults and the schema agree keeps both in step.
- **mirqab-view**: a config without the key parses to `Mode::Window`;
  `"popover"` still parses; `closes_on_escape()` is true for Popover only.
- **test-ui-shell.sh** (Broadway has no layer shell, so E14 tells the modes
  apart):
  - with a default config, `--selftest home` has no
    `POPOVER_UNSUPPORTED` notice in the widget dump and does have a
    `WindowControls`;
  - with `window.mode = "popover"` in the test config, the notice is there
    and the `WindowControls` is not;
  - `--background`, then `mirqab open` (no `--window`), `mirqab toggle`,
    `mirqab toggle`: the `ui` stream shows show, hide, show, with one
    process throughout; then `mirqab close` and the exit after
    `MIRQAB_UI_EXIT_MS`, as today.
- **Not covered headless** (owner's check): Escape and `Ctrl+W` as real
  key presses, the window controls' look, another workspace, the rule.

## Acceptance

1. With no `window.mode` in the config, the tray click and `mirqab toggle`
   open a normal window that stays open when the pointer moves over other
   windows (Hyprland, `follow_mouse = 1`).
2. A second click or `toggle` hides it; a third shows it again at the size
   it had, in the same process. The process is gone 60–70 s after a hide.
3. With the window on another workspace, `toggle` twice puts it in front of
   the user on the current workspace.
4. Escape does nothing in the window; `Ctrl+W`, the close button and the
   compositor's close key hide it. Escape still closes a popover.
5. Settings → Opens as → Popover still gives the anchored popover, and the
   README says what focus-follows-mouse does to it.
6. With the README's rule, the window floats.
7. `tools/lint.sh`, the affected tests, `tools/check-docs.sh` and
   `tools/secrets.sh` pass.

## Checklist

- [ ] U1: on Hyprland, with the window visible on another workspace, run
      `mirqab open`. Record what `present()` does; decide the D4 fallback.
- [ ] U2: on Hyprland, see the window-mode header with `WindowControls`
      (which buttons show, whether the handle drags a floating window).
- [ ] U3: check the rule in D7 on Hyprland 0.56, and the key names for a
      size; write only what was run.
- [ ] U4: with the window focused, click the tray icon with
      `follow_mouse = 1`: the click hides it (it does not depend on focus).
- [x] Defaults (core, schema, view) and their tests.
- [x] window.rs: header, Escape by mode, `Ctrl+W`; selftest mode.
- [x] test-ui-shell.sh cases.
- [x] Strings, `docs/cli.md`, architecture §4, README, plan 001 lines, row
      in plans/README.md.
- [ ] Gates; owner's check (acceptance 1–6).

## As built

- The default is `"window"` in `defaults()`, the schema and `Mode`;
  Settings lists Window first. `Mode::closes_on_escape()` is true for
  Popover only; window.rs binds Escape to hide only then, and `Ctrl+W`
  (`<Control>w`) otherwise.
- The header row is a `gtk::WindowHandle` around the old header box; in
  window mode the box ends with `gtk::WindowControls::new(PackType::End)`.
  The controls follow the mode the window **got**, not the one asked for,
  so a popover that falls back to a window (no layer shell) gets them too:
  it is a real window. The plan's test expected none there; the test now
  expects one.
- `--selftest` opens with the config's mode (`None`), so the Broadway test
  tells the modes apart by the `POPOVER_UNSUPPORTED` notice.
- `test-ui-shell.sh`: the default selftest has no notice, one
  `GtkWindowControls` and one `GtkWindowHandle`; after `config.set
  window.mode popover` (over `socat`, as test-ui-home.sh does) the notice
  shows; `mirqab open` (no `--window`), `toggle`, `toggle` keeps one
  process past the 5 s exit delay. The daemon publishes events, not the
  window's state, so the test reads hide-then-show from the exit timer:
  if the second toggle did not show the window, the process would have
  quit.
- README: the window paragraph, the popover's focus-follows-mouse limit,
  what `toggle` does across workspaces, and the float rule with `float`
  and `center` (both from haseen's working rules, E19). No size line
  (U3). The bind lines stay in the older syntax.
- The D4 fallback (re-map on `show`) is not built; it waits on U1.
- Tests: `cargo test -p mirqab-view --lib` 2/2, `-p mirqab-core --lib
  config` 9/9, `tests/run.sh tests/test-ui-shell.sh` 27/27.

## Unverified

- **U1 — `present()` across workspaces.** GTK only promises "may" (E17).
  What Hyprland does with an activation request that comes from a socket
  message and not from the user's input (focus, urgent mark, or nothing;
  `misc:focus_on_activate`) was not run: `hyprctl` had no session in the
  planning shell.
- **U2 — window controls on Hyprland.** That `gtk::WindowControls` shows a
  close button under libadwaita 1.10 on Hyprland, and that it looks right
  beside the page title, is from the GTK API, not from a run.
- **U3 — the rule.** The `float = true` form is copied from haseen's
  working rules (E19). The keys for size and centring, and the equivalent
  in the older config syntax, were not checked against Hyprland 0.56.
- **U4 — toggle from the tray.** D3 does not depend on focus by design
  (E7); the click path on the owner's bar is still worth one look.
- **A re-shown window lands on the current workspace** (D4, acceptance 3).
  Expected of any compositor mapping a new toplevel; not run.
- **Why the popover closes on hover.** The owner observed it; that the
  path is `follow_mouse` → focus → `is-active` (E10) is read from the code,
  not traced on the compositor.
- **Wayland and position.** That a client is not told its toplevel's
  position is stated from the xdg-shell protocol's design, not quoted; the
  GTK half is quoted (E16).
- **The selftest's config.** That `--selftest` reads the test's config
  file, so the popover case can be driven from test-ui-shell.sh, was not
  read in `selftest.rs`; if it does not, that case moves to the
  `--background` run.
