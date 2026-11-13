//! The confirm flow of a command that changes something (plan 005): it
//! prints what it will do; `--dry-run` stops there; `-y` goes ahead; with a
//! terminal it asks `[y/N]`; with no terminal and no `-y` it does nothing,
//! and the command exits 3.

use crate::out;
use mirqab_core::proto::strings;
use std::io::{BufRead, IsTerminal};

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Go,
    DryRun,
    /// Declined, or no terminal to ask in.
    No,
}

pub fn ask(would_do: &[String], yes: bool, dry_run: bool) -> Answer {
    out::line(strings::CONFIRM_WILL);
    for w in would_do {
        out::line(&format!("  {w}"));
    }
    if dry_run {
        return Answer::DryRun;
    }
    if yes {
        return Answer::Go;
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        crate::out::err(strings::CONFIRM_NEEDS_TERMINAL);
        return Answer::No;
    }
    out::print(strings::CONFIRM_PROMPT);
    let mut reply = String::new();
    let _ = stdin.lock().read_line(&mut reply);
    match reply.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Answer::Go,
        _ => {
            crate::out::err(strings::CONFIRM_DECLINED);
            Answer::No
        }
    }
}
