//! The one place commands run (architecture §5.3): a fixed argv, no shell,
//! `LC_ALL=C`, stdin closed unless given, and a timeout after which the child
//! is killed. Output that is not UTF-8 is kept, lossily; once the child has
//! exited, its pipes are read only until the same deadline, so a process it
//! left running cannot hold the call. Tests put stubs first on `PATH`.

use crate::proto::strings;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

#[derive(Debug)]
pub enum ExecError {
    /// The program could not be started (missing, not executable).
    Spawn(std::io::Error),
    /// It ran past the timeout and was killed.
    Timeout,
    Io(std::io::Error),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Spawn(e) => f.write_str(&strings::cannot_start(&e.to_string())),
            ExecError::Timeout => f.write_str(strings::TIMED_OUT),
            ExecError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// How often a running child is checked. Only while a command runs, never
/// as a sampler: the commands are short and rare.
const POLL: Duration = Duration::from_millis(10);

/// Runs `argv` and waits at most `timeout`. `input`, when given, is written to
/// stdin (curl's `--config -` keeps secrets out of the process list, §5.6).
pub fn run(argv: &[&str], input: Option<&[u8]>, timeout: Duration) -> Result<Output, ExecError> {
    let (prog, args) = argv.split_first().ok_or_else(|| {
        ExecError::Spawn(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty argv",
        ))
    })?;
    let mut child = Command::new(prog)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ExecError::Spawn)?;

    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        // A child that exits without reading closes the pipe; that is its
        // business, and its status says so.
        let _ = stdin.write_all(data);
    }
    // Drain both pipes on their own threads, so a chatty child cannot block
    // on a full pipe while we wait for it.
    let (tx, rx) = mpsc::channel();
    pump(child.stdout.take(), 0, tx.clone());
    pump(child.stderr.take(), 1, tx);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ExecError::Timeout);
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(ExecError::Io(e)),
        }
    };
    // The child has exited, but a process it left behind (`cmd &`) can hold
    // the pipes open for as long as it likes. Collect until both reach EOF
    // or the same deadline passes, with a short grace so a child that wrote
    // just before exiting at the deadline is still read. Past that, the
    // command's own result stands with what was read: the child finished in
    // time, and a straggler's output is not its answer. The reader threads
    // end when the straggler closes the pipes.
    let until = deadline.max(Instant::now() + GRACE);
    let mut bufs = [Vec::new(), Vec::new()];
    let mut open = 2;
    while open > 0 {
        let left = until.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Chunk::Data(i, b)) => bufs[i].extend_from_slice(&b),
            Ok(Chunk::Eof) => open -= 1,
            Err(_) => break,
        }
    }
    while let Ok(Chunk::Data(i, b)) = rx.try_recv() {
        bufs[i].extend_from_slice(&b);
    }
    // One byte that is not UTF-8 must not cost the whole stream.
    let [out, err] = bufs;
    Ok(Output {
        status: status.code(),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    })
}

/// How long output is still collected after a child exits at its deadline.
const GRACE: Duration = Duration::from_millis(100);

/// What a reader thread sends: bytes from stream 0 (stdout) or 1 (stderr),
/// or the end of its stream.
enum Chunk {
    Data(usize, Vec<u8>),
    Eof,
}

/// Reads `pipe` to its end on a thread of its own, sending what arrives.
fn pump(pipe: Option<impl Read + Send + 'static>, stream: usize, tx: mpsc::Sender<Chunk>) {
    std::thread::spawn(move || {
        if let Some(mut p) = pipe {
            let mut buf = [0u8; 8192];
            loop {
                match p.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(Chunk::Data(stream, buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = tx.send(Chunk::Eof);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_with_c_locale_and_captures_output() {
        let out = run(
            &["sh", "-c", "echo $LC_ALL; echo err >&2; exit 3"],
            None,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out.stdout, "C\n");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(out.status, Some(3));
        assert!(!out.success());
    }

    #[test]
    fn passes_stdin() {
        let out = run(&["cat"], Some(b"hello"), Duration::from_secs(5)).unwrap();
        assert_eq!(out.stdout, "hello");
    }

    #[test]
    fn kills_on_timeout() {
        let t = Instant::now();
        let r = run(&["sleep", "5"], None, Duration::from_millis(100));
        assert!(matches!(r, Err(ExecError::Timeout)));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn output_that_is_not_utf8_is_kept_lossily() {
        // Audit 2026-11-21, L3: one bad byte used to empty the whole stream.
        let out = run(
            &["sh", "-c", "printf 'ok\\377'; printf 'e\\377' >&2"],
            None,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(out.stdout.starts_with("ok"), "{:?}", out.stdout);
        assert!(out.stderr.starts_with('e'), "{:?}", out.stderr);
    }

    #[test]
    fn a_grandchild_holding_the_pipes_does_not_outlast_the_timeout() {
        // Audit 2026-11-21, L4: the child exits at once, but a background
        // grandchild keeps both pipes open for five seconds.
        let t = Instant::now();
        let r = run(
            &["sh", "-c", "echo early; sleep 5 & exit 0"],
            None,
            Duration::from_millis(300),
        );
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        let out = r.unwrap();
        assert_eq!(out.status, Some(0));
        assert_eq!(out.stdout, "early\n");
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let r = run(
            &["/nonexistent/mirqab-no-such-program"],
            None,
            Duration::from_secs(1),
        );
        assert!(matches!(r, Err(ExecError::Spawn(_))));
    }
}
