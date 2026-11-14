//! Glyph names (architecture §7.2). Icons are Material Design glyphs from
//! Symbols Nerd Font (`ttf-nerd-fonts-symbols`, 3.5.1 on the development
//! machine), shown in a label with the `icon` style class. Every glyph the
//! interface uses is named here and nowhere else. The code points were read
//! from the font's own glyph names (`md-<name>`) with fontTools.

/// A glyph: its Nerd Font name and its character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub name: &'static str,
    pub ch: char,
}

const fn g(name: &'static str, ch: char) -> Glyph {
    Glyph { name, ch }
}

pub const HOME: Glyph = g("md-home", '\u{f02dc}');
pub const APPS: Glyph = g("md-apps", '\u{f003b}');
pub const SYSTEM: Glyph = g("md-chip", '\u{f061a}');
pub const FANS: Glyph = g("md-fan", '\u{f0210}');
pub const DISK: Glyph = g("md-harddisk", '\u{f02ca}');
pub const NETWORK: Glyph = g("md-access_point_network", '\u{f0002}');
pub const DEVICES: Glyph = g("md-usb_port", '\u{f11f0}');
pub const BATTERY: Glyph = g("md-battery_high", '\u{f12a3}');
pub const VOLUME: Glyph = g("md-volume_high", '\u{f057e}');
pub const PORTS: Glyph = g("md-lan_connect", '\u{f0318}');
pub const AGENTS: Glyph = g("md-robot_outline", '\u{f167a}');
pub const AWAKE: Glyph = g("md-coffee_outline", '\u{f06ca}');
pub const TIME: Glyph = g("md-clock_outline", '\u{f0150}');
pub const WEATHER: Glyph = g("md-weather_partly_cloudy", '\u{f0595}');
pub const TRENDS: Glyph = g("md-chart_line", '\u{f012a}');
pub const SUPPORT: Glyph = g("md-lifebuoy", '\u{f087e}');
pub const SETTINGS: Glyph = g("md-cog_outline", '\u{f08bb}');
pub const PIN: Glyph = g("md-pin", '\u{f0403}');
pub const PIN_OUTLINE: Glyph = g("md-pin_outline", '\u{f0931}');
pub const DRAG: Glyph = g("md-drag_vertical", '\u{f01dd}');
pub const MENU: Glyph = g("md-dots_vertical", '\u{f01d9}');
pub const CLOSE: Glyph = g("md-close", '\u{f0156}');
pub const HEALTH: Glyph = g("md-heart_pulse", '\u{f05f6}');
pub const WARNING: Glyph = g("md-alert_circle_outline", '\u{f05d6}');
pub const OK: Glyph = g("md-check_circle_outline", '\u{f05e1}');
pub const EXPAND: Glyph = g("md-chevron_right", '\u{f0142}');
pub const COLLAPSE: Glyph = g("md-chevron_down", '\u{f0140}');
pub const REFRESH: Glyph = g("md-refresh", '\u{f0450}');
// An Apps row's kind when its app has no icon in the theme (plan 006).
pub const KIND_APP: Glyph = g("md-application_outline", '\u{f0614}');
pub const KIND_SERVICE: Glyph = g("md-cog", '\u{f0493}');
pub const KIND_TERMINAL: Glyph = g("md-console", '\u{f018d}');
pub const KIND_CONTAINER: Glyph = g("md-docker", '\u{f0868}');
pub const KIND_FLATPAK: Glyph = g("md-package_variant", '\u{f03d6}');
pub const KIND_PROCESS: Glyph = g("md-cube_outline", '\u{f01a7}');
pub const KIND_SYSTEM: Glyph = g("md-account_cog", '\u{f1370}');

/// Every glyph above, for tests and the selftest's widget dump.
pub const ALL: &[Glyph] = &[
    HOME,
    APPS,
    SYSTEM,
    FANS,
    DISK,
    NETWORK,
    DEVICES,
    BATTERY,
    VOLUME,
    PORTS,
    AGENTS,
    AWAKE,
    TIME,
    WEATHER,
    TRENDS,
    SUPPORT,
    SETTINGS,
    PIN,
    PIN_OUTLINE,
    DRAG,
    MENU,
    CLOSE,
    HEALTH,
    WARNING,
    OK,
    EXPAND,
    COLLAPSE,
    REFRESH,
    KIND_APP,
    KIND_SERVICE,
    KIND_TERMINAL,
    KIND_CONTAINER,
    KIND_FLATPAK,
    KIND_PROCESS,
    KIND_SYSTEM,
];
