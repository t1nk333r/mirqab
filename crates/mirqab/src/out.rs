//! Every line the CLI prints goes through here (plan 005). `println!` and
//! `eprintln!` panic when their stream is gone, and the release profile
//! turns a panic into an abort, so `mirqab bar --list | head -1` dumped
//! core (review of PR #10). A `tools/lint.sh` rule keeps both macros out of
//! the rest of this crate.

use mirqab_core::proto::strings;
use std::io::{ErrorKind, Write};

/// Writes `text` to stdout as is. A reader that went away (a broken pipe)
/// ends the process with status 0, as it asked for; any other failure (a
/// full disk under a redirect) is reported and ends it with status 1, so a
/// truncated file never looks like success (review of PR #12).
pub fn print(text: &str) {
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_all(text.as_bytes()).and_then(|_| out.flush()) {
        if e.kind() == ErrorKind::BrokenPipe {
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
