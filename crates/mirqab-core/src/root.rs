//! `Root`: the directory that stands for `/` (architecture §5.3). Every read
//! of system state goes through it, so a test passes a fixture tree and
//! production passes `/`. This is the only file that builds a path from `/`.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Where the kernel's DRM device nodes live: an fd whose link names a
/// path under it is a GPU client's (plan 006, part 4, decision 13).
pub const DRM_DIR: &str = "/dev/dri/";

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

    /// At most the first `n` bytes of `rel` when it is a regular file, and
    /// an error for anything else: a FIFO, a socket, a device, a directory
    /// or a symlink as the last component (plan 006, part 4, decision 14).
    /// A process controls its own root, so a FIFO at `proc/<pid>/root/
    /// .flatpak-info` would block a plain open, and the read with it. The
    /// file is opened with O_NONBLOCK and O_NOFOLLOW and checked with
    /// fstat on the open descriptor, so nothing can be swapped in between
    /// a check and the read. Links before the last component stay inside a
    /// fixture tree, as in `read_prefix`.
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
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
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
}
