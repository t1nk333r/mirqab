//! Which process holds a socket (plan 007): a process's `proc/<pid>/fd/*`
//! links read `socket:[<inode>]`, and the tables name each socket's inode.
//! Only the user's own processes can be read; root's and other users'
//! sockets keep their uid and no process, never a guess.

use crate::root::Root;
use crate::samplers::apps::procfs::Proc;
use std::collections::{BTreeSet, HashMap};

/// The socket inodes `proc/<pid>/fd/` holds, in fd order, each once. A
/// process that exited or closed its directory to us holds none.
pub fn socket_inodes(root: &Root, pid: u32) -> Vec<u64> {
    let dir = format!("proc/{pid}/fd");
    let Ok(names) = root.read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<u64> = Vec::new();
    for n in names {
        let Ok(target) = root.readlink(&format!("{dir}/{n}")) else {
            continue;
        };
        let Some(inode) = target
            .to_str()
            .and_then(|t| t.strip_prefix("socket:["))
            .and_then(|t| t.strip_suffix(']'))
            .and_then(|t| t.parse::<u64>().ok())
        else {
            continue;
        };
        if !out.contains(&inode) {
            out.push(inode);
        }
    }
    out
}

/// The processes in `procs` that hold each inode of `wanted`, as
/// `(pid, start)`, in pid order. Every process is read: a forked server's
/// workers share their parent's socket, and the row (one per holder tree,
/// `mod.rs`) lists them all, so a stop can name each. An inode no process
/// holds is absent from the map.
pub fn owners_of(
    root: &Root,
    procs: &[&Proc],
    wanted: &BTreeSet<u64>,
) -> HashMap<u64, Vec<(u32, u64)>> {
    let mut found: HashMap<u64, Vec<(u32, u64)>> = HashMap::new();
    if wanted.is_empty() {
        return found;
    }
    let mut ordered: Vec<&&Proc> = procs.iter().collect();
    ordered.sort_by_key(|p| p.pid());
    for p in ordered {
        for inode in socket_inodes(root, p.pid()) {
            if wanted.contains(&inode) {
                found.entry(inode).or_default().push(p.key());
            }
        }
    }
    found
}
