//! The one real signal in the tests (plan 006, part 2): `act::signal` on
//! the live machine, through a pidfd, to a `sleep` this test started and
//! to nothing else. A second child, named with a wrong start time, is not
//! signalled; the test ends it itself. Both children are ended when the
//! test leaves, however it leaves.

use mirqab_core::Root;
use mirqab_core::act::{self, ActError, Guard, Real, Signal, Target};
use mirqab_core::sys;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};

/// A child of this test, ended and reaped on drop.
struct Owned(Child);

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn sleeper() -> Owned {
    Owned(
        Command::new("sleep")
            .arg("1000")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleep starts"),
    )
}

#[test]
fn a_real_signal_reaches_only_the_planned_child() {
    let root = Root::host();
    let guard = Guard {
        own_uid: sys::uid(),
        daemon: vec![std::process::id()],
        refused: Vec::new(),
    };

    // A wrong start time: the pid names another process now, so nothing
    // is sent and the child keeps running.
    let mut other = sleeper();
    let opid = other.0.id();
    let ostart = act::facts(&root, opid).expect("the child is there").start;
    let wrong = Target {
        pid: opid,
        start: ostart + 1,
    };
    let r = act::signal(&root, &Real, &guard, wrong, Signal::Term, false);
    assert!(matches!(r, Err(ActError::Gone(_))), "{r:?}");
    assert!(
        other.0.try_wait().unwrap().is_none(),
        "the child with the wrong start time still runs"
    );
    drop(other);

    let mut child = sleeper();
    let pid = child.0.id();
    let start = act::facts(&root, pid).expect("the child is there").start;
    let target = Target { pid, start };
    match act::signal(&root, &Real, &guard, target, Signal::Term, false) {
        Ok(entry) => {
            assert_eq!(entry["pid"], pid);
            assert_eq!(entry["signal"], "TERM");
            let status = child.0.wait().unwrap();
            assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        }
        // Outside a user slice (a CI container) the cgroup refusal holds
        // for the test's own child too; the drop ends it.
        Err(ActError::Refused(why)) => {
            let cgroup = root.read(&format!("proc/{pid}/cgroup")).unwrap_or_default();
            let slice = format!("/user.slice/user-{}.slice/", sys::uid());
            assert!(
                !cgroup.contains(&slice),
                "refused inside the user's slice: {why} ({cgroup})"
            );
            assert!(why.contains("outside your own processes"), "{why}");
            assert!(child.0.try_wait().unwrap().is_none(), "nothing was sent");
        }
        Err(e) => panic!("{e:?}"),
    }
}
