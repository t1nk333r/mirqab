//! The one place commands run (architecture §5.3): a fixed argv, no shell,
//! `LC_ALL=C`, stdin closed unless given, and a timeout after which the child
//! is killed. Tests put stubs first on `PATH`.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
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
            ExecError::Spawn(e) => write!(f, "cannot start: {e}"),
            ExecError::Timeout => write!(f, "timed out"),
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
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });

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
    Ok(Output {
        status: status.code(),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
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
    fn a_missing_program_is_a_spawn_error() {
        let r = run(
            &["/nonexistent/mirqab-no-such-program"],
            None,
            Duration::from_secs(1),
        );
        assert!(matches!(r, Err(ExecError::Spawn(_))));
    }
}
