//! The few system calls std does not wrap: the peer's uid on a Unix socket,
//! this process's uid, adopting a socket systemd passed in, and polling
//! stdout for a closed reader. Each `unsafe`
//! block is one libc call with its contract stated; nothing else in the
//! daemon uses `unsafe` (CLAUDE.md "Style").
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};

/// The uid of the process at the other end of a connected Unix socket.
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the fd is a valid, open socket owned by `stream` for the whole
    // call; `cred` and `len` point to live stack values of the size we pass,
    // which is what SO_PEERCRED writes.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(cred.uid)
}

/// This process's real uid.
pub fn uid() -> u32 {
    // SAFETY: getuid takes no arguments, cannot fail and touches no memory.
    unsafe { libc::getuid() }
}

/// The first socket systemd passed (sd_listen_fds(3)): fd 3, when
/// LISTEN_PID names this process and LISTEN_FDS is at least 1.
pub fn listen_fd() -> Option<UnixListener> {
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let fds: u32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if pid != std::process::id() || fds < 1 {
        return None;
    }
    const SD_LISTEN_FDS_START: RawFd = 3;
    // SAFETY: systemd's contract is that fd 3 is an open listening socket
    // handed to this pid and owned by nobody else in the process; we take
    // ownership exactly once, here, before anything could open fd 3.
    let listener = unsafe { UnixListener::from_raw_fd(SD_LISTEN_FDS_START) };
    // systemd passes the socket without close-on-exec; the commands exec.rs
    // runs (systemctl, from phase D) must not inherit the listening socket.
    // SAFETY: fd 3 is the open socket just adopted; F_SETFD with FD_CLOEXEC
    // changes only its close-on-exec flag and touches no memory.
    unsafe {
        libc::fcntl(SD_LISTEN_FDS_START, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    // Children (the commands exec.rs runs) must not inherit the variables.
    // SAFETY: called at start-up on the main thread before any other thread
    // exists, as std::env::remove_var requires.
    unsafe {
        std::env::remove_var("LISTEN_PID");
        std::env::remove_var("LISTEN_FDS");
        std::env::remove_var("LISTEN_FDNAMES");
    }
    Some(listener)
}

/// Whether nobody reads stdout any more: the write end of a pipe whose
/// reader closed reports an error on poll (`mirqab bar --watch`, plan 004).
pub fn stdout_closed() -> bool {
    let mut p = libc::pollfd {
        fd: libc::STDOUT_FILENO,
        events: 0,
        revents: 0,
    };
    // SAFETY: one valid pollfd for the call's duration, a zero timeout.
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    n > 0 && p.revents & (libc::POLLERR | libc::POLLHUP) != 0
}
