//! `mirqab mcp-config CLIENT` (plan 005): how to register `mirqab mcp` with
//! a coding agent. For the clients with their own `mcp add` command it is
//! that command, which `--write` runs after asking; mirqab never edits
//! another program's file itself. Cursor and the generic form get the JSON
//! to add. The commands were read from each client's `--help`
//! (plan 005, As built).

use crate::confirm::{self, Answer};
use crate::{Args, EXIT_CONFIRM, EXIT_FAILED, EXIT_USAGE};
use mirqab_core::exec;
use mirqab_core::proto::strings;
use serde_json::json;
use std::process::ExitCode;
use std::time::Duration;

/// The client's own command that registers the server, if it has one.
pub fn command(client: &str) -> Option<&'static [&'static str]> {
    Some(match client {
        "claude" => &["claude", "mcp", "add", "mirqab", "--", "mirqab", "mcp"],
        "codex" => &["codex", "mcp", "add", "mirqab", "--", "mirqab", "mcp"],
        "gemini" => &["gemini", "mcp", "add", "mirqab", "mirqab", "mcp"],
        "opencode" => &[
            "opencode", "mcp", "add", "--global", "mirqab", "--", "mirqab", "mcp",
        ],
        _ => return None,
    })
}

/// The JSON block for clients configured by file.
pub fn json_block() -> String {
    serde_json::to_string_pretty(
        &json!({"mcpServers": {"mirqab": {"command": "mirqab", "args": ["mcp"]}}}),
    )
    .unwrap_or_default()
}

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

pub fn run(args: Args) -> ExitCode {
    let (mut client, mut write, mut yes, mut dry_run) = (None, false, false, false);
    for a in &args.rest {
        match a.as_str() {
            "--write" => write = true,
            "-y" | "--yes" => yes = true,
            "--dry-run" => dry_run = true,
            s if !s.starts_with('-') && client.is_none() => client = Some(s.to_string()),
            other => return usage(&strings::unknown_option(other)),
        }
    }
    let Some(client) = client else {
        return usage(&strings::command_help("mcp-config").unwrap_or_default());
    };
    if !strings::MCP_CLIENTS.contains(&client.as_str()) {
        return usage(&strings::mcp_bad_client(&client));
    }
    let argv = command(&client);
    if !write && !dry_run {
        match argv {
            Some(a) => crate::out::print(&strings::mcp_config_run(&a.join(" "))),
            None => crate::out::print(&strings::mcp_config_json(
                strings::mcp_config_path(&client),
                &json_block(),
            )),
        }
        return ExitCode::SUCCESS;
    }
    let Some(argv) = argv else {
        crate::out::err(strings::MCP_CONFIG_PRINT_ONLY);
        return ExitCode::from(EXIT_FAILED);
    };
    match confirm::ask(&[argv.join(" ")], yes, dry_run) {
        Answer::DryRun => ExitCode::SUCCESS,
        Answer::No => ExitCode::from(EXIT_CONFIRM),
        Answer::Go => match exec::run(argv, None, Duration::from_secs(60)) {
            Ok(out) if out.success() => {
                crate::out::print(&out.stdout);
                ExitCode::SUCCESS
            }
            Ok(out) => {
                crate::out::err(&strings::mcp_config_failed(out.stderr.trim()));
                ExitCode::from(EXIT_FAILED)
            }
            Err(e) => {
                crate::out::err(&strings::mcp_config_failed(&e.to_string()));
                ExitCode::from(EXIT_FAILED)
            }
        },
    }
}
