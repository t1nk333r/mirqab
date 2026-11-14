//! `mirqab`: the daemon and the CLI in one binary (architecture §3, §11).
//! A hand-written argument parser keeps it small (plan 001, "Rejected":
//! clap).

mod apps;
mod bar;
mod cli;
mod confirm;
mod daemon;
mod elevate;
mod mcp;
mod mcp_config;
mod out;
mod quit;
mod tray;

use mirqab_core::proto::strings;
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit codes: 0 done; 1 the daemon refused or failed; 2 no daemon; 3 a
/// change that needs confirmation and could not get it; 64 a
/// usage error.
pub const EXIT_FAILED: u8 = 1;
pub const EXIT_NO_DAEMON: u8 = 2;
/// A change that needs confirmation and could not get it (plan 005).
pub const EXIT_CONFIRM: u8 = 3;
pub const EXIT_USAGE: u8 = 64;

/// The socket every command may name, and what is left for the command.
pub struct Args {
    pub socket: Option<PathBuf>,
    pub rest: Vec<String>,
}

fn usage_error(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

fn main() -> ExitCode {
    let mut socket = None;
    let mut rest = Vec::new();
    let mut help = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--socket" => match it.next() {
                Some(v) => socket = Some(PathBuf::from(v)),
                None => return usage_error(&strings::missing_value("--socket")),
            },
            "-h" | "--help" => help = true,
            "-V" | "--version" => {
                out::print(&format!(
                    "{} {}\n",
                    strings::APP_NAME,
                    mirqab_core::proto::VERSION
                ));
                return ExitCode::SUCCESS;
            }
            _ => rest.push(a),
        }
    }
    let command = if rest.is_empty() {
        String::new()
    } else {
        rest.remove(0)
    };

    // `mirqab`, `mirqab --help`, `mirqab help [COMMAND]`, `mirqab COMMAND --help`.
    if command.is_empty() {
        out::print(&strings::cli_usage());
        return if help {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(EXIT_USAGE)
        };
    }
    let topic = match (command.as_str(), help) {
        ("help", true) if rest.is_empty() => Some("help".to_string()),
        ("help", _) => Some(rest.first().cloned().unwrap_or_default()),
        (c, true) => Some(c.to_string()),
        _ => None,
    };
    if let Some(topic) = topic {
        if topic.is_empty() {
            out::print(&strings::cli_usage());
            return ExitCode::SUCCESS;
        }
        if topic == "--markdown" && command == "help" {
            out::print(&strings::cli_markdown());
            return ExitCode::SUCCESS;
        }
        return match strings::command_help(&topic) {
            Some(text) => {
                out::print(&text);
                ExitCode::SUCCESS
            }
            None if topic.starts_with('-') => usage_error(&strings::unknown_option(&topic)),
            None => usage_error(&strings::unknown_command(&topic)),
        };
    }

    let args = Args { socket, rest };
    match command.as_str() {
        "daemon" => daemon::run(args),
        "status" | "open" | "toggle" | "close" | "history" | "load" | "sensors" => {
            cli::run(&command, args)
        }
        "bar" => bar::run(args),
        "apps" | "what" => apps::run(&command, args),
        "quit" => quit::run(args),
        // With no socket to name, the server still starts: each tool then
        // says there is no daemon, which the agent can tell the user.
        "mcp" | "--mcp" if !args.rest.is_empty() => {
            usage_error(&strings::unknown_option(&args.rest[0]))
        }
        "mcp" | "--mcp" => mcp::run(
            &args
                .socket
                .clone()
                .or_else(mirqab_core::server::default_socket)
                .unwrap_or_default(),
        ),
        "mcp-config" => mcp_config::run(args),
        "completions" => match args.rest.as_slice() {
            [shell] => match strings::cli_completions(shell) {
                Some(script) => {
                    out::print(&script);
                    ExitCode::SUCCESS
                }
                None => usage_error(&strings::bad_shell(shell)),
            },
            _ => usage_error(&strings::command_help("completions").unwrap_or_default()),
        },
        other => usage_error(&strings::unknown_command(other)),
    }
}
