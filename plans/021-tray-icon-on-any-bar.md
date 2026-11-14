# 021 — Tray icon on any bar

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW. One new drawing, one string, two test files and the README.
  The risk is in the drawing: it has to read at 16 px on a dark bar and on a
  light one, and only the owner's bar can show that.
- **Depends on**: 004
- **Category**: integration
- **Planned at**: 2026-11-14, from the owner
- **State**: DONE (2026-11-14). The owner's check (acceptance 1–3) stays
  open.

## Problem

Plan 004's acceptance 1 (the icon in the desktop's tray) failed the owner's
check on 2026-11-14 (Hyprland, haseen's Quickshell bar, mirqab `8dbc180`
installed at user scope). The item registers and a left click toggles the
window, but the bar draws Quickshell's missing-icon image. Two separate
causes:

1. **The bar had started before the icon was installed.** The running shell
   did not find `mirqab-symbolic`; a fresh `qs` process on the same machine
   did. This is not a mirqab bug, but nothing in the README says so.
2. **The icon is one dark colour and the bar does not recolour it.** Once
   found, `#2e3436` on a dark bar is close to invisible. Plan 004 flagged
   this (plans/004-tray-and-bar-items.md:276-282).

The owner decided (2026-11-14): the tray uses a **two-tone, full-colour
icon** — a light watchtower with a dark outline, readable on dark and light
bars. The symbolic icon stays.

## Evidence

| # | Fact | Source |
|---|------|--------|
| E1 | The tray sends only `IconName`, the value of `strings::TRAY_ICON`; it overrides neither `icon_theme_path` nor `icon_pixmap`. | crates/mirqab/src/tray/item.rs:85; crates/mirqab-proto/src/strings.rs:1036-1037 (`"mirqab-symbolic"`) |
| E2 | Nothing else in the tree uses the name: `git grep -n -E 'mirqab-symbolic\|TRAY_ICON' origin/main -- . ':!plans'` prints README.md:260, strings.rs:1037, item.rs:85, tests/test-units.sh:50 and :52. The GTK interface does not load it. | command output, 2026-11-14 |
| E3 | The symbolic icon is four paths filled `#2e3436` on a 16 × 16 view box. | dist/icons/mirqab-symbolic.svg |
| E4 | Quickshell uses `IconName` whenever it is not empty (and there is no overlay name) and passes `IconThemePath` beside it; it uses `IconPixmap` only when the name is empty. | quickshell `src/services/status_notifier/item.cpp:96-109` (master, read 2026-11-14) |
| E5 | Quickshell resolves the name with `QIcon::fromTheme`. With a path it then tries `QPixmap("/<path>/<name>")` — the name as given, no extension added. If both fail it draws a black image with two purple (`0xd900d8`) quarters. | quickshell `src/core/iconimageprovider.cpp:18-44`, `:50-66` |
| E6 | A bar started before the icon was copied does not find it; a fresh `qs` process (`QT_QPA_PLATFORMTHEME=gtk3`) resolves `Quickshell.iconPath("mirqab-symbolic", true)` to `image://icon/mirqab-symbolic`. `~/.local/share/icons/hicolor` existed already; `symbolic/apps` was created by the copy; there is no `icon-theme.cache`. | owner's check, 2026-11-14 **[machine]** (quickshell 0.3.2.r7, qt6-base 6.12.0) |
| E7 | haseen's bar draws the tray icon in a plain `Image`, with no recolouring. | haseen `share/haseen/shell/plugins/haseen.tray/TrayIcon.qml:3-4`, `:9-17` |
| E8 | ksni offers `icon_theme_path`, `icon_name` and `icon_pixmap`; a pixmap is `width`, `height` and ARGB32 bytes in network byte order. | ksni 0.3.7 `src/lib.rs:151`, `:162`, `:167`; `src/tray.rs:151` |
| E9 | The daemon is a headless crate: it may not name the toolkit, gdk-pixbuf or cairo. It depends on mirqab-core, mirqab-view, serde_json and ksni only. | tools/lint.sh:23, :26, :84-94; crates/mirqab/Cargo.toml |
| E10 | The tray already cost about 437 KB of binary; the idle measurement with the tray is 0.000 % CPU and 6.9 MiB. | plans/004-tray-and-bar-items.md:240-251 |
| E11 | The desktop entry names `Icon=io.github.t1nk333r.mirqab`, and no file of that name exists; `dist/icons/` holds only the symbolic icon. A test pins the line. | dist/mirqab.desktop; tests/test-units.sh:45 |
| E12 | Plan 020 already installs the application icon as `/usr/share/icons/hicolor/scalable/apps/mirqab.svg`. | plans/020-packaging-and-release.md:55 |
| E13 | The README tells the user to copy only the symbolic icon. | README.md:256-261 |

