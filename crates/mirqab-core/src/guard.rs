//! What an acting tool must never touch beyond architecture §5.4's
//! refusals (plan 005, rule 4): the MCP server's own ancestry, so an agent
//! cannot kill itself, its terminal or its editor by accident. Read through
//! `Root`, so the walk is tested on fixture trees.

use crate::root::Root;

/// The deepest walk: a loop in a broken tree must end.
const MAX_DEPTH: usize = 64;

/// `pid` and its ancestors up to (not including) pid 1, nearest first.
pub fn ancestry(root: &Root, pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cur = pid;
    while cur > 1 && out.len() < MAX_DEPTH && !out.contains(&cur) {
        out.push(cur);
        match parent(root, cur) {
            Some(p) => cur = p,
            None => break,
        }
    }
    out
}

/// The parent pid from `proc/<pid>/stat`: the field after the command,
/// whose parentheses may hold spaces and parentheses.
fn parent(root: &Root, pid: u32) -> Option<u32> {
    let stat = root.read(&format!("proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act;

    fn tree(name: &str, procs: &[(u32, &str, u32)]) -> (Root, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("mirqab-guard-{name}-{}", std::process::id()));
        let _ = act::remove_path(&d, false);
        for (pid, comm, ppid) in procs {
            let stat = format!("{pid} ({comm}) S {ppid} 1 1 0 -1\n");
            act::write_file(&d.join(format!("proc/{pid}/stat")), stat.as_bytes(), false).unwrap();
        }
        (Root::new(&d), d)
    }

    #[test]
    fn the_walk_stops_at_init_and_reads_odd_command_names() {
        let (root, d) = tree(
            "walk",
            &[
                (900, "mirqab", 850),
                (850, "node (agent) x", 400),
                (400, "zsh", 1),
            ],
        );
        assert_eq!(ancestry(&root, 900), [900, 850, 400]);
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_loop_or_a_missing_parent_ends_the_walk() {
        let (root, d) = tree("loop", &[(10, "a", 11), (11, "b", 10)]);
        assert_eq!(ancestry(&root, 10), [10, 11]);
        assert_eq!(ancestry(&root, 77), [77], "no stat: the pid itself");
        act::remove_path(&d, false).unwrap();
    }
}
