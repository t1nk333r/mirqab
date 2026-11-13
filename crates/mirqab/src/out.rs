//! Output that ends quietly when its reader has gone: `println!` panics on
//! a closed pipe, and the release profile aborts on a panic, so
//! `mirqab bar --list | head -1` dumped core (review of PR #10, done in
//! plan 005).

use std::io::Write;

/// Writes `text` to stdout as is; a closed or failing stdout ends the
/// process with status 0, as a reader that stopped reading asked for.
pub fn print(text: &str) {
    let mut out = std::io::stdout().lock();
    if out
        .write_all(text.as_bytes())
        .and_then(|_| out.flush())
        .is_err()
    {
        std::process::exit(0);
    }
}

/// `print` with a newline.
pub fn line(text: &str) {
    print(&format!("{text}\n"));
}
