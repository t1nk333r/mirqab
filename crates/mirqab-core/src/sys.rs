//! The few system calls std does not wrap: the peer's uid and pid on a Unix
//! socket, this process's uid, adopting a socket systemd passed in, polling
//! stdout for a closed reader, the boot-time clock, signalling a process
//! through a pidfd, a Unix-socket connect that cannot hang (plan 006), and
//! the machine's interface addresses (plan 007). Each `unsafe` block is one libc call
//! with its contract stated; nothing else in the daemon uses `unsafe`
//! (CLAUDE.md "Style").
#![allow(unsafe_code)]

use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
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
/// the machine sleeps. An error is returned, never a zero: a clock stuck
/// at zero would keep every plan alive for ever (review of PR #17).
pub fn boottime() -> std::io::Result<Duration> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live stack value of the type clock_gettime writes;
    // CLOCK_BOOTTIME exists on every kernel mirqab runs on (2.6.39+).
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Duration::new(
        ts.tv_sec.max(0) as u64,
        ts.tv_nsec.clamp(0, 999_999_999) as u32,
    ))
}

/// A pidfd for `pid` (pidfd_open(2), Linux 5.3). It refers to that one
/// process for as long as it is open, whatever pid the kernel hands out
/// later, so a check made after opening it and a signal sent through it
/// are about the same process (plan 006, decision 1). ESRCH: no such
/// process; EINVAL (older kernels) or ENOENT (seen on 7.2): `pid` is a
/// thread that leads no thread group; ENOSYS: a kernel before 5.3. The fd
/// is close-on-exec, as every pidfd is.
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

/// A connected stream to the Unix socket at `path`, or an error within
/// about `timeout` (plan 006, part 4, decision 11; D2 of PR #17).
/// `UnixStream::connect` waits without bound when the listener's backlog
/// is full, which a frozen compositor's would be. On Linux an `AF_UNIX`
/// stream connect waits for a backlog slot for the socket's send timeout
/// (`sock_sndtimeo` in `unix_stream_connect`, net/unix/af_unix.c) and then
/// fails with EAGAIN; a non-blocking one fails with EAGAIN at once and
/// never reports EINPROGRESS, so there is nothing to `poll`. The socket is
/// made blocking with `SO_SNDTIMEO` set to `timeout`, connected, and its
/// send timeout cleared; EAGAIN after the wait is returned as ETIMEDOUT.
/// The descriptor is close-on-exec.
pub fn connect_timeout(path: &Path, timeout: Duration) -> std::io::Result<UnixStream> {
    let bytes = path.as_os_str().as_bytes();
    let mut addr = libc::sockaddr_un {
        sun_family: libc::AF_UNIX as libc::sa_family_t,
        sun_path: [0; 108],
    };
    // One byte is kept for the terminating NUL.
    if bytes.is_empty() || bytes.len() >= addr.sun_path.len() || bytes.contains(&0) {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    for (d, s) in addr.sun_path.iter_mut().zip(bytes) {
        *d = *s as libc::c_char;
    }
    if timeout.is_zero() {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    // SAFETY: socket takes three integers by value and returns a new
    // descriptor or -1; no memory is passed.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the kernel just returned this descriptor; it is open and
    // owned by nothing else in the process.
    let stream = UnixStream::from(unsafe { OwnedFd::from_raw_fd(fd) });
    // SO_SNDTIMEO, which bounds the connect's wait for a backlog slot.
    stream.set_write_timeout(Some(timeout))?;
    let len = (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t;
    // SAFETY: the descriptor is open for the call's duration (owned by
    // `stream`); `addr` is a live, initialised sockaddr_un and `len` does
    // not pass its size.
    let rc = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&addr as *const libc::sockaddr_un).cast(),
            len,
        )
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        return Err(match e.raw_os_error() {
            Some(libc::EAGAIN) => std::io::Error::from_raw_os_error(libc::ETIMEDOUT),
            _ => e,
        });
    }
    stream.set_write_timeout(None)?;
    Ok(stream)
}

/// Frees the list `getifaddrs` gave, whichever way `ifaddrs` returns.
struct IfAddrs(*mut libc::ifaddrs);

impl Drop for IfAddrs {
    fn drop(&mut self) {
        // SAFETY: the pointer is the head getifaddrs returned, freed exactly
        // once, here; nothing reads the list after the guard drops.
        unsafe { libc::freeifaddrs(self.0) }
    }
}

