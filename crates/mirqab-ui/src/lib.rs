//! `mirqab-ui`: the GTK4/libadwaita window (architecture §7). A library so
//! the widget tests in `tests/` can build its widgets; `main.rs` only calls
//! [`run`].

pub mod app;
pub mod client;
pub mod pages;
pub mod selftest;
pub mod style;
pub mod tabs;
pub mod widgets;
pub mod window;

use adw::prelude::*;
use app::{APP_ID, Ctx, Opts};
use gtk::{gio, glib};
use mirqab_proto::strings;
use std::cell::RefCell;
use std::rc::Rc;

/// Runs the application. A second `mirqab-ui` hands its arguments to the
/// first and exits (a single-instance `adw::Application`); `--selftest` and
/// tests (`MIRQAB_TEST=1`) run without registering on the session bus.
pub fn run() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match Opts::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n{}", strings::UI_USAGE);
            return glib::ExitCode::from(64);
        }
    };
    if opts.help {
        print!("{}", strings::UI_USAGE);
        return glib::ExitCode::SUCCESS;
    }
    let mut flags = gio::ApplicationFlags::HANDLES_COMMAND_LINE;
    if opts.selftest.is_some() || std::env::var("MIRQAB_TEST").as_deref() == Ok("1") {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    let ctx: Rc<RefCell<Option<Rc<Ctx>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, cmd| {
        let args: Vec<String> = cmd
            .arguments()
            .iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let Ok(opts) = Opts::parse(&args) else {
            return glib::ExitCode::from(64);
        };
        let first = ctx.borrow().is_none();
        if first {
            let socket = opts
                .socket
                .clone()
                .or_else(client::default_socket)
                .unwrap_or_default();
            *ctx.borrow_mut() = Some(Ctx::new(app, socket));
        }
        let Some(c) = ctx.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };
        if let Some(tab) = &opts.selftest {
            selftest::run(&c, tab);
        } else if !(first && opts.background) {
            let mode = opts.window.then_some(mirqab_view::config::Mode::Window);
            c.show(opts.tab.as_deref(), mode);
        } else {
            // Started hidden by the unit: wait for a `show`, but not forever.
            c.hide();
        }
        glib::ExitCode::SUCCESS
    });
    app.run_with_args(&std::env::args().collect::<Vec<_>>())
}
