//! The one place commands run (architecture §5.3): a fixed argv, no shell,
//! `LC_ALL=C`, stdin closed unless given, and a timeout after which the child
//! is killed. Output that is not UTF-8 is kept, lossily; once the child has
//! exited, its pipes are read only until the same deadline, so a process it
//! left running cannot hold the call. A killed child that cannot die at
//! once (stuck in uninterruptible sleep) is reaped on a thread of its own,
//! so it cannot hold the call either, and the same program is not started
//! again until it has died. Tests put stubs first on `PATH`.

use crate::proto::strings;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
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
    /// It ran past the timeout and was killed; or an earlier run of the
    /// same program was, has not died yet, and this one was not started.
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
    if outstanding(prog) {
        return Err(ExecError::Timeout);
    }
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
                reap_or_detach(prog, child, GRACE, spawn_reaper);
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
    drain_ready(&rx, &mut bufs);
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

/// Reaping a killed child: a check that does not block, and a wait that
/// does. `Child`, and a fake in the tests.
trait Reap: Send + 'static {
    /// Whether the child is gone: reaped, or no longer ours to wait for.
    fn reaped(&mut self) -> bool;
    fn wait(&mut self);
}

impl Reap for std::process::Child {
    fn reaped(&mut self) -> bool {
        !matches!(self.try_wait(), Ok(None))
    }

    fn wait(&mut self) {
        let _ = std::process::Child::wait(self);
    }
}

/// A program whose child was killed at its timeout and has not died yet.
/// The program is not started again meanwhile: a driver fault that hangs
/// one `nvidia-smi` hangs the next as well, and each would add a task in
/// uninterruptible sleep (one more to the load average) and a thread.
struct Stuck {
    id: u64,
    program: String,
    /// The child, when no thread could be started to wait for it: `run`
    /// checks it without blocking instead.
    held: Option<Box<dyn Reap>>,
}

static STUCK: Mutex<Vec<Stuck>> = Mutex::new(Vec::new());
static STUCK_ID: AtomicU64 = AtomicU64::new(0);

fn stuck() -> std::sync::MutexGuard<'static, Vec<Stuck>> {
    // The list stays valid whatever a holder panicked in.
    STUCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether an earlier child of `program` is still dying. A held child
/// that has died since is dropped from the list here.
fn outstanding(program: &str) -> bool {
    let mut list = stuck();
    list.retain_mut(|s| match &mut s.held {
        Some(child) if s.program == program => !child.reaped(),
        _ => true,
    });
    list.iter().any(|s| s.program == program)
}

/// The reaper's stack: it only waits.
const REAPER_STACK: usize = 64 * 1024;

/// Starts `f` on a thread of its own, the way `run` reaps.
fn spawn_reaper(f: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("mirqab-reap".into())
        .stack_size(REAPER_STACK)
        .spawn(f)
        .map(drop)
}

