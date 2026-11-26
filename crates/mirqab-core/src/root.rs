//! `Root`: the directory that stands for `/` (architecture §5.3). Every read
//! of system state goes through it, so a test passes a fixture tree and
//! production passes `/`. This is the only file that builds a path from `/`.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Where the kernel's DRM device nodes live: an fd whose link names a
/// path under it is a GPU client's (plan 006, part 4, decision 13).
pub const DRM_DIR: &str = "/dev/dri/";

/// A file's device and inode numbers: the same pair is the same file.
pub type FileId = (u64, u64);

#[derive(Debug, Clone)]
pub struct Root {
    base: PathBuf,
}

impl Root {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Root { base: base.into() }
    }

    /// The live machine.
    pub fn host() -> Self {
        Root::new("/")
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Whether this root is the live machine. Only then does the daemon
    /// signal processes or probe the compositor's socket (plan 006): a
    /// fixture's pid 2001 may be a real process of whoever runs the tests.
    pub fn is_host(&self) -> bool {
        self.base == Path::new("/")
    }

    /// The real path of `rel` ("proc/stat") under the root. Absolute paths and
    /// `..` are refused, so nothing names a file outside the tree by its text.
    /// (A symlink inside a fixture can still point out; fixtures are ours.)
    pub fn path(&self, rel: &str) -> io::Result<PathBuf> {
        let rel = Path::new(rel);
        let mut out = self.base.clone();
        for c in rel.components() {
            match c {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("path outside the root: {}", rel.display()),
                    ));
                }
            }
        }
        Ok(out)
    }

    pub fn read(&self, rel: &str) -> io::Result<String> {
        std::fs::read_to_string(self.path(rel)?)
    }

    pub fn read_bytes(&self, rel: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.path(rel)?)
    }

    /// The names in a directory, sorted.
    pub fn read_dir(&self, rel: &str) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(self.path(rel)?)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn readlink(&self, rel: &str) -> io::Result<PathBuf> {
        std::fs::read_link(self.path(rel)?)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).map(|p| p.exists()).unwrap_or(false)
    }

    /// When `rel` last changed, or None when it cannot be read.
    pub fn modified(&self, rel: &str) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.path(rel).ok()?)
            .ok()?
            .modified()
            .ok()
    }

    /// At most the first `n` bytes of `rel` (an ELF header, plan 006),
    /// following links only where they stay inside the tree. On the live
    /// machine `proc/<pid>/exe` is the process's own executable; in a
    /// fixture the same link names a path of the live machine, which must
    /// never be read, so a link that leaves a fixture tree is refused.
    pub fn read_prefix(&self, rel: &str, n: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let path = self.path(rel)?;
        if !self.is_host() {
            let real = std::fs::canonicalize(&path)?;
            let base = std::fs::canonicalize(&self.base)?;
            if !real.starts_with(&base) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("link leaves the root: {rel}"),
                ));
            }
        }
        let mut buf = Vec::with_capacity(n);
        std::fs::File::open(path)?
            .take(n as u64)
            .read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// The id of the mount `rel` resolves to, the number in field 1 of
    /// `proc/<pid>/mountinfo`. `rel` is opened with O_PATH, which calls no
    /// filesystem's open, and the id comes from `fdinfo`, which the kernel
    /// answers from the descriptor (see `pinned_mount_id`). A test uses it
    /// to write a fixture mountinfo that names the scratch tree's mount.
    pub fn mount_id(&self, rel: &str) -> io::Result<u64> {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH)
            .open(self.path(rel)?)?;
        pinned_mount_id(&f)
    }

    /// At most the first `n` bytes of `rel` when it is a regular file, and
    /// an error for anything else: a FIFO, a socket, a device, a directory
    /// or a symlink as the last component (plan 006, part 4, decision 14).
    /// A process controls its own root, so a FIFO at `proc/<pid>/root/
    /// .flatpak-info` would block a plain open, and the read with it. The
    /// file is opened with O_NONBLOCK and O_NOFOLLOW and checked with
    /// fstat on the open descriptor, so nothing can be swapped in between
    /// a check and the read; O_NOCTTY too, because the daemon is a session
    /// leader without a terminal, and a planted tty would otherwise become
    /// its controlling terminal on open. Links before the last component
    /// stay inside a fixture tree, as in `read_prefix`.
    pub fn read_regular_prefix(&self, rel: &str, n: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let path = self.path(rel)?;
        if !self.is_host() {
            let parent = path.parent().unwrap_or(&self.base);
            let real = std::fs::canonicalize(parent)?;
            let base = std::fs::canonicalize(&self.base)?;
            if !real.starts_with(&base) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("link leaves the root: {rel}"),
                ));
            }
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
            .open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a regular file: {rel}"),
            ));
        }
        let mut buf = Vec::with_capacity(n.min(64 * 1024));
        file.take(n as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// At most the first `n` bytes of `rel` and the file's device and
    /// inode, when it resolves to a regular file on a mount `clear` accepts
    /// by id. Links are followed (a Flatpak export is one, and
    /// `proc/<pid>/exe` is the reason for this function), and nothing on an
    /// uncleared mount is opened (plan 024, C5, C7 and C10):
    ///
    /// 1. `rel` is opened with O_PATH, which runs no filesystem's open; a
    ///    FIFO or a device opened so does not block either. Following
    ///    `proc/<pid>/exe` jumps to the executable's dentry and looks
    ///    nothing up.
    /// 2. Its mount id goes to `clear`, which reads its state after the
    ///    pin, as in `read_regular_prefix_in`.
    /// 3. Only then is it checked to be a regular file (fstat on FUSE may
    ///    ask its server, so not before) and reopened for reading through
    ///    the pinned descriptor, with O_NONBLOCK and O_NOCTTY as in
    ///    `read_regular_prefix`.
    ///
    /// A path walk that crosses a mount still looks each name up, so this
    /// guards the file a link names, not every directory on the way to it.
    /// On a fixture root a link that leaves the tree is refused, as in
    /// `read_prefix`.
    pub fn read_regular_prefix_cleared(
        &self,
        rel: &str,
        n: usize,
        clear: impl Fn(u64) -> bool,
    ) -> io::Result<(Vec<u8>, FileId)> {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let path = self.path(rel)?;
        if !self.is_host() {
            let real = std::fs::canonicalize(&path)?;
            let base = std::fs::canonicalize(&self.base)?;
            if !real.starts_with(&base) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("link leaves the root: {rel}"),
                ));
            }
        }
        let open = |p: &Path, flags: libc::c_int| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(p)
        };
        let pinned = open(&path, libc::O_PATH)?;
        if !clear(pinned_mount_id(&pinned)?) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{rel} is not on a cleared mount"),
            ));
        }
        let meta = pinned.metadata()?;
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a regular file: {rel}"),
            ));
        }
        let reader = open(
            Path::new(&format!("/proc/self/fd/{}", pinned.as_raw_fd())),
            libc::O_NONBLOCK | libc::O_NOCTTY,
        )?;
        let mut buf = Vec::with_capacity(n.min(64 * 1024));
        reader.take(n as u64).read_to_end(&mut buf)?;
        Ok((buf, (meta.dev(), meta.ino())))
    }

    /// The device and inode `rel` resolves to, or None when it cannot be
    /// read: whether two paths name one file.
    pub fn file_id(&self, rel: &str) -> Option<FileId> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(self.path(rel).ok()?).ok()?;
        Some((m.dev(), m.ino()))
    }

    /// The device and inode of `rel`, found one component at a time so
    /// that no name is looked up in a directory on a mount `clear` does not
    /// accept by id (plan 024, review of C11). The path can come from a
    /// process in another mount namespace, which chooses it, and a lookup
    /// or a `stat` on a FUSE mount waits on its server for as long as it
    /// likes:
    ///
    /// 1. The root is opened with O_PATH and its mount id goes to `clear`.
    /// 2. Each component is opened O_PATH|O_NOFOLLOW through the pinned
    ///    descriptor of the directory before it (`/proc/self/fd/N/name`),
    ///    which was cleared already; its own mount id is cleared next,
    ///    since a lookup may cross onto a mount placed on it.
    /// 3. Only a cleared descriptor is asked for its metadata. A component
    ///    before the last must be a directory, so a symlink there is
    ///    refused, not followed; a symlink as the last gives its own id.
    ///
    /// O_DIRECTORY is not passed, so an automount point is not triggered.
    /// `clear` reads its state at each call, for the reason
    /// `read_regular_prefix_in` gives.
    pub fn file_id_cleared(&self, rel: &str, clear: impl Fn(u64) -> bool) -> io::Result<FileId> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let invalid = |what: String| io::Error::new(io::ErrorKind::InvalidInput, what);
        let mut parts = Vec::new();
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(part) => parts.push(part),
                Component::CurDir => {}
                _ => return Err(invalid(format!("not a plain path: {rel}"))),
            }
        }
        if parts.is_empty() {
            return Err(invalid(format!("no name: {rel:?}")));
        }
        let open = |p: &Path, flags: libc::c_int| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(p)
        };
        let check = |f: &std::fs::File| -> io::Result<()> {
            if clear(pinned_mount_id(f)?) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{rel} crosses an uncleared mount"),
                ))
            }
        };
        let mut cur = open(&self.base, libc::O_PATH | libc::O_DIRECTORY)?;
        check(&cur)?;
        let mut meta = cur.metadata()?;
        for part in parts {
            if !meta.is_dir() {
                return Err(invalid(format!("not a directory on the way to {rel}")));
            }
            let at = PathBuf::from(format!("/proc/self/fd/{}", cur.as_raw_fd())).join(part);
            cur = open(&at, libc::O_PATH | libc::O_NOFOLLOW)?;
            check(&cur)?;
            meta = cur.metadata()?;
        }
        Ok((meta.dev(), meta.ino()))
    }

    /// At most the first `n` bytes of `name` inside the directory `dir`
    /// resolves to (`proc/<pid>/root`), when it is a regular file on a
    /// mount `clear` accepts by id (audit 2026-11-21, L7). A process
    /// controls its own root, and an open or a lookup on a FUSE mount waits
    /// on its server, O_NONBLOCK or not, so nothing on an uncleared mount
    /// is asked anything:
    ///
    /// 1. `dir` is opened with O_PATH. Following `proc/<pid>/root` jumps to
    ///    the root's dentry and calls no filesystem; it is also the root
    ///    itself, where a mount placed over it is not (a chroot root does
    ///    not follow an overmount).
    /// 2. Its mount id goes to `clear` before `name` is looked up in it.
    /// 3. `name`, one component, is opened O_PATH|O_NOFOLLOW through that
    ///    pinned descriptor (`/proc/self/fd/N/name`), and its mount id is
    ///    cleared too, since a file can be a mount of its own.
    /// 4. Only then is it checked to be a regular file (no FIFO, socket,
    ///    device, directory or symlink) and reopened for reading through
    ///    its own pinned descriptor, with O_NONBLOCK and O_NOCTTY as in
    ///    `read_regular_prefix`.
    ///
    /// `clear` is called only after the descriptor it is asked about is
    /// pinned, and it must decide from state it reads at that call, not
    /// from a copy taken earlier: Linux reuses a freed mount id at once, so
    /// an earlier listing can name another mount by the same id, while a
    /// listing read after the pin cannot (the descriptor keeps its mount,
    /// and the id, alive). Given that, a `pivot_root`, a mount or an
    /// unmount at any point leaves the read on the objects that were
    /// cleared. On a fixture root the descriptors are real too, and `dir`
    /// must stay inside the tree.
    pub fn read_regular_prefix_in(
        &self,
        dir: &str,
        name: &str,
        n: usize,
        clear: impl Fn(u64) -> bool,
    ) -> io::Result<Vec<u8>> {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not one name: {name:?}"),
            ));
        }
        let path = self.path(dir)?;
        if !self.is_host() {
            let real = std::fs::canonicalize(&path)?;
            let base = std::fs::canonicalize(&self.base)?;
            if !real.starts_with(&base) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("link leaves the root: {dir}"),
                ));
            }
        }
        let open = |p: &Path, flags: libc::c_int| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(p)
        };
        let check = |f: &std::fs::File, what: &str| -> io::Result<u64> {
            let id = pinned_mount_id(f)?;
            if clear(id) {
                Ok(id)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{what} is not on a cleared mount"),
                ))
            }
        };
        let dir_fd = open(&path, libc::O_PATH | libc::O_DIRECTORY)?;
        check(&dir_fd, dir)?;
        let fd_path = |f: &std::fs::File| PathBuf::from(format!("/proc/self/fd/{}", f.as_raw_fd()));
        let file = open(
            &fd_path(&dir_fd).join(name),
            libc::O_PATH | libc::O_NOFOLLOW,
        )?;
        check(&file, name)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a regular file: {dir}/{name}"),
            ));
        }
        let reader = open(&fd_path(&file), libc::O_NONBLOCK | libc::O_NOCTTY)?;
        let mut buf = Vec::with_capacity(n.min(64 * 1024));
        reader.take(n as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }
}