## Design

### D1. One new icon: `dist/icons/mirqab.svg`

- Name `mirqab`, installed as `hicolor/scalable/apps/mirqab.svg` — the name
  and path plan 020 already uses (E12).
- The drawing is this project's own watchtower (roof, cabin with two
  windows, legs with a brace, base), redrawn from the symbolic icon's shapes
  (E3). It is original work: no shape is taken from Stat Space or from any
  icon set.
- Two tones: a light fill and a dark outline about one sixteenth of the
  icon wide. Every light shape sits on a slightly larger dark copy of
  itself, so the outline is the contrast on a light bar and the fill is the
  contrast on a dark one. The cabin windows are dark.
- 16 × 16 view box, drawn on the pixel grid, because a tray shows it at
  16–24 px. It is a vector, so the launcher's larger sizes need no second
  file.
- Plain SVG only: `path`/`rect` with `fill`. No `paint-order`, filter,
  mask, clip path, gradient, CSS or `currentColor`. Qt's SVG renderer and
  librsvg both draw this subset; whether Qt draws the richer features the
  same way is not checked, so the icon does not use them.
- The two colours are written in the file. The colour-literal lint rule
  covers the interface crates and `style.css`, not `dist/` (tools/lint.sh:102-104).

### D2. The tray sends `IconName = "mirqab"` and nothing else

`strings::TRAY_ICON` becomes `"mirqab"`. `IconThemePath` and `IconPixmap`
stay empty. See Rejected for why the other two properties do not earn their
place.

### D3. The symbolic icon stays, and stays installed

`dist/icons/mirqab-symbolic.svg` is unchanged and is still installed to
`hicolor/symbolic/apps/`. After this plan nothing in mirqab asks for it by
name (E2); it is there for GTK and for desktops that ask for
`<icon>-symbolic` and recolour it. Its header comment says so.

### D4. The desktop entry uses the same icon

`dist/mirqab.desktop` gets `Icon=mirqab`. Today it names an icon that does
not exist (E11), so launchers show nothing. One drawing under one name is
the smallest fix and agrees with plan 020 (E12). `StartupWMClass` stays the
application id. Plan 001's as-built note (plans/001-foundation.md:748-750,
"icon name equal to app id") gets one line pointing here.

### D5. README

Replace README.md:256-261 with both copies and the restart note:

```sh
install -Dm644 dist/icons/mirqab.svg ~/.local/share/icons/hicolor/scalable/apps/mirqab.svg
install -Dm644 dist/icons/mirqab-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/mirqab-symbolic.svg
```

and one sentence: a bar that was already running when the icon was
installed keeps showing a placeholder until the bar is restarted, because
bars look icons up once per process (E6). No string changes besides
`TRAY_ICON`.

## Rejected

| Option | Why not |
|--------|---------|
| Send `IconPixmap` beside `IconName`, so a bar that cannot find the name still has pixels | Quickshell never falls back: a non-empty name wins even when it does not resolve (E4, E5). It would not have changed the owner's result. Other hosts were not checked. |
| Send `IconPixmap` only (no name), so no install and no bar restart is needed | The daemon cannot rasterise an SVG: no toolkit, no image crates (E9). Pre-rendered ARGB tables for 16–64 px are about 34 KB in the binary and on the bus, need a build step or committed binary data, and take the icon out of the icon theme (no per-theme override, sizes fixed). It trades a one-time bar restart for permanent cost. Revisit only if the owner finds the restart unacceptable. |
| Send `IconThemePath` pointing at the installed icon directory | On Quickshell the path is only used as `/<path>/mirqab`, without `.svg` (E5), so it finds nothing. The daemon would also have to know its install prefix. What other hosts do with it was not checked. |
| Keep the symbolic icon and recolour it in haseen's bar | The bar renders in software and does not recolour on purpose (E7); and it would fix one bar only. |
| A light-only icon | Invisible on a light bar. The owner asked for both. |
| Install the icon under the application id as well | Two names for one file. One name is enough once the desktop entry uses it (D4). |
| Refresh an icon cache or signal the bar from mirqab | There is no cache on the owner's machine (E6), and a bar's per-process lookup state is not mirqab's to reset. |

## Files