/// Waits up to `grace` for a killed child to die, then leaves it to a
/// thread of its own, and marks `program` as stuck until the thread has
/// reaped it. SIGKILL takes effect only once a task leaves
/// uninterruptible sleep (`nvidia-smi` inside a hung driver call), and the
/// timeout is the call's bound; the thread reaps the child whenever it
/// dies, so no zombie stays (plan 024 L1). When no thread can be started
/// (`std::thread::spawn` would panic, and the release profile aborts on a
/// panic), the child stays in the list and each later `run` of `program`
/// checks it without blocking.
fn reap_or_detach(
    program: &str,
    mut child: impl Reap,
    grace: Duration,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
) {
    let until = Instant::now() + grace;
    while !child.reaped() {
        if Instant::now() >= until {
            // Locked before the thread starts, so its removal cannot come
            // ahead of the entry.
            let mut list = stuck();
            let id = STUCK_ID.fetch_add(1, Ordering::Relaxed);
            // The child reaches the thread only once it runs, so a failed
            // start leaves it here to be held.
            let (tx, rx) = mpsc::channel::<Box<dyn Reap>>();
            let started = spawn(Box::new(move || {
                if let Ok(mut child) = rx.recv() {
                    child.wait();
                }
                stuck().retain(|s| s.id != id);
            }));
            let child: Box<dyn Reap> = Box::new(child);
            let held = match started {
                Ok(()) => tx.send(child).err().map(|e| e.0),
                Err(_) => Some(child),
            };
            list.push(Stuck {
                id,
                program: program.to_string(),
                held,
            });
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// What a reader thread sends: bytes from stream 0 (stdout) or 1 (stderr),
/// or the end of its stream.
enum Chunk {
    Data(usize, Vec<u8>),
    Eof,
}

/// Takes the data already queued, past any end-of-stream between: once
/// the wait has given up, one stream's end can sit ahead of the other's
/// last bytes.
fn drain_ready(rx: &mpsc::Receiver<Chunk>, bufs: &mut [Vec<u8>; 2]) {
    while let Ok(c) = rx.try_recv() {
        if let Chunk::Data(i, b) = c {
            bufs[i].extend_from_slice(&b);
        }
    }
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

    /// Polls `run(program)` until it is a spawn error (the program does not
    /// exist, so a spawn error means it was started), for at most 5 s.
    fn until_started(program: &str) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if let Err(ExecError::Spawn(_)) = run(&[program], None, Duration::from_secs(1)) {
                return true;
            }
            std::thread::sleep(POLL);
        }
        false
    }

    #[test]
    fn a_killed_child_that_cannot_die_is_reaped_on_its_own_thread() {
        // Plan 024 L1: SIGKILL waits for a task in uninterruptible sleep,
        // which no test can make, so a fake stands in: it never dies until
        // released, and its wait reports when it ran. For the same reason
        // `run`'s own call of `reap_or_detach` is not exercised here: were
        // it to go back to a blocking `child.wait()`, no test would fail
        // (`kills_on_timeout` uses a child that dies at once).
        struct Hung(mpsc::Receiver<()>, mpsc::Sender<()>);
        impl Reap for Hung {
            fn reaped(&mut self) -> bool {
                false
            }
            fn wait(&mut self) {
                let _ = self.0.recv();
                let _ = self.1.send(());
            }
        }
        // A program that does not exist: starting it is a spawn error, so
        // a timeout means it was refused before any start.
        let program = "/nonexistent/mirqab-test-hung-thread";
        let (release, released) = mpsc::channel();
        let (reaped_tx, reaped) = mpsc::channel();
        let t = Instant::now();
        reap_or_detach(program, Hung(released, reaped_tx), GRACE, spawn_reaper);
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        assert!(reaped.try_recv().is_err(), "still stuck");
        // Review of the L1 fix: while the killed child lives, the program
        // is not started again, so a hung driver cannot pile up children.
        for _ in 0..2 {
            let r = run(&[program], None, Duration::from_secs(1));
            assert!(matches!(r, Err(ExecError::Timeout)), "{r:?}");
        }
        release.send(()).unwrap();
        reaped
            .recv_timeout(Duration::from_secs(5))
            .expect("reaped once it could die");
        assert!(until_started(program), "runs again once reaped");
    }

    #[test]
    fn a_child_no_thread_could_wait_for_is_checked_by_the_next_run() {
        // Review of the L1 fix: a thread that cannot be started must not
        // panic (the release profile aborts); the child is kept, and each
        // later run checks it without blocking.
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        struct Held(Arc<AtomicBool>);
        impl Reap for Held {
            fn reaped(&mut self) -> bool {
                self.0.load(Ordering::SeqCst)
            }
            fn wait(&mut self) {
                unreachable!("no thread waits for a held child");
            }
        }
        let program = "/nonexistent/mirqab-test-hung-held";
        let dead = Arc::new(AtomicBool::new(false));
        reap_or_detach(program, Held(dead.clone()), GRACE, |_| {
            Err(std::io::Error::other("no threads"))
        });
        let r = run(&[program], None, Duration::from_secs(1));
        assert!(matches!(r, Err(ExecError::Timeout)), "{r:?}");
        dead.store(true, Ordering::SeqCst);
        let r = run(&[program], None, Duration::from_secs(1));
        assert!(matches!(r, Err(ExecError::Spawn(_))), "{r:?}");
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
            Duration::from_secs(1),
        );
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        let out = r.unwrap();
        assert_eq!(out.status, Some(0));
        assert_eq!(out.stdout, "early\n");
    }

    #[test]
    fn the_final_drain_reads_past_an_end_of_stream() {
        // Review of audit 2026-11-21, L4: after the wait times out, the
        // queue can hold stderr's end ahead of stdout's last bytes. The
        // threads' timing cannot be forced through `run`, so the queue is
        // built by hand.
        let (tx, rx) = mpsc::channel();
        tx.send(Chunk::Eof).unwrap();
        tx.send(Chunk::Data(0, b"late".to_vec())).unwrap();
        tx.send(Chunk::Data(1, b"err".to_vec())).unwrap();
        let mut bufs = [Vec::new(), Vec::new()];
        drain_ready(&rx, &mut bufs);
        assert_eq!(bufs, [b"late".to_vec(), b"err".to_vec()]);
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