/// The id of the mount the descriptor `f` sits on, from the `mnt_id:`
/// line of this process's `fdinfo` for it (Linux 3.15 and later). The
/// kernel prints it from the descriptor itself: unlike `fstatfs`, or a
/// `statx` that may refresh attributes, no filesystem is asked, so a hung
/// FUSE server cannot hold the call. These are our own descriptors, not
/// state of the machine, so they are read here on a fixture root too.
fn pinned_mount_id(f: &std::fs::File) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", f.as_raw_fd()))?;
    info.lines()
        .find_map(|l| l.strip_prefix("mnt_id:"))
        .and_then(|v| v.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "fdinfo has no mnt_id"))
}

#[cfg(test)]
mod tests {
    use super::Root;

    #[test]
    fn joins_relative_paths() {
        let r = Root::new("/tmp/fixture");
        assert_eq!(
            r.path("proc/stat").unwrap().to_str(),
            Some("/tmp/fixture/proc/stat")
        );
        assert_eq!(
            r.path("./proc/./meminfo").unwrap().to_str(),
            Some("/tmp/fixture/proc/meminfo")
        );
    }

    #[test]
    fn refuses_escapes() {
        let r = Root::new("/tmp/fixture");
        assert!(r.path("../etc/passwd").is_err());
        assert!(r.path("proc/../../x").is_err());
        assert!(r.path("/etc/passwd").is_err());
    }

