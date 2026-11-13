//! The token table: every colour, radius, spacing and font role, with its
//! default and its CSS variable (architecture §7.3). This is the only Rust
//! file with a colour literal. The interface turns a [`Theme`] into the small
//! stylesheet that sets the variables, and custom-drawn widgets read colours
//! from the same table, because CSS cannot colour a path.
//!
//! Precedence, lowest first: the built-in defaults for the scheme, then the
//! desktop token file named by `theme.follow`, then the user's own
//! `~/.config/mirqab/theme.json`. Override values are checked against the
//! token's kind and written back in a canonical form, so a value cannot
//! carry CSS of its own (`red; } window { …`).

use mirqab_proto::strings as s;
use serde_json::Value;
use std::collections::BTreeMap;

/// Dark or light defaults. `theme.scheme` in config.json picks, and
/// `"system"` follows the desktop's preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, or `rgb()`/`rgba()` with numbers.
    Color,
    /// A number with `em` or `px`.
    Length,
    /// A font family name: letters, digits, spaces, `.`, `_`, `-`.
    Font,
}

pub struct Token {
    /// The CSS variable without its leading `--`; also the key in theme.json.
    pub name: &'static str,
    pub kind: Kind,
    pub dark: &'static str,
    pub light: &'static str,
}

const fn color(name: &'static str, dark: &'static str, light: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Color,
        dark,
        light,
    }
}

const fn length(name: &'static str, v: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Length,
        dark: v,
        light: v,
    }
}

const fn font(name: &'static str, v: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Font,
        dark: v,
        light: v,
    }
}

/// Every token. The toolkit's named colours come first (we recolour its
/// style, we do not replace it), then the app's own.
pub const TOKENS: &[Token] = &[
    // The named colours of the toolkit's style (architecture §7.3).
    color("window-bg-color", "#1d1d20", "#fafafb"),
    color("window-fg-color", "#f2f2f4", "#1d1d20"),
    color("view-bg-color", "#1d1d20", "#ffffff"),
    color("view-fg-color", "#f2f2f4", "#1d1d20"),
    color("headerbar-bg-color", "#1d1d20", "#fafafb"),
    color("headerbar-fg-color", "#f2f2f4", "#1d1d20"),
    color("sidebar-bg-color", "#232327", "#ebebed"),
    color("sidebar-fg-color", "#f2f2f4", "#1d1d20"),
    color("card-bg-color", "#2a2a2e", "#ffffff"),
    color("card-fg-color", "#f2f2f4", "#1d1d20"),
    color("popover-bg-color", "#2a2a2e", "#ffffff"),
    color("popover-fg-color", "#f2f2f4", "#1d1d20"),
    color("dialog-bg-color", "#2a2a2e", "#ffffff"),
    color("dialog-fg-color", "#f2f2f4", "#1d1d20"),
    color("accent-bg-color", "#e66b1f", "#e66b1f"),
    color("accent-fg-color", "#ffffff", "#ffffff"),
    color("accent-color", "#ff8a3d", "#b84d00"),
    color("success-color", "#57c87a", "#1f8a44"),
    color("warning-color", "#f5c04a", "#9c6a00"),
    color("error-color", "#ff6b6b", "#c01c28"),
    // The app's own colours.
    color("mirqab-series-1", "#ff8a3d", "#d35f00"),
    color("mirqab-series-2", "#5aa9ff", "#1c6fd1"),
    color("mirqab-series-3", "#57c87a", "#1f8a44"),
    color("mirqab-series-4", "#b78cff", "#7a4fd0"),
    color("mirqab-series-5", "#f5c04a", "#9c6a00"),
    color("mirqab-series-6", "#ff6fa8", "#c2306e"),
    color("mirqab-grid", "#ffffff1a", "#0000001a"),
    color("mirqab-meter-track", "#ffffff14", "#00000014"),
    color("mirqab-map-land", "#ffffff33", "#00000026"),
    color("mirqab-map-night", "#00000066", "#0000004d"),
    // Radii and spacing, in em of the base font (architecture §7.3).
    length("mirqab-radius-card", "0.75em"),
    length("mirqab-radius-tile", "0.6em"),
    length("mirqab-space-1", "0.25em"),
    length("mirqab-space-2", "0.5em"),
    length("mirqab-space-3", "1em"),
    length("mirqab-sidebar-width", "5.5em"),
    // Font roles.
    font("mirqab-font-icon", "Symbols Nerd Font"),
];

