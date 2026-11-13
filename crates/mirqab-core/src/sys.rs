//! The few system calls std does not wrap: the peer's uid on a Unix socket,
//! this process's uid, and adopting a socket systemd passed in. Each `unsafe`
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
