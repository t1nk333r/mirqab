//! The confirm flow of a command that changes something (plan 005): it
//! prints what it will do; `--dry-run` stops there; `-y` goes ahead; with a
//! terminal it asks `[y/N]`; with no terminal and no `-y` it does nothing,
//! and the command exits 3. A stdout whose reader went away changes none of
//! that: the command goes on and exits with its own code (`out::keep_going`).

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
    ask_with(strings::CONFIRM_WILL, would_do, yes, dry_run, false)
}

/// `ask` under another first line (`mirqab quit`'s "This will quit:").
/// With `to_stderr` the header and the list go to stderr, so stdout holds
/// only a `--json` answer (review of PR #17); the prompt still needs
/// stdout to be a terminal.
pub fn ask_with(
    header: &str,
    would_do: &[String],
    yes: bool,
    dry_run: bool,
    to_stderr: bool,
) -> Answer {
    // A reader that goes away from here on does not end the command before
    // it acts or decide its exit code (audit 2026-11-21, L40).
    out::keep_going();
    let say = |text: &str| {
        if to_stderr {
            out::err(text)
        } else {
            out::line(text)
        }
    };
    say(header);
    for w in would_do {
        say(&format!("  {w}"));
    }
    if dry_run {
        return Answer::DryRun;
    }
    if yes {
        return Answer::Go;
    }
    let stdin = std::io::stdin();
    // The question needs someone to see it as well as to answer it: with
    // stdout piped, the prompt would vanish and the read would wait
    // (review of PR #13).
    if !stdin.is_terminal() || !std::io::stdout().is_terminal() {
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