/// Every IPv4 and IPv6 address of every interface, with the interface's
/// name (getifaddrs(3)): what the `ports` stream classifies a bind address
/// by (plan 007). The names never leave the daemon.
pub fn ifaddrs() -> std::io::Result<Vec<(String, std::net::IpAddr)>> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `head` is a live stack pointer getifaddrs writes the list's
    // head to; on success the list is ours until freeifaddrs.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let list = IfAddrs(head);
    let mut out = Vec::new();
    let mut cur = list.0;
    while !cur.is_null() {
        // SAFETY: `cur` is a non-null node of the list getifaddrs built,
        // alive until `list` drops; each node's name is a NUL-terminated
        // string and its `ifa_addr`, when not null, points to a sockaddr
        // whose family says which sockaddr type it is (getifaddrs(3)).
        let (next, entry) = unsafe {
            let ifa = &*cur;
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                .to_string_lossy()
                .into_owned();
            let addr = if ifa.ifa_addr.is_null() {
                None
            } else {
                match i32::from((*ifa.ifa_addr).sa_family) {
                    libc::AF_INET => {
                        let sin = &*ifa.ifa_addr.cast::<libc::sockaddr_in>();
                        Some(IpAddr::V4(Ipv4Addr::from(
                            sin.sin_addr.s_addr.to_ne_bytes(),
                        )))
                    }
                    libc::AF_INET6 => {
                        let sin6 = &*ifa.ifa_addr.cast::<libc::sockaddr_in6>();
                        Some(IpAddr::V6(Ipv6Addr::from(sin6.sin6_addr.s6_addr)))
                    }
                    _ => None,
                }
            };
            (ifa.ifa_next, addr.map(|a| (name, a)))
        };
        out.extend(entry);
        cur = next;
    }
    drop(list);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_that_leads_no_group_has_no_pidfd() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let t = std::thread::spawn(move || {
            // SAFETY: gettid takes no arguments, cannot fail and touches no
            // memory.
            let tid = unsafe { libc::syscall(libc::SYS_gettid) };
            tx.send(tid as u32).unwrap();
            let _ = stop_rx.recv();
        });
        let tid = rx.recv().unwrap();
        let err = pidfd_open(tid).expect_err("a thread id is not a process");
        assert!(
            matches!(err.raw_os_error(), Some(libc::ENOENT | libc::EINVAL)),
            "{err:?}"
        );
        stop_tx.send(()).unwrap();
        t.join().unwrap();
        assert!(pidfd_open(std::process::id()).is_ok());
    }

    #[test]
    fn ifaddrs_lists_the_loopback() {
        let all = ifaddrs().expect("getifaddrs");
        assert!(
            all.iter().any(|(_, a)| a.is_loopback()),
            "no loopback address among {} entries",
            all.len()
        );
    }

    #[test]
    fn the_peer_pid_and_the_boot_clock() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_pid(&a).unwrap(), std::process::id());
        assert!(boottime().unwrap() > Duration::ZERO);
    }

    #[test]
    fn a_connect_to_a_full_backlog_gives_up_in_time() {
        // Under /tmp, not $TMPDIR: a socket path must stay under 108 bytes.
        let d = std::path::PathBuf::from("/tmp").join(format!("mqs-{}", std::process::id()));
        let _ = crate::act::remove_path(&d, false);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("s.sock");
        assert_eq!(
            connect_timeout(&path, Duration::from_millis(200))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ENOENT)
        );
        let listener = UnixListener::bind(&path).unwrap();
        // SAFETY: listen on the open, bound socket `listener` owns, with a
        // backlog of 0: one connection waits, the next finds it full.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let first = connect_timeout(&path, Duration::from_millis(200)).expect("one slot");
        assert_eq!(peer_pid(&first).unwrap(), std::process::id());
        let t = std::time::Instant::now();
        let second = connect_timeout(&path, Duration::from_millis(200));
        let took = t.elapsed();
        assert!(took < Duration::from_secs(1), "{took:?}");
        let err = second.expect_err("the backlog is full");
        assert_eq!(err.raw_os_error(), Some(libc::ETIMEDOUT), "{err:?}");
        assert!(took >= Duration::from_millis(150), "it waited: {took:?}");
        // A pending connection taken frees the slot.
        let _ = listener.accept().unwrap();
        assert!(connect_timeout(&path, Duration::from_millis(200)).is_ok());
        assert!(connect_timeout(&path, Duration::ZERO).is_err());
        drop(listener);
        let _ = crate::act::remove_path(&d, false);
    }
}
