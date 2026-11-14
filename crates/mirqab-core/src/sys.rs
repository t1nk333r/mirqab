//! The few system calls std does not wrap: the peer's uid and pid on a Unix
//! socket, this process's uid, adopting a socket systemd passed in, polling
//! stdout for a closed reader, the boot-time clock, and signalling a
//! process through a pidfd (plan 006). Each `unsafe` block is one libc call
//! with its contract stated; nothing else in the daemon uses `unsafe`
//! (CLAUDE.md "Style").
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

/// The uid of the process at the other end of a connected Unix socket.
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    peer_cred(stream).map(|c| c.uid)
}

/// The pid of the process at the other end of a connected Unix socket, as
/// the kernel recorded it at connect time (plan 006: the caller whose
/// ancestry an action refuses). 0, a peer in a pid namespace this one
/// cannot see, is an error.
pub fn peer_pid(stream: &UnixStream) -> std::io::Result<u32> {
    match peer_cred(stream)?.pid {
        pid if pid > 0 => Ok(pid as u32),
        _ => Err(std::io::Error::from_raw_os_error(libc::ESRCH)),
    }
}

fn peer_cred(stream: &UnixStream) -> std::io::Result<libc::ucred> {
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
    Ok(cred)
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

/// Time since boot, suspend included (CLOCK_BOOTTIME): a plan made before
/// a suspend must not be live after it (plan 006). `Instant` stops while
/// the machine sleeps.
pub fn boottime() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live stack value of the type clock_gettime writes;
    // CLOCK_BOOTTIME exists on every kernel mirqab runs on (2.6.39+).
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    if rc != 0 {
        return Duration::ZERO;
    }
    Duration::new(
        ts.tv_sec.max(0) as u64,
        ts.tv_nsec.clamp(0, 999_999_999) as u32,
    )
}

/// A pidfd for `pid` (pidfd_open(2), Linux 5.3). It refers to that one
/// process for as long as it is open, whatever pid the kernel hands out
/// later, so a check made after opening it and a signal sent through it
/// are about the same process (plan 006, decision 1). ESRCH: no such
/// process; EINVAL: `pid` is a thread that leads no thread group; ENOSYS:
/// a kernel before 5.3. The fd is close-on-exec, as every pidfd is.
pub fn pidfd_open(pid: u32) -> std::io::Result<OwnedFd> {
    let pid =
        libc::pid_t::try_from(pid).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: pidfd_open takes a pid and a flags word by value and returns
    // a new descriptor or -1; no memory is passed.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0 as libc::c_uint) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = RawFd::try_from(fd).map_err(|_| std::io::Error::from_raw_os_error(libc::EBADF))?;
    // SAFETY: the kernel just returned this descriptor; it is open and
    // owned by nothing else in the process.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Sends `signo` to the process `fd` refers to (pidfd_send_signal(2)).
/// ESRCH: it has exited.
pub fn pidfd_signal(fd: BorrowedFd<'_>, signo: i32) -> std::io::Result<()> {
    // SAFETY: the descriptor is open for the call's duration (borrowed); a
    // null info makes the kernel fill the siginfo as the kill syscall
    // would; flags must be 0.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signo,
            std::ptr::null::<libc::siginfo_t>(),
            0 as libc::c_uint,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
