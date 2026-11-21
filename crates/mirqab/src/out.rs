//! Every line the CLI prints goes through here (plan 005). `println!` and
//! `eprintln!` panic when their stream is gone, and the release profile
//! turns a panic into an abort, so `mirqab bar --list | head -1` dumped
//! core (review of PR #10). A `tools/lint.sh` rule keeps both macros out of
//! the rest of this crate.

use mirqab_core::proto::strings;
use std::io::{ErrorKind, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by a command that changes something (`keep_going`): a reader that
/// went away no longer ends the process.
static KEEP_GOING: AtomicBool = AtomicBool::new(false);
/// The reader went away under `keep_going`: nothing more is written.
static STDOUT_GONE: AtomicBool = AtomicBool::new(false);

/// From here on a broken pipe does not end the process: stdout is marked
/// gone, nothing more is written to it, and the command finishes and exits
/// with its own code. The confirm flow calls it, so `mirqab quit firefox -y
/// | head -c0` still quits, and a failure after the act still exits 1
/// (audit 2026-11-21, L40).
pub fn keep_going() {
    KEEP_GOING.store(true, Ordering::Relaxed);
}

/// Whether stdout's reader went away under `keep_going`: a command then
/// says on stderr what failed, which it would have printed (review of
/// audit 2026-11-21, L40).
pub fn stdout_gone() -> bool {
    STDOUT_GONE.load(Ordering::Relaxed)
}

/// Writes `text` to stdout as is. A reader that went away (a broken pipe)
/// ends the process with status 0, as it asked for, unless a command that
/// changes something said `keep_going`; any other failure (a full disk
/// under a redirect) is reported and ends it with status 1, so a truncated
/// file never looks like success (review of PR #12).
pub fn print(text: &str) {
    if STDOUT_GONE.load(Ordering::Relaxed) {
        return;
    }
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_all(text.as_bytes()).and_then(|_| out.flush()) {
        if e.kind() == ErrorKind::BrokenPipe {
            if KEEP_GOING.load(Ordering::Relaxed) {
                STDOUT_GONE.store(true, Ordering::Relaxed);
                return;
            }
            std::process::exit(0);
        }
        err(&strings::output_failed(&e.to_string()));
        std::process::exit(i32::from(crate::EXIT_FAILED));
    }
}

/// `print` with a newline.
pub fn line(text: &str) {
    print(&format!("{text}\n"));
}

/// A line on stderr. A stderr that cannot be written is ignored: there is
/// nowhere left to say so.
pub fn err(text: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{text}");
}
