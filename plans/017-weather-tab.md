# Plan 017: Weather tab

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW technically. Two things to watch: the tab sends
  coordinates to a third party, and the data source's free tier is for
  non-commercial use.
- **Depends on**: 001, 016 (the city table, the sun and moon arithmetic)
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Stat Space's Weather tab shows current conditions for a city, the next 24
hours as a curve with rain marks, a ten-day list with range bars, and sun
and moon; Pro adds more cities, rain notifications and severe-weather
alerts. Decisions (architecture §1.2): more cities free; rain
notifications free where data exists; severe alerts dropped.

## Linux data sources

No root.

| Data | Source |
|------|--------|
| Forecast | Open-Meteo: `https://api.open-meteo.com/v1/forecast` with `latitude`, `longitude`, `current=`, `hourly=`, `daily=`, `timezone=auto`, `forecast_days` (up to 16), `temperature_unit`, `wind_speed_unit`, `precipitation_unit`. No key. (https://open-meteo.com/en/docs, read 2026-11-13) |
| Variables used | current: `temperature_2m, apparent_temperature, relative_humidity_2m, weather_code, wind_speed_10m, wind_direction_10m, precipitation, is_day, surface_pressure`; hourly: `temperature_2m, precipitation_probability, precipitation, weather_code, is_day`; daily: `weather_code, temperature_2m_max, temperature_2m_min, precipitation_probability_max, precipitation_sum, sunrise, sunset, uv_index_max` |
| Condition names and icons | WMO weather codes in `weather_code` (table in the same documentation), mapped to a word and a Nerd-font glyph |
| Rain in the next hours | `minutely_15=precipitation` (15-minute data, available for some regions only; elsewhere it is interpolated from hourly, same documentation) |
| Air quality | `https://air-quality-api.open-meteo.com/v1/air-quality` (`european_aqi`, `us_aqi`, `pm2_5`), optional card, off by default |
| City search | Plan 016's bundled table, then the Open-Meteo geocoding API on request |
| Current location | Not used by default. Optional: GeoClue2 (`org.freedesktop.GeoClue2` **[unverified]** on the bus) at city accuracy, off by default |
| Sun and moon | Computed locally (plan 016) |
| Transport | `curl` through `exec.rs`, with `--config -` on stdin, `--proto =https`, `--max-time 10`, a `User-Agent` naming the app and version |

Terms (https://open-meteo.com/en/terms, read 2026-11-13): the free API is
for non-commercial use, under 10,000 calls a day, data under CC BY 4.0
with attribution. One user's use is a few dozen calls a day. The page
footer carries "Weather data by Open-Meteo.com" with a link.

Decided (owner, 2026-11-13): Open-Meteo is the default provider. Its
base URL is the setting `weather.endpoint` (and `weather.geocodingEndpoint`
for the search), so a self-hosted Open-Meteo or a paid plan's host works
with no code change. `README.md` states the non-commercial terms and
tells a commercial user to set their own endpoint.

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Severe weather alerts | Dropped: no keyless worldwide source. National feeds differ per country |
| Rain notifications | Kept, off by default, and labelled "where 15-minute data exists" |
| Apple's weather source | Replaced by Open-Meteo; numbers will differ |
| Automatic location | Off by default; the user picks a city |

## Design

### Privacy

- Coordinates are rounded to 0.05° (about 5.5 km) before they are sent;
  the city table's own coordinates are already city centres.
- Requests happen only while a consumer is active (the page, a pinned
  card, the bar item, or the rain rule) and at most every 30 minutes per
  city (`weather.refreshMinutes`, minimum 15). Closed and unpinned means
  no request at all.
- The cache holds the last response per city in
  `~/.cache/mirqab/weather/<city-id>.json`. City ids are opaque; the file
  holds the rounded coordinates the user chose.
- Nothing about weather goes into `history.db`.
- The endpoint base URL is a setting (`weather.endpoint`), so a
  self-hosted Open-Meteo works.

### Stream `weather`

Sent on subscribe from the cache at once (with its age), then again when
a fetch completes.

```json
{"cities":[{"id":"c1","name":"Tokyo","country":"JP","fetchedUnix":0,"stale":false,
   "units":{"temp":"C","wind":"km/h","precip":"mm"},
   "now":{"temp":21.5,"feels":22.0,"code":2,"text":"Partly cloudy","isDay":true,
          "humidity":60,"windSpeed":12,"windDir":220,"pressure":1013,"uv":4},
   "hours":[{"unix":0,"temp":21.0,"code":2,"precipProb":10,"precip":0.0,"isDay":true}],
   "days":[{"date":"2026-01-01","code":61,"min":14.0,"max":22.0,"precipProb":70,"precipSum":4.2}],
   "rainSoon":{"startsInMin":25,"mm":0.6,"source":"15min"},
   "sun":{"riseUnix":0,"setUnix":0},"moon":{"phase":0.42}}],
 "error":null,"attribution":"Weather data by Open-Meteo.com"}
```

A failed fetch keeps the cached data, sets `stale`, and puts a short
reason in `error`; it retries on the next interval, not sooner. A 429 or
5xx doubles the interval up to 4 hours.

### Page

From the owner's screenshots of the reference, redrawn, not copied:

1. A city picker across the top (chips; add, remove, reorder).
2. The hero card: a large temperature, the condition word and glyph,
   feels-like, high and low, and a row of small figures (wind, humidity,
   UV, pressure).
3. Next 24 hours: a temperature curve (the shared `Graph` widget, one stroked path) with hour labels,
   condition glyphs along it and precipitation probability as ticks
   under the axis.
4. Ten days: one row per day with the glyph, the precipitation chance,
   and a range bar positioned on the week's overall minimum to maximum.
5. Sun and moon: an arc with the sun's position, rise and set times, day
   length, the moon phase.
6. Optional: air quality.
7. The footer: attribution, the data's age, a Refresh button (limited to
   once a minute).

Units follow `weather.units` (`metric`, `imperial`, or per field);
default from the locale. Pinnable cards: `weather.now`, `weather.hours`.
Bar item `weather` (glyph and temperature). CLI: `mirqab weather [city]
[--json]`.

Rain rule (off by default): when `rainSoon.startsInMin` first drops under
30 for the first city, one notification through
`org.freedesktop.Notifications`, at most once per 3 hours.

Actions: `weather.city.add|remove|reorder`, `weather.refresh`.

### What is adapted

haseen's weather plugin (MIT, itself adapted from Omarchy, MIT) has the
WMO code table and the Open-Meteo request shape. The code table and its
wording may be adapted with a `NOTICE.md` row and the upstream notice in
the file header. The layout is new.

## Rejected

- **wttr.in.** A text front end to other sources, with no service terms
  to rely on.
- **OpenWeatherMap, WeatherAPI, Tomorrow.io.** Each needs a key per
  user.
- **MET Norway (api.met.no).** A good keyless source, but its terms
  require an identifying User-Agent with contact details and it has no
  geocoding or 15-minute data. Kept as the documented alternative if
  Open-Meteo's terms change.
- **An HTTP client crate (`ureq`, `reqwest`).** TLS stacks in the binary;
  `curl` is already a dependency.
- **Location by IP address.** A second third party, and wrong on a VPN.
- **Fetching in the background for a quick first paint.** A request
  nobody asked for; the cache gives the first paint.
- **Weather history in the database.** Not this app's job.

## Tests and fixtures

`tests/fixtures/weather/`: synthetic responses (`forecast.json`,
`forecast-imperial.json`, `forecast-no-minutely.json`, `error-429.json`,
`truncated.json`) for a made-up point at 0.00, 0.00. A stub `curl` that
records its argv and stdin and prints a chosen fixture.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/weather.rs` | Response to stream shape; every WMO code has a word and a glyph; units; `rainSoon` from 15-minute data and its absence |
| same | **Privacy**: coordinates in the recorded request are rounded to 0.05°; no request with no consumer; no second request inside the interval; the URL host is the configured endpoint only |
| same | 429 backs off; a truncated body keeps the cache and sets `stale`; the cache is served with its age on subscribe |
| same | `curl` argv is fixed, the URL goes in on stdin, and `--proto =https` is present |
| `crates/mirqab-view/tests/weather.rs` | curve points, range-bar positions, hour labels across midnight |
| `tests/test-ui-weather.sh` (`mirqab-ui --selftest weather`, plan 001) | The page from a fixture; the attribution is visible; the stale state |
| `tests/test-cli.sh` | `weather --json` from the cache with the daemon offline from the network |

## Acceptance

1. A chosen city shows current conditions, 24 hours and ten days that
   agree with open-meteo.com's own page for that place.
2. Three cities work, with one request each per interval.
3. With the page closed and nothing pinned, `mirqab status` shows no
   weather request for an hour.
4. Offline, the cached forecast shows with its age and a stale mark.
5. The attribution is on the page.

## Checklist

1. Resolve Unverified.
2. `crates/mirqab-core/src/http.rs` if plan 011 has not made it (the one
   `curl` wrapper).
3. `crates/mirqab-core/src/samplers/weather/{mod,openmeteo,codes,cache,rain}.rs`.
4. `tests/golden/weather.json`; fixtures; tests.
5. `crates/mirqab-view/src/tabs/weather.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/weather/{mod,page,city_chips,hero_card,hours_card,days_card,day_row,sun_moon_card,air_card}.rs`.
6. CLI `weather`; bar item; the rain rule.
7. `NOTICE.md`: the code table's origin; Open-Meteo attribution in
   `README.md` and the About text.
8. Architecture §10: confirm the request table still matches.

## Unverified

- That a self-hosted Open-Meteo answers the same paths and parameters
  under another base URL, so `weather.endpoint` is enough
  (https://github.com/open-meteo/open-meteo, read its API notes).
- `minutely_15` coverage and its exact response keys: fetch one sample
  for a covered and an uncovered place and record them as fixtures
  (scrubbed to the made-up point).
- GeoClue2's presence and its agent requirement outside GNOME; if it
  needs a desktop agent the app cannot be, drop the option.
- `uv_index_max` daily against a current UV value: pick whichever the
  API returns for `current`.
