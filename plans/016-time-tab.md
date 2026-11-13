# Plan 016: Time tab

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: LOW. Arithmetic over the system's time zone database; one
  optional request (city search).
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Stat Space's Time tab is a world clock: a dotted world map with the
user's cities, each city's time and offset, a scrubber to slide the whole
set forward and back, working-hours shading, and sunrise, sunset and moon
for a city. The owner asked for "world clocks, scrubber". The Team
planner and "Today's meetings" are dropped (architecture §1.2).

## Linux data sources

No root.

| Data | Source |
|------|--------|
| Time zone rules | `/usr/share/zoneinfo` (package `tzdata`), read by the `jiff` crate (0.2.43) through its system-database support. `tzdata.zi` is present **[machine]** |
| The list of zones with coordinates and country | `/usr/share/zoneinfo/zone1970.tab`: 312 zones, each with ISO 6709 coordinates and country codes **[machine]** |
| Country names | `/usr/share/zoneinfo/iso3166.tab` |
| The local zone | `/etc/localtime` symlink target; D-Bus `org.freedesktop.timedate1` `Timezone` and its `PropertiesChanged` for changes |
| More cities than `zone1970.tab` has | A bundled table `share/mirqab/cities.json`, built by a script from GeoNames `cities15000` (CC BY 4.0, https://download.geonames.org/export/dump/readme.txt): name, country, latitude, longitude, zone. About 25,000 rows, under 1 MB gzipped **[unverified]** |
| City search fallback | `https://geocoding-api.open-meteo.com/v1/search?name=…&count=5` returns `name`, `latitude`, `longitude`, `timezone`, `country` (https://open-meteo.com/en/docs/geocoding-api), used only when the bundled table has no match and the user presses "Search online" |
| Sunrise, sunset, twilight | Computed locally: the NOAA solar equations (https://gml.noaa.gov/grad/solcalc/calcdetails.html), accurate to about a minute outside the polar regions |
| Moon phase | Computed locally: days since a known new moon (Julian day 2451550.1) modulo the synodic month (29.53058867 days), as haseen's `Moon.js` does |
| Land outline for the map | Natural Earth 1:110m land polygons (public domain, https://www.naturalearthdata.com/about/terms-of-use/), rasterised once by a tools script into a 144 × 72 bit mask |
| Clock format | `config.json` `time.hour12`; default from the locale (`LC_TIME`) |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Team planner | Dropped: the owner's instruction |
| Today's meetings | Dropped: needs calendar accounts |
| Menu bar clock looks | One bar item, `clock`, with a format string. The twelve looks are dropped |
| Nothing else | The tab is pure computation |

## Design

### Where the work happens

Zone arithmetic is in the daemon (`jiff`), because the CLI, the bar item
and MCP need it too, and so that one implementation serves them all.
The interface never computes an offset.

### Stream `time`

Not a timer in the daemon. On subscribe and on any change (city list,
zone change, a DST transition inside the window, scrub position), the
daemon sends, per city, the offsets and transitions covering the
scrubber's range. The interface then draws the ticking clock from its own
one-minute GLib timeout (`glib::timeout_add_seconds_local`, marked
`// mirqab:sample`, removed when the page is unmapped) plus those offsets.
Seconds are not shown: the 1 s interval the owner allowed (2026-11-13) is
for a live graph on screen, and a clock is not one.

```json
{"local":{"zone":"Europe/London","label":"Home"},
 "cities":[{"id":"c1","name":"Tokyo","country":"JP","zone":"Asia/Tokyo",
            "lat":35.7,"lon":139.7,
            "offsets":[{"fromUnix":0,"utcOffsetSec":32400,"abbr":"JST"}],
            "work":{"start":"09:00","end":"18:00","days":[1,2,3,4,5]},
            "sun":{"riseUnix":0,"setUnix":0,"dawnUnix":0,"duskUnix":0,"dayLengthSec":0},
            "moon":{"phase":0.42,"name":"waxing gibbous","illumination":0.8}}],
 "range":{"fromUnix":0,"toUnix":0}}
```

City coordinates are city-level (one decimal place) and are the user's
own choice; nothing here is sent anywhere. The seed list for a new
install is the local zone only.

### Page

1. The map: a custom widget (`world_map.rs`). Land is a grid of dots
   drawn as **one** filled `gsk::Path` holding every dot, built once per
   size and kept; a marker per city; the night side drawn as a second
   filled path from the solar terminator at the scrubbed time
   (architecture §7.4). No image, no blur. Dot positions and the
   terminator polygon come from `mirqab_view::tabs::time`.
2. The city list: name, time, day offset ("tomorrow", "yesterday"),
   UTC offset and its difference from local, a state word (working,
   evening, asleep, weekend) from the city's working hours, and a
   24-cell strip shaded by that state.
3. The scrubber: drag from −12 h to +36 h in 15-minute steps; every
   clock, strip and the terminator follow. A "Now" button returns. While
   scrubbed, a label states the local time being shown.
4. "Best time to talk": the overlap of working hours across the ticked
   cities, as a highlighted span on the strips. This is the free, local
   remainder of the Team planner: no people, no accounts.
5. A detail card for the selected city: sunrise, sunset, day length,
   golden and blue hours, moon phase drawn as two arcs in one path.
6. Add city: a search field over the bundled table, with "Search online"
   as an explicit fallback. Reorder by drag; remove; rename the label;
   set working hours.

Pinnable cards: `time.clocks` (the list, compact), `time.sun`. Bar item
`clock` (`config.json` `bar.clock.format`, and optionally a second
city). CLI: `mirqab time [--json]`, `mirqab time <city>`,
`mirqab time convert "15:00 Tokyo" <city>`.

Actions: `time.city.add`, `time.city.remove`, `time.city.update`,
`time.city.reorder`. All reversible; no confirmation.

## Rejected

- **`chrono-tz`.** It compiles the whole database into the binary, which
  then goes stale until a rebuild. `jiff` reads the system's.
- **Zone math in the interface (`glib::TimeZone`).** The CLI, the bar
  item and MCP could not share it.
- **A map image or map tiles.** A bitmap cannot take theme colours; tiles
  need a network.
- **A widget per dot.** About 3,000 widgets for a picture; one path node
  is the same picture.
- **`GtkDrawingArea` (Cairo) for the map.** Software drawing of the whole
  widget (architecture §7.4).
- **A seconds hand.** A 1 s timer for something that is not a graph.
- **Geolocation to seed the list.** The local zone is enough and leaks
  nothing.
- **Shipping GeoNames whole.** `cities15000` is the smallest set that
  covers what people type.

## Tests and fixtures

`tests/fixtures/zoneinfo/`: a small tree with six zones compiled from
hand-written rules by `zic` at test time (a northern DST zone, a southern
DST zone, a half-hour offset, a zone with no DST, one that crosses the
date line, UTC), plus matching `zone1970.tab` and `iso3166.tab`. The
clock is injected everywhere.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/time.rs` | Offsets and abbreviations; a DST transition inside the scrub range produces two offset rows; day offset words across the date line; the local zone follows `etc/localtime` |
| same | Working-state for each city at fixed instants, including a weekend that differs by zone; the overlap span, and no overlap |
| `crates/mirqab-core/tests/sun.rs` | Sunrise and sunset for five published city/date pairs within 2 minutes (values from the NOAA calculator, cited in the test); polar day and night return `null`, not a wrong time |
| `crates/mirqab-core/tests/moon.rs` | Eight published new and full moon dates within 12 hours |
| `crates/mirqab-core/tests/cities.rs` | Search ranks an exact name first; handles diacritics; the table's zones all exist in the fixture list |
| `crates/mirqab-view/tests/time.rs` | strip shading, the scrubber's snap, terminator points |
| `tests/test-ui-time.sh` (`mirqab-ui --selftest time`, plan 001) | The page loads; scrubbing changes every row. A widget test checks the map is at most three render nodes whatever the dot count, and lint finds no timeout under 60 s in `tabs/time/` |
| `tools/secrets.sh` | No coordinates with more than two decimals in fixtures or defaults |

## Acceptance

1. Adding three cities shows correct times, checked against `TZ=… date`.
2. Scrubbing across a DST change shows the hour shift at the right
   instant.
3. Sunrise and sunset for the user's city are within two minutes of a
   published table.
4. With the page open, the daemon's `time` sampler shows no periodic
   work in `mirqab status`.

## Checklist

1. Resolve Unverified; add `jiff` with the line plan 001 requires (what
   it replaces, binary size cost).
2. `tools/mkcities.sh` and `tools/mklandmask.sh` (build-time data
   scripts; inputs downloaded by the maintainer, outputs committed with
   their source and licence recorded in `NOTICE.md`).
3. `share/mirqab/{cities.json,landmask.json}`.
4. `crates/mirqab-core/src/samplers/time/{mod,zones,cities,work}.rs`;
   `crates/mirqab-core/src/astro/{sun,moon}.rs`.
5. `tests/golden/time.json`; the tests above.
6. `crates/mirqab-view/src/tabs/time.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/time/{mod,page,world_map,city_row,scrubber,hour_strip,sun_card,moon_shape,add_city}.rs`.
7. CLI `time`; bar item `clock`.
8. `NOTICE.md` rows: GeoNames (CC BY 4.0), Natural Earth (public
   domain), the moon arithmetic adapted from haseen (MIT).

## Unverified

- `jiff`'s API for listing a zone's transitions in a range, and its
  binary size cost; fall back to probing offsets at 15-minute steps if
  there is no such call.
- The size of the GeoNames extract after trimming to five fields; if it
  is over 1 MB compressed, raise the population cut-off.
- GeoNames attribution wording required by CC BY 4.0 in an About page.
- Whether one path of about 3,000 circles (a 144 × 72 grid) builds and
  draws fast enough with the default renderer and with
  `GSK_RENDERER=cairo`: time `snapshot()` once at the popover's size; if
  it is over 4 ms, halve the grid or use squares.
