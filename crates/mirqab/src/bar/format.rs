//! The three output formats of `mirqab bar` (plan 004): Waybar's custom
//! module JSON (`text`, `alt`, `tooltip`, `class`, `percentage`), the
//! plain text, and every field as JSON.

use super::items::Reading;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Waybar,
    Plain,
    Json,
}

impl Format {
    pub fn parse(v: &str) -> Option<Format> {
        match v {
            "waybar" => Some(Format::Waybar),
            "plain" => Some(Format::Plain),
            "json" => Some(Format::Json),
            _ => None,
        }
    }
}

/// One output line for `item`'s reading, without the newline.
pub fn line(f: Format, item: &str, r: &Reading) -> String {
    match f {
        Format::Plain => r.text.clone(),
        Format::Waybar => {
            let mut v = json!({
                "text": r.text,
                "alt": item,
                "tooltip": r.tooltip,
                "class": r.class,
            });
            if let Some(p) = r.percentage {
                v["percentage"] = json!(p);
            }
            v.to_string()
        }
        Format::Json => json!({
            "item": item,
            "text": r.text,
            "tooltip": r.tooltip,
            "class": r.class,
            "percentage": r.percentage,
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waybar_leaves_out_an_unknown_percentage() {
        let r = Reading {
            text: "1.2".into(),
            tooltip: "Load 1.20 0.90 0.70".into(),
            class: "normal",
            percentage: None,
        };
        let v: serde_json::Value = serde_json::from_str(&line(Format::Waybar, "load", &r)).unwrap();
        assert_eq!(v["alt"], "load");
        assert!(v.get("percentage").is_none());
        let v: serde_json::Value = serde_json::from_str(&line(Format::Json, "load", &r)).unwrap();
        assert!(v["percentage"].is_null());
        assert_eq!(line(Format::Plain, "load", &r), "1.2");
    }
}