| File | Change |
|------|--------|
| `dist/icons/mirqab.svg` | new (D1) |
| `dist/icons/mirqab-symbolic.svg` | header comment only (D3) |
| `dist/mirqab.desktop` | `Icon=mirqab` (D4) |
| `crates/mirqab-proto/src/strings.rs` | `TRAY_ICON = "mirqab"`, doc comment |
| `crates/mirqab/tests/tray.rs` | icon properties (Tests) |
| `tests/test-units.sh` | icon and desktop assertions (Tests) |
| `README.md` | install lines, restart note (D5) |
| `plans/004-…md`, `plans/001-…md`, `plans/020-…md` | one as-built line each pointing here; 004's State drops the icon check when this plan's acceptance passes |
| `plans/README.md` | row 021 |

`crates/mirqab/src/tray/item.rs` does not change: it already sends
`strings::TRAY_ICON`.

## Tests and fixtures

Affected scope only: `crates/mirqab/tests/tray.rs` and `tests/test-units.sh`
(`# mirqab:covers dist/**`). No new fixture.

- **tray.rs** (private `dbus-daemon`, as today): after registration,
  `IconName` equals `strings::TRAY_ICON` and is `"mirqab"`; `IconThemePath`
  is empty; `IconPixmap` is an empty array. These go in the existing
  `activate_toggles_and_the_tooltip_leaves_no_subscription` test beside the
  `Category` read, so no second bus is started.
- **test-units.sh**: `dist/icons/mirqab.svg` exists, has
  `viewBox="0 0 16 16"`, uses exactly two distinct `fill` colours, and has
  none of `paint-order`, `filter`, `mask`, `clipPath`, `Gradient`, `<style`,
  `currentColor`; the symbolic icon's checks stay; strings.rs holds
  `TRAY_ICON: &str = "mirqab"` once; the desktop entry holds `Icon=mirqab`
  (replacing line 45).
- Not tested headless: how the icon looks. That is the owner's check.

## Acceptance

1. With both icons installed and the bar restarted, the owner's haseen bar
   shows the watchtower, readable on the dark bar. (Closes plan 004
   acceptance 1.)
2. The same icon is readable on a light bar or a light theme.
3. A launcher shows the icon for the mirqab entry.
4. `tools/lint.sh`, the two affected tests, `tools/check-docs.sh` and
   `tools/secrets.sh` pass. The daemon's dependency list and its stripped
   size are unchanged apart from the string.

## Checklist

- [x] U1: render the draft icon at 16, 22 and 24 px on a dark and a light
      background with a fresh `qs` process (and once with librsvg); adjust
      the outline width before anything else is touched.
- [ ] U2: check that Qt draws the dark-copy-under-light-shape outline
      without gaps at 16 px at scale 1 and 1.25.
- [x] Draw `dist/icons/mirqab.svg`; update the symbolic icon's comment.
- [x] `TRAY_ICON`, the desktop entry, the two tests.
- [x] README; the as-built lines in plans 001, 004, 020; row in plans/README.md.
- [ ] Gates; owner's check (acceptance 1–3).

## As built

- `dist/icons/mirqab.svg`: dark `#2e3436` copies under light `#eeeeec`
  shapes, the outline about one unit wide on the 16-unit grid. The cabin
  windows are holes in the light cabin, so the dark copy shows through.
- U1 with librsvg (`rsvg-convert` at 16, 22, 24 and 48 px on `#161616` and
  `#f2f2f2`): every size reads on both backgrounds. On the dark one the
  light fill carries the shape and the outline merges with the bar; at 22
  and 24 px the grid does not land on pixels and the edges soften, but it
  stays legible. U2 (Qt at scale 1.25) is left to the owner's check on the
  installed icon.
- `tests/test-units.sh` also refuses `stroke`, so the outline stays filled
  shapes. `tray.rs` reads `IconName`, `IconThemePath` and `IconPixmap` in
  the existing activate test.
- Tests: `cargo test -p mirqab --test tray` 5/5; `tests/run.sh
  tests/test-units.sh` 17/17.

## Unverified

- **U2 — Qt at a fractional scale.** The drawing was rendered with librsvg
  only (As built). That Qt's SVG renderer draws it cleanly on the owner's
  bar at scale 1.25 is the owner's check.
- **Other tray hosts.** Only Quickshell's source was read (E4, E5). What
  Waybar, KDE or GNOME's extension do with `IconThemePath` or with a
  pixmap beside a name is not known, so this plan claims nothing for them.
- **Why the running bar missed the icon.** E6 is an observation. Whether Qt
  caches the theme's directory list, or a failed lookup, was not traced in
  Qt's source; the README states only the observed behaviour.
- **The StatusNotifierItem text.** The freedesktop page returned 403 on
  2026-11-14; the property meanings here come from ksni's documentation
  (E8) and Quickshell's code.
- **Launchers and `Icon=mirqab`.** That the owner's launcher finds a
  `scalable/apps` icon installed at user scope is expected, not checked
  (acceptance 3).