    /// A scratch tree under `${TMPDIR:-/tmp}`, removed on drop (through
    /// `act`, the one module that removes files).
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = crate::act::remove_path(&self.0, false);
        }
    }
    fn scratch(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!(
            "mirqab-root-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = crate::act::remove_path(&d, false);
        std::fs::create_dir_all(d.join("proc/1/root")).unwrap();
        Scratch(d)
    }

    #[test]
    fn a_pinned_read_asks_about_the_directory_mount_before_the_name() {
        // Audit 2026-11-21, L7: the directory's mount is cleared before
        // anything inside it is looked up, so a refusal comes before the
        // lookup that a FUSE server would answer: a missing name is
        // refused, not "not found".
        let s = scratch("pin-order");
        let r = Root::new(&s.0);
        let mnt = r.mount_id("proc/1/root").unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let e = r
            .read_regular_prefix_in("proc/1/root", "absent", 16, |id| {
                asked.borrow_mut().push(id);
                false
            })
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert_eq!(*asked.borrow(), [mnt]);
    }

    #[test]
    fn a_pinned_read_asks_about_what_it_already_holds() {
        // Review of audit 2026-11-21, L7: `clear` runs after the descriptor
        // is pinned, and the read goes through that descriptor. Here the
        // first call swaps the directory for another; the read still gets
        // the file of the directory that was asked about.
        let s = scratch("pin-swap");
        let dir = s.0.join("proc/1/root");
        std::fs::write(dir.join("info"), "asked").unwrap();
        let other = s.0.join("proc/1/other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("info"), "swapped").unwrap();
        let r = Root::new(&s.0);
        let calls = std::cell::Cell::new(0);
        let got = r.read_regular_prefix_in("proc/1/root", "info", 16, |_| {
            if calls.replace(calls.get() + 1) == 0 {
                std::fs::rename(&dir, s.0.join("proc/1/old")).unwrap();
                std::fs::rename(&other, &dir).unwrap();
            }
            true
        });
        assert_eq!(got.unwrap(), b"asked");
        assert_eq!(calls.get(), 2, "the directory and the file");
    }

    #[test]
    fn a_pinned_read_reads_a_regular_file_on_a_cleared_mount() {
        let s = scratch("pin-read");
        std::fs::write(s.0.join("proc/1/root/info"), "abcdef").unwrap();
        let r = Root::new(&s.0);
        let mnt = r.mount_id("proc/1/root").unwrap();
        assert_eq!(r.mount_id("proc/1/root/info").unwrap(), mnt);
        let got = r.read_regular_prefix_in("proc/1/root", "info", 4, |id| id == mnt);
        assert_eq!(got.unwrap(), b"abcd");
    }

    #[test]
    fn a_pinned_read_refuses_what_is_not_a_regular_file() {
        let s = scratch("pin-kinds");
        let dir = s.0.join("proc/1/root");
        let fifo = dir.join("fifo");
        let made = crate::exec::run(
            &["mkfifo", &fifo.to_string_lossy()],
            None,
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert!(made.success(), "{made:?}");
        std::fs::write(dir.join("real"), "x").unwrap();
        std::os::unix::fs::symlink("real", dir.join("link")).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/real"), "x").unwrap();
        let r = Root::new(&s.0);
        for name in ["fifo", "link", "sub", "sub/real", "..", ".", ""] {
            let e = r
                .read_regular_prefix_in("proc/1/root", name, 4, |_| true)
                .unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{name}: {e}");
        }
    }

    #[test]
    fn a_cleared_read_opens_nothing_on_a_refused_mount() {
        // Plan 024, C5 and C7: the link is pinned with O_PATH and its
        // mount refused before anything is opened. A plain open of a FIFO
        // with no writer blocks, so an answer at all proves it was not
        // opened; with the mount cleared it is still no regular file.
        let s = scratch("cleared-fifo");
        let dir = s.0.join("proc/1");
        let fifo = s.0.join("fifo");
        let made = crate::exec::run(
            &["mkfifo", &fifo.to_string_lossy()],
            None,
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert!(made.success(), "{made:?}");
        std::os::unix::fs::symlink(&fifo, dir.join("exe")).unwrap();
        let r = Root::new(&s.0);
        let mnt = r.mount_id("proc/1").unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let e = r
            .read_regular_prefix_cleared("proc/1/exe", 20, |id| {
                asked.borrow_mut().push(id);
                false
            })
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert_eq!(*asked.borrow(), [mnt]);
        let e = r
            .read_regular_prefix_cleared("proc/1/exe", 20, |_| true)
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{e}");
    }

    #[test]
    fn a_cleared_read_follows_a_link_to_a_regular_file() {
        let s = scratch("cleared-read");
        std::fs::write(s.0.join("program"), "abcdef").unwrap();
        std::os::unix::fs::symlink(s.0.join("program"), s.0.join("proc/1/exe")).unwrap();
        let r = Root::new(&s.0);
        let (head, id) = r
            .read_regular_prefix_cleared("proc/1/exe", 4, |_| true)
            .unwrap();
        assert_eq!(head, b"abcd");
        assert_eq!(Some(id), r.file_id("program"));
        assert_ne!(Some(id), r.file_id("proc/1"));
        // A link out of a fixture tree is not followed.
        let out = scratch("cleared-out");
        std::fs::write(out.0.join("program"), "x").unwrap();
        std::os::unix::fs::symlink(out.0.join("program"), s.0.join("proc/2")).unwrap();
        let e = r
            .read_regular_prefix_cleared("proc/2", 4, |_| true)
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
    }

    #[test]
    fn a_cleared_file_id_asks_about_each_mount_before_looking_inside() {
        // Plan 024, review of C11: the host's path comes from another mount
        // namespace, and a lookup on a hung FUSE mount waits for good, so
        // each directory's mount is cleared before a name is looked up in
        // it. A refused root is refused before the (missing) name is looked
        // up: the error is a refusal, not "not found", and only the root's
        // mount was asked about.
        let s = scratch("id-order");
        std::fs::create_dir_all(s.0.join("usr/bin")).unwrap();
        std::fs::write(s.0.join("usr/bin/tool"), "x").unwrap();
        let r = Root::new(&s.0);
        let mnt = r.mount_id("usr").unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let e = r
            .file_id_cleared("absent/tool", |id| {
                asked.borrow_mut().push(id);
                false
            })
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert_eq!(*asked.borrow(), [mnt]);
        // Cleared, every component is asked about: the root, usr, bin and
        // tool, and the id is the file's.
        asked.borrow_mut().clear();
        let got = r.file_id_cleared("usr/bin/tool", |id| {
            asked.borrow_mut().push(id);
            true
        });
        assert_eq!(got.unwrap(), r.file_id("usr/bin/tool").unwrap());
        assert_eq!(asked.borrow().len(), 4);
    }

    #[test]
    fn a_cleared_file_id_follows_no_symlink_on_the_way() {
        let s = scratch("id-link");
        std::fs::create_dir_all(s.0.join("usr/bin")).unwrap();
        std::fs::write(s.0.join("usr/bin/tool"), "x").unwrap();
        std::os::unix::fs::symlink("usr/bin", s.0.join("bin")).unwrap();
        std::os::unix::fs::symlink("tool", s.0.join("usr/bin/alias")).unwrap();
        let r = Root::new(&s.0);
        let tool = r.file_id("usr/bin/tool").unwrap();
        let e = r.file_id_cleared("bin/tool", |_| true).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{e}");
        // The last component names itself, so a link there is not the file.
        assert_ne!(r.file_id_cleared("usr/bin/alias", |_| true).unwrap(), tool);
        assert!(r.file_id_cleared("usr/../usr/bin/tool", |_| true).is_err());
        assert!(r.file_id_cleared("/usr/bin/tool", |_| true).is_err());
    }

    #[test]
    fn a_pinned_read_does_not_leave_a_fixture_tree() {
        let s = scratch("pin-out");
        let out = scratch("pin-out-target");
        std::fs::write(out.0.join("info"), "x").unwrap();
        std::fs::create_dir(s.0.join("proc/2")).unwrap();
        std::os::unix::fs::symlink(&out.0, s.0.join("proc/2/root")).unwrap();
        let e = Root::new(&s.0)
            .read_regular_prefix_in("proc/2/root", "info", 4, |_| true)
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
    }

    /// Set in the environment of the child that `opening_a_tty_does_not_
    /// make_it_the_controlling_terminal` starts.
    const NOCTTY_CHILD: &str = "MIRQAB_TEST_NOCTTY_CHILD";

    #[test]
    fn opening_a_tty_does_not_make_it_the_controlling_terminal() {
        // Audit 2026-11-21, L6: the daemon is a session leader without a
        // terminal under systemd, so a tty a process planted at its
        // `.flatpak-info` would become the daemon's controlling terminal
        // when opened without O_NOCTTY. That takes a session leader, which
        // this test process is not and must not become, so a child copy of
        // this binary does it: it starts a session and reports back.
        let exe = std::env::current_exe().unwrap();
        let out = crate::exec::run(
            &[
                "env",
                &format!("{NOCTTY_CHILD}=1"),
                &exe.to_string_lossy(),
                "--exact",
                "root::tests::noctty_child",
                "--ignored",
                "--test-threads=1",
            ],
            None,
            std::time::Duration::from_secs(20),
        )
        .unwrap();
        assert!(
            out.success() && out.stdout.contains("1 passed"),
            "status {:?}\n{}{}",
            out.status,
            out.stdout,
            out.stderr
        );
    }

    #[test]
    #[ignore = "run only as the child of the test above"]
    // Two libc calls with no safe wrapper in std; root.rs is one of the
    // files allowed `unsafe` (CLAUDE.md), and only this test uses it.
    #[allow(unsafe_code)]
    fn noctty_child() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        if std::env::var_os(NOCTTY_CHILD).is_none() {
            return;
        }
        // SAFETY: setsid takes no arguments and touches no memory; this
        // process was spawned without a process group of its own, so it is
        // not a group leader and the call succeeds.
        assert_ne!(unsafe { libc::setsid() }, -1, "setsid");
        let ptmx = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open("/dev/ptmx")
        {
            Ok(f) => f,
            // A container or build sandbox without pseudo-terminals: there
            // is no tty to plant, so nothing to check.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => panic!("a pseudo-terminal: {e}"),
        };
        let unlock: libc::c_int = 0;
        let mut n: libc::c_uint = 0;
        // SAFETY: both ioctls take a pointer to an int that lives across
        // the call, on a descriptor this function owns.
        unsafe {
            assert_eq!(libc::ioctl(ptmx.as_raw_fd(), libc::TIOCSPTLCK, &unlock), 0);
            assert_eq!(libc::ioctl(ptmx.as_raw_fd(), libc::TIOCGPTN, &mut n), 0);
        }
        let r = Root::host().read_regular_prefix(&format!("dev/pts/{n}"), 16);
        assert!(r.is_err(), "a tty is not a regular file");
        let tty = std::fs::File::open("/dev/tty");
        assert!(
            tty.is_err(),
            "the pseudo-terminal became the controlling terminal"
        );
    }
}
