//! The StatusNotifierItem's properties and clicks (plan 004).

use crate::bar::items;
use crate::cli::call;
use mirqab_core::proto::stream::{self, SystemData};
use mirqab_core::proto::{Request, strings};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A host that reads the tooltip on a timer gets the cached line, so the
/// tray never samples more than once in this long (plan 004).
const TIP_CACHE: Duration = Duration::from_secs(10);

pub struct Item {
    pub(super) socket: PathBuf,
    tip: Mutex<Option<(Instant, String)>>,
}

impl Item {
    pub fn new(socket: &Path) -> Item {
        Item {
            socket: socket.to_path_buf(),
            tip: Mutex::new(None),
        }
    }

    /// Sends `ui.*` as an action, as `mirqab open` does; a failure is the
    /// daemon's to log.
    pub(super) fn ui(&self, name: &str, tab: Option<&str>) {
        let _ = call(
            &self.socket,
            Request::new(1, "action")
                .with_params(json!({"name": name, "args": {"tab": tab}, "dryRun": false})),
        );
    }

    /// `CPU 12% · Mem 41% · 52°` from one `get system`.
    fn tip_line(&self) -> String {
        let Ok(v) = call(
            &self.socket,
            Request::new(1, "get").with_stream(stream::SYSTEM),
        ) else {
            return strings::APP_NAME.to_string();
        };
        let d: SystemData = serde_json::from_value(v).unwrap_or_default();
        let text = |item: &str| {
            items::read(item, &d, &[])
                .map(|r| r.text)
                .unwrap_or_else(|| strings::NO_VALUE.to_string())
        };
        strings::tray_tooltip(&text("cpu"), &text("mem"), &text("temp"))
    }

    /// The tooltip line, from the cache when it is under 10 s old.
    pub fn tooltip_line(&self) -> String {
        let mut tip = self.tip.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, line)) = tip.as_ref()
            && at.elapsed() < TIP_CACHE
        {
            return line.clone();
        }
        let line = self.tip_line();
        *tip = Some((Instant::now(), line.clone()));
        line
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        strings::APP_NAME.into()
    }

    fn title(&self) -> String {
        strings::APP_NAME.into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn icon_name(&self) -> String {
        strings::TRAY_ICON.into()
    }

    /// Left click: show or hide the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.ui("ui.toggle", None);
    }

    /// Middle click: Home.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.ui("ui.show", Some("home"));
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tooltip_line(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        super::menu::items(self)
    }
}

/// The streams the daemon serves, from `status`.
pub(super) fn capabilities(socket: &Path) -> Vec<String> {
    call(socket, Request::new(1, "status"))
        .ok()
        .and_then(|v| {
            v["streams"]
                .as_object()
                .map(|m| m.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// `tabs.hidden` from the daemon's config.
pub(super) fn hidden(socket: &Path) -> Vec<String> {
    call(socket, Request::new(1, "config.get"))
        .ok()
        .and_then(|v| v["tabs"]["hidden"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