/// The number of `mirqab-series-N` colours.
pub const SERIES: usize = 6;

/// How haseen's shell token file (`share/haseen/themed/shell.json.tpl`)
/// maps onto ours: (its key, our tokens). Keys it lacks are left alone.
const FOLLOW_HASEEN: &[(&str, &[&str])] = &[
    (
        "background",
        &[
            "window-bg-color",
            "view-bg-color",
            "headerbar-bg-color",
            "sidebar-bg-color",
        ],
    ),
    (
        "surfaceAlt",
        &["card-bg-color", "popover-bg-color", "dialog-bg-color"],
    ),
    (
        "foreground",
        &[
            "window-fg-color",
            "view-fg-color",
            "headerbar-fg-color",
            "sidebar-fg-color",
            "card-fg-color",
            "popover-fg-color",
            "dialog-fg-color",
        ],
    ),
    (
        "accent",
        &["accent-bg-color", "accent-color", "mirqab-series-1"],
    ),
    ("accentFg", &["accent-fg-color"]),
    ("urgent", &["error-color"]),
    ("warning", &["warning-color"]),
    ("success", &["success-color"]),
];

/// A colour with components from 0 to 1, for drawing code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    /// Parses the colour forms [`Kind::Color`] accepts.
    pub fn parse(v: &str) -> Option<Rgba> {
        let v = v.trim();
        if let Some(hex) = v.strip_prefix('#') {
            return parse_hex(hex);
        }
        let (args, alpha) = if let Some(rest) = v.strip_prefix("rgba(") {
            (rest.strip_suffix(')')?, true)
        } else {
            (v.strip_prefix("rgb(")?.strip_suffix(')')?, false)
        };
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        if parts.len() != if alpha { 4 } else { 3 } {
            return None;
        }
        let channel = |p: &str| -> Option<f32> {
            let n: f32 = p.parse().ok()?;
            (0.0..=255.0).contains(&n).then_some(n / 255.0)
        };
        let a = if alpha {
            let n: f32 = parts[3].parse().ok()?;
            if !(0.0..=1.0).contains(&n) {
                return None;
            }
            n
        } else {
            1.0
        };
        Some(Rgba {
            r: channel(parts[0])?,
            g: channel(parts[1])?,
            b: channel(parts[2])?,
            a,
        })
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn to_hex(self) -> String {
        let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        if c(self.a) == 255 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                c(self.r),
                c(self.g),
                c(self.b),
                c(self.a)
            )
        }
    }
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize, w: usize| u8::from_str_radix(&hex[i..i + w], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => {
            let d = |i| digit(i, 1).map(|x| x * 17);
            (
                d(0)?,
                d(1)?,
                d(2)?,
                if hex.len() == 4 { d(3)? } else { 255 },
            )
        }
        6 | 8 => {
            let d = |i| digit(i, 2);
            (
                d(0)?,
                d(2)?,
                d(4)?,
                if hex.len() == 8 { d(6)? } else { 255 },
            )
        }
        _ => return None,
    };
    let f = |x: u8| f32::from(x) / 255.0;
    Some(Rgba {
        r: f(r),
        g: f(g),
        b: f(b),
        a: f(a),
    })
}

/// A value in its canonical CSS form, or None when it is not a valid value
/// of `kind`.
pub fn canonical(kind: Kind, v: &str) -> Option<String> {
    match kind {
        Kind::Color => Rgba::parse(v).map(Rgba::to_hex),
        Kind::Length => {
            let v = v.trim();
            let (num, unit) = if let Some(n) = v.strip_suffix("em") {
                (n, "em")
            } else {
                (v.strip_suffix("px")?, "px")
            };
            let n: f64 = num.parse().ok()?;
            (n.is_finite() && (0.0..=100.0).contains(&n)).then(|| format!("{n}{unit}"))
        }
        Kind::Font => {
            let v = v.trim();
            let ok = !v.is_empty()
                && v.len() <= 64
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c));
            ok.then(|| format!("\"{v}\""))
        }
    }
}

