//! The token table and the generated stylesheet (plan 001, Tests): every
//! token has a valid default in both schemes, the stylesheet sets each
//! variable once, and an override changes only its own keys.

use mirqab_view::theme::{self, Kind, Rgba, Scheme, TOKENS, Theme, canonical};
use serde_json::json;
use std::collections::HashSet;

#[test]
fn every_token_is_unique_and_has_valid_defaults() {
    let mut seen = HashSet::new();
    for t in TOKENS {
        assert!(seen.insert(t.name), "{} twice", t.name);
        assert!(canonical(t.kind, t.dark).is_some(), "{} dark", t.name);
        assert!(canonical(t.kind, t.light).is_some(), "{} light", t.name);
    }
    // The app's own tokens the architecture names (§7.3).
    for name in [
        "mirqab-series-1",
        "mirqab-series-6",
        "mirqab-grid",
        "mirqab-meter-track",
        "mirqab-map-land",
        "mirqab-map-night",
    ] {
        assert!(theme::token(name).is_some(), "{name} missing");
    }
}

#[test]
fn the_stylesheet_sets_each_variable_once() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let css = Theme::new(scheme).stylesheet();
        for t in TOKENS {
            let decl = format!("--{}:", t.name);
            assert_eq!(css.matches(&decl).count(), 1, "{decl} in {scheme:?}");
        }
        assert_eq!(css.matches(":root {").count(), 1);
        assert_eq!(css.matches('}').count(), 1);
    }
}

#[test]
fn an_override_changes_only_its_keys() {
    let base = Theme::new(Scheme::Dark);
    let mut t = base.clone();
    t.apply(&json!({"accent-bg-color": "#3584E4", "mirqab-radius-card": "1em"}));
    assert!(t.problems.is_empty(), "{:?}", t.problems);
    assert_eq!(t.value("accent-bg-color"), Some("#3584e4"));
    assert_eq!(t.value("mirqab-radius-card"), Some("1em"));
    for tok in TOKENS {
        if tok.name != "accent-bg-color" && tok.name != "mirqab-radius-card" {
            assert_eq!(t.value(tok.name), base.value(tok.name), "{}", tok.name);
        }
    }
}

#[test]
fn bad_overrides_are_reported_and_cannot_inject_css() {
    let mut t = Theme::new(Scheme::Dark);
    let before = t.value("accent-bg-color").unwrap().to_string();
    t.apply(&json!({
        "accent-bg-color": "red; } window { background: url(x)",
        "mirqab-space-1": "3 ems",
        "mirqab-font-icon": "Evil\"; } * {",
        "no-such-token": "#fff",
        "mirqab-grid": 7
    }));
    assert_eq!(t.problems.len(), 5, "{:?}", t.problems);
    assert_eq!(t.value("accent-bg-color"), Some(before.as_str()));
    let css = t.stylesheet();
    assert!(!css.contains("url("));
    assert!(!css.contains("Evil"));

    let mut t = Theme::new(Scheme::Dark);
    t.apply(&json!(["not", "an", "object"]));
    assert_eq!(t.problems.len(), 1);
}

#[test]
fn colours_parse_in_every_accepted_form() {
    let c = |v| Rgba::parse(v).map(Rgba::to_hex);
    assert_eq!(c("#fff").as_deref(), Some("#ffffff"));
    assert_eq!(c("#ffff").as_deref(), Some("#ffffff"));
    assert_eq!(c("#11223380").as_deref(), Some("#11223380"));
    assert_eq!(c("rgb(255, 0, 0)").as_deref(), Some("#ff0000"));
    assert_eq!(c("rgba(0,0,0,0.5)").as_deref(), Some("#00000080"));
    for bad in [
        "#ggg",
        "#12345",
        "rgb(1,2)",
        "rgba(0,0,0,2)",
        "rgb(300,0,0)",
        "red",
        "",
    ] {
        assert!(Rgba::parse(bad).is_none(), "{bad}");
    }
    assert_eq!(canonical(Kind::Length, "0.5em").as_deref(), Some("0.5em"));
    assert_eq!(canonical(Kind::Length, "12px").as_deref(), Some("12px"));
    assert!(canonical(Kind::Length, "-1em").is_none());
    assert!(canonical(Kind::Length, "1em;").is_none());
    assert_eq!(
        canonical(Kind::Font, "Symbols Nerd Font").as_deref(),
        Some("\"Symbols Nerd Font\"")
    );
}

#[test]
fn following_haseen_maps_its_keys_and_the_user_file_wins() {
    let followed = json!({
        "mode": "light",
        "background": "#101010",
        "accent": "#00aa88",
        "urgent": "#ff0000",
        "radius": 6,
        "fontFamily": "ignored"
    });
    assert_eq!(Theme::followed_scheme(&followed), Some(Scheme::Light));
    let mut t = Theme::new(Scheme::Dark);
    t.follow(&followed);
    assert!(t.problems.is_empty(), "{:?}", t.problems);
    assert_eq!(t.value("window-bg-color"), Some("#101010"));
    assert_eq!(t.value("sidebar-bg-color"), Some("#101010"));
    assert_eq!(t.value("accent-bg-color"), Some("#00aa88"));
    assert_eq!(t.value("mirqab-series-1"), Some("#00aa88"));
    assert_eq!(t.value("error-color"), Some("#ff0000"));
    assert_eq!(t.value("mirqab-radius-card"), Some("6px"));
    t.apply(&json!({"accent-bg-color": "#123456"}));
    assert_eq!(t.value("accent-bg-color"), Some("#123456"));
}

#[test]
fn series_colours_wrap() {
    let t = Theme::new(Scheme::Dark);
    assert_eq!(t.series(0), t.series(theme::SERIES));
    assert_ne!(t.series(0), t.series(1));
}
