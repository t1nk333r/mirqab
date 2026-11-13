//! Pressure stall information from `proc/pressure/{cpu,memory,io}`
//! (plan 002): the kernel's own 10 s and 60 s averages.

use crate::proto::stream::{Pressure, Stall};
use crate::root::Root;

pub fn parse(text: &str) -> Option<Stall> {
    let field = |line: &str, key: &str| -> Option<f64> {
        line.split_whitespace()
            .find_map(|f| f.strip_prefix(key))
            .and_then(|v| v.parse().ok())
    };
    let some = text.lines().find(|l| l.starts_with("some "))?;
    let full = text.lines().find(|l| l.starts_with("full "));
    Some(Stall {
        some10: field(some, "avg10=")?,
        some60: field(some, "avg60=")?,
        full10: full.and_then(|l| field(l, "avg10=")),
        full60: full.and_then(|l| field(l, "avg60=")),
    })
}

pub fn read(root: &Root) -> Option<Pressure> {
    let get = |name: &str| {
        root.read(&format!("proc/pressure/{name}"))
            .ok()
            .as_deref()
            .and_then(parse)
    };
    let p = Pressure {
        cpu: get("cpu"),
        memory: get("memory"),
        io: get("io"),
    };
    (p.cpu.is_some() || p.memory.is_some() || p.io.is_some()).then_some(p)
}