pub fn token(name: &str) -> Option<&'static Token> {
    TOKENS.iter().find(|t| t.name == name)
}

/// The resolved tokens for one scheme, with what went wrong applying the
/// override files.
#[derive(Debug, Clone)]
pub struct Theme {
    pub scheme: Scheme,
    values: BTreeMap<&'static str, String>,
    pub problems: Vec<String>,
}

impl Theme {
    /// The built-in defaults.
    pub fn new(scheme: Scheme) -> Theme {
        let values = TOKENS
            .iter()
            .map(|t| {
                let v = match scheme {
                    Scheme::Dark => t.dark,
                    Scheme::Light => t.light,
                };
                (
                    t.name,
                    canonical(t.kind, v).expect("built-in token is valid"),
                )
            })
            .collect();
        Theme {
            scheme,
            values,
            problems: Vec::new(),
        }
    }

    /// Applies `~/.config/mirqab/theme.json`: an object whose keys are token
    /// names. Unknown keys and invalid values are skipped and reported.
    pub fn apply(&mut self, overrides: &Value) {
        let Some(map) = overrides.as_object() else {
            self.problems.push(s::theme_not_object(s::THEME_FILE));
            return;
        };
        for (k, v) in map {
            self.set(k, v, s::THEME_FILE);
        }
    }

    /// Applies a followed desktop token file (`theme.follow`). haseen's
    /// shell token file is the one format known so far.
    pub fn follow(&mut self, file: &Value) {
        let Some(map) = file.as_object() else {
            self.problems
                .push(s::theme_not_object(s::FOLLOWED_THEME_FILE));
            return;
        };
        for (theirs, ours) in FOLLOW_HASEEN {
            if let Some(v) = map.get(*theirs) {
                for name in *ours {
                    self.set(name, v, s::FOLLOWED_THEME_FILE);
                }
            }
        }
        if let Some(r) = map.get("radius").and_then(Value::as_f64) {
            self.set(
                "mirqab-radius-card",
                &Value::from(format!("{r}px")),
                s::FOLLOWED_THEME_FILE,
            );
        }
    }

    /// The scheme a followed file asks for (haseen's `mode`), if any.
    pub fn followed_scheme(file: &Value) -> Option<Scheme> {
        match file.get("mode")?.as_str()? {
            "dark" => Some(Scheme::Dark),
            "light" => Some(Scheme::Light),
            _ => None,
        }
    }

    fn set(&mut self, name: &str, v: &Value, source: &str) {
        let Some(t) = token(name) else {
            self.problems.push(s::theme_unknown_token(source, name));
            return;
        };
        match v.as_str().and_then(|s| canonical(t.kind, s)) {
            Some(c) => {
                self.values.insert(t.name, c);
            }
            None => self
                .problems
                .push(s::theme_invalid_value(source, name, &v.to_string())),
        }
    }

    /// A token's value in canonical CSS form.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// A colour token for drawing code.
    pub fn color(&self, name: &str) -> Option<Rgba> {
        Rgba::parse(self.value(name)?)
    }

    /// Series colour `n`, counted from 0 and wrapping.
    pub fn series(&self, n: usize) -> Rgba {
        let name = SERIES_NAMES[n % SERIES];
        self.color(name).expect("series tokens are colours")
    }

    /// The stylesheet that sets every variable once, on `:root`.
    pub fn stylesheet(&self) -> String {
        let mut out = String::from(
            "/* mirqab: generated from the token table; not edited by hand */\n:root {\n",
        );
        for t in TOKENS {
            out.push_str(&format!("  --{}: {};\n", t.name, self.values[t.name]));
        }
        out.push_str("}\n");
        out
    }
}

const SERIES_NAMES: [&str; SERIES] = [
    "mirqab-series-1",
    "mirqab-series-2",
    "mirqab-series-3",
    "mirqab-series-4",
    "mirqab-series-5",
    "mirqab-series-6",
];
