//! Stream names and data shapes. Each tab's plan adds its stream here and
//! pins its shape with `tests/golden/<stream>.json` (architecture §5.2);
//! `tools/check-docs.sh` reads `STREAMS` to check that every golden exists.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SELF: &str = "self";
pub const UI: &str = "ui";

/// Every stream the protocol defines so far.
pub const STREAMS: &[&str] = &["apps", "ports", "process", "self", "system", "ui"];
pub const SYSTEM: &str = "system";
pub const APPS: &str = "apps";
/// `get apps.inspect {pid}`: one process explained (plan 006). A request
/// under the `apps` stream, not a stream of its own.
pub const APPS_INSPECT: &str = "apps.inspect";
/// The `action`s that end an app or a process (plan 006, part 2): SIGTERM,
/// and SIGKILL for Force quit. Both need `confirm`.
pub const APPS_QUIT: &str = "apps.quit";
pub const APPS_KILL: &str = "apps.kill";
/// The Apps inspector's live graph of one process (plan 006, part 3):
/// `stat` and `statm` of the pid its `params` name, every second while
/// `live`. It reads nothing else and takes no action.
pub const PROCESS: &str = "process";
/// The most distinct pids the `process` stream reads in one tick.
pub const PROCESS_MAX_PIDS: usize = 8;
/// What listens on which port, who started it, and whether the network
/// can reach it (plan 007). Every 5 s, never `live`.
pub const PORTS: &str = "ports";

/// The `self` stream: what mirqab costs (plan 001).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfData {
    pub daemon: ProcessCost,
    /// None while no interface is attached.
    pub ui: Option<UiCost>,
    /// Subscribers per stream.
    pub subscribers: BTreeMap<String, u32>,
    pub uptime_sec: u64,
    /// The footer's machine line (plan 001 phase D).
    #[serde(default)]
    pub machine: Machine,
}

/// The machine the daemon runs on, for the footer: what `/proc/cpuinfo`
/// calls the CPU and how long the machine has been up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub cpu_model: Option<String>,
    pub uptime_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCost {
    /// Percent of one core since the previous sample.
    pub cpu: f64,
    pub rss_bytes: u64,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCost {
    pub cpu: f64,
    pub rss_bytes: u64,
    pub pid: u32,
}

/// The `ui` stream: events only, for the interface (plan 001).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum UiEvent {
    Show {
        tab: Option<String>,
        mode: Option<String>,
    },
    Hide,
    Toggle {
        tab: Option<String>,
    },
    ConfigChanged,
}

/// The `system` stream (plan 002). Every part may be absent: hardware
/// differs, and a reading the machine cannot give is `None`, never zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemData {
    pub cpu: Option<Cpu>,
    pub load: Option<Load>,
    pub mem: Option<Mem>,
    pub pressure: Option<Pressure>,
    pub temps: Vec<Temp>,
    pub thermal: Option<Thermal>,
    pub gpus: Vec<Gpu>,
    pub power: Option<Power>,
    pub uptime_sec: Option<u64>,
    pub boot_time: Option<i64>,
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cpu {
    /// Busy share of all cores since the previous sample; None on the first.
    pub percent: Option<f64>,
    pub user: Option<f64>,
    pub system: Option<f64>,
    pub iowait: Option<f64>,
    pub cores: Vec<Core>,
    pub mhz_avg: Option<u32>,
    pub mhz_max: Option<u32>,
    pub governor: Option<String>,
    pub epp: Option<String>,
    pub model: Option<String>,
    pub physical: Option<u32>,
    pub logical: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Core {
    pub id: u32,
    pub percent: Option<f64>,
    pub mhz: Option<u32>,
    /// `p` (performance) or `e` (efficiency) on hybrid CPUs; None otherwise.
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub runnable: u32,
    pub tasks: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mem {
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// Total − available.
    pub used_bytes: u64,
    /// The part of available memory that is cache: available − free, so
    /// used + cache + free = total.
    pub cache_bytes: u64,
    pub shmem_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub zram: Option<Zram>,
    /// KiB a second paged in and out (vmstat counts pgpgin in KiB).
    pub page_in_per_sec: Option<f64>,
    pub page_out_per_sec: Option<f64>,
    /// Pages a second swapped in and out.
    pub swap_in_per_sec: Option<f64>,
    pub swap_out_per_sec: Option<f64>,
    pub oom_kills: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Zram {
    pub orig_bytes: u64,
    pub compr_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pressure {
    pub cpu: Option<Stall>,
    pub memory: Option<Stall>,
    pub io: Option<Stall>,
}

/// Pressure stall percentages: the share of time some (or all) tasks
/// waited, averaged over 10 and 60 s by the kernel.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stall {
    pub some10: f64,
    pub some60: f64,
    pub full10: Option<f64>,
    pub full60: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Temp {
    pub id: String,
    pub label: String,
    pub celsius: f64,
    pub critical: Option<f64>,
    /// `cpu`, `gpu`, `disk`, `memory` or `other`.
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thermal {
    /// `cool`, `warm`, `hot` or `critical`.
    pub state: String,
    pub hottest: Option<String>,
    pub throttling: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Gpu {
    pub id: String,
    pub name: String,
    /// `amd`, `intel`, `nvidia` or `other`.
    pub vendor: String,
    /// Runtime-suspended: nothing else is read, so it is not woken.
    pub suspended: bool,
    pub busy_percent: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub celsius: Option<f64>,
    pub watts: Option<f64>,
    pub mhz: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Power {
    pub package_watts: Option<f64>,
    pub battery_watts: Option<f64>,
    /// `rapl`, `battery`, `both` or `none`.
    pub source: String,
    /// RAPL is there but needs root: the root helper (plan 015) would give
    /// package watts.
    pub needs_helper: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Advice {
    pub code: String,
    pub reason: String,
}

/// The `apps` stream (plan 006): one row per app, every process when asked.
/// The daemon measures once for every subscriber; each gets the groups
/// sorted, limited and expanded as its own `params` say.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsData {
    pub groups: Vec<AppGroup>,
    pub totals: AppsTotals,
    /// The order in force: `cpu`, `mem` or `name`.
    pub sort: String,
    /// The focus source the `idleHeavy` tags come from (`hyprland`);
    /// absent when there is none, so a reader can tell "not tagged" from
    /// "cannot be tagged" (plan 006, part 4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppGroup {
    /// Stable while the group lives: `scope:<unit>`, `service:<unit>`,
    /// `flatpak:<app id>`, `container:<id>`, `session:<sid>`,
    /// `tree:<pid>` or `system`.
    pub id: String,
    pub name: String,
    /// The desktop entry's `Icon=`, when the group maps to one.
    pub icon: Option<String>,
    /// `app`, `service`, `flatpak`, `container`, `terminal`, `process` or
    /// `system`.
    pub kind: String,
    /// The systemd user unit, for an `app` or `service` group.
    pub unit: Option<String>,
    /// Percent of one core since the previous sample (may pass 100); None
    /// on the first.
    pub cpu: Option<f64>,
    /// An estimate: every member's resident memory minus its shared pages,
    /// plus the largest shared part once. The inspector's PSS is exact.
    pub rss_bytes: u64,
    pub procs: u32,
    pub threads: u32,
    /// Bytes a second read from and written to storage by the members the
    /// daemon may read (the user's own); None on the first sample.
    pub io_read_per_sec: Option<f64>,
    pub io_write_per_sec: Option<f64>,
    pub pids: Vec<u32>,
    /// The member using the most CPU.
    pub top: Option<AppTop>,
    /// Every member, only in the groups a subscriber expanded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<Vec<AppProcess>>,
    /// "Idle but heavy": at least 20 % of one core on average over a full
    /// five minutes in which none of its processes had focus (plan 006,
    /// part 4). Only while a focus source runs; absent when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub idle_heavy: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppTop {
    pub pid: u32,
    pub name: String,
    pub cpu: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppProcess {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cpu: Option<f64>,
    pub rss_bytes: u64,
    pub threads: u32,
    /// The kernel's state letter: `R`, `S`, `D`, `Z`, `T`, `I`.
    pub state: String,
    /// Clock ticks after boot (field 22 of `stat`): with the pid, the
    /// process's identity, so a reused pid is another row (plan 006, D5).
    /// The value a `wouldDo` entry carries.
    pub start: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppsTotals {
    /// Groups before a view's `limit`, System included: more than
    /// `groups.len()` means the list was cut.
    pub groups: u32,
    pub procs: u32,
    pub threads: u32,
    pub cpu: Option<f64>,
    /// Logical CPUs, so a client can show percent of the machine.
    pub cores: Option<u32>,
}

/// The `process` stream (plan 006, part 3): one process, from its `stat`
/// and `statm` only. `params` are `{"pid": N}`, and optionally `"start"`
/// (the start time the subscriber knows the process by). A reading holds
/// every field, `cpu` null on the first; once the pid is gone, or holds a
/// process with another start time than the one asked for (or, without
/// `start`, the one first seen), the data is `{"pid": N, "gone": true}`.
/// `capped` means more than `PROCESS_MAX_PIDS` pids are watched and this
/// one is not read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProcessData {
    pub pid: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub capped: bool,
    /// Clock ticks after boot, as `AppProcess.start`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    /// The kernel's command name (`comm`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Percent of one core since the previous reading (may pass 100);
    /// null on the first.
    pub cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// The `ports` stream (plan 007): one row per port, protocol, bind pair and
/// owner (a tree of the user's processes, or a uid).
/// `params` are `udp` (bool: also bound UDP sockets) and `lite` (bool:
/// the counts only, [`PortsLite`]), nothing else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortsData {
    /// Sorted by port, then protocol, then pid.
    pub ports: Vec<PortRow>,
    /// Your rows (`owner` `you`) that the network can reach: root's and
    /// other users' exposed rows carry the pill but are not counted.
    pub exposed_count: u32,
    /// Your rows whose launcher's session ended (an agent that exited).
    pub left_behind_count: u32,
    /// Every row in this view, whoever's.
    pub total: u32,
    /// Whether UDP rows are in this view.
    pub udp: bool,
    /// What `docker ps` answered: `ok`, `absent`, `denied`, `failed`, or
    /// `off` while lite.
    pub docker: String,
    /// The system snapshot of root's listeners, loaded on request.
    pub system: SystemPorts,
}

/// One listening socket and the process that holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortRow {
    pub port: u16,
    /// `tcp` or `udp`.
    pub proto: String,
    /// The bound address: `127.0.0.1`, `0.0.0.0`, `::`; the IPv4 one when
    /// an IPv4 and an IPv6 socket of one owner fold into one row.
    pub bind: String,
    /// `[4]`, `[6]` or `[4, 6]`.
    pub families: Vec<u8>,
    /// `local`, `all`, `lan`, `bridge`, `vpn`, `link`, `other` or
    /// `unknown`.
    pub scope: String,
    /// Whether anything but this machine can connect: `scope != local`.
    pub exposed: bool,
    /// `you`, `root`, `other` or `docker`.
    pub owner: String,
    pub uid: Option<u32>,
    /// The process holding the socket, for your rows only, when found:
    /// the root of the tree of processes that hold it.
    pub pid: Option<u32>,
    /// Every process of that tree that holds the socket (a forked
    /// server's workers): the root first, then the rest by pid. Empty for
    /// root's and other users' rows.
    pub pids: Vec<u32>,
    /// Its start time (clock ticks after boot), with the pid its identity.
    pub start: Option<u64>,
    /// The process's name (`comm`, or the program's when cut short).
    pub process: Option<String>,
    /// The name and the first argument that says what it serves
    /// (`next dev`, `http.server`), never the whole command line.
    pub label: Option<String>,
    pub rss_bytes: Option<u64>,
    pub uptime_sec: Option<u64>,
    pub project: Option<PortProject>,
    pub shell: Option<PortShell>,
    pub launcher: Option<PortLauncher>,
    pub container: Option<ContainerRef>,
    /// The systemd user unit that runs it.
    pub unit: Option<String>,
    /// `http://localhost:<port>`, `https` for 443 and 8443; never probed.
    pub url: String,
}

/// The project folder a process runs in (plan 007, part 1b).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortProject {
    pub name: String,
    /// With `~` for the home directory.
    pub path: String,
    pub framework: Option<String>,
}

/// The nearest shell above a process (plan 007, part 1b).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortShell {
    pub name: String,
    pub pid: u32,
}

/// What started a process (plan 007, part 1b): `terminal`, `editor`,
/// `agent`, `service`, `container` or `unknown`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortLauncher {
    pub kind: String,
    pub name: Option<String>,
    pub session_ended: bool,
}

/// A container that publishes a port (plan 007, part 1b).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContainerRef {
    pub id: String,
    pub name: Option<String>,
    pub image: Option<String>,
}

/// Root's listeners with their processes, read once on request (plan 007,
/// part 2a); `loaded` false until then.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemPorts {
    pub loaded: bool,
    /// Unix seconds of the snapshot.
    pub at: Option<i64>,
    pub ports: Vec<SystemPort>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemPort {
    pub port: u16,
    pub proto: String,
    pub bind: String,
    pub families: Vec<u8>,
    pub process: Option<String>,
    pub pid: Option<u32>,
}

/// The `ports` stream with `lite: true`: counts only, for a badge, the
/// tray and a bar item. `total` counts every row of the view. While every
/// subscriber is lite the sampler skips the owner walk, so rows fold by
/// (port, proto, uid, bind) rather than by process: `total` and
/// `exposedCount` can be lower than the full view's (two processes of one
/// user on a port are one row), and `leftBehindCount` is 0, since no row has a launcher. With a
/// full subscriber too, the counts are the full view's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortsLite {
    pub lite: bool,
    pub total: u32,
    pub exposed_count: u32,
    pub left_behind_count: u32,
    pub udp: bool,
}

/// `get apps.inspect {pid}` (plan 006, "Inspector"). A field the daemon
/// could not read is left out, never an error: another user's process
/// gives name, CPU and memory only (`limited`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Inspect {
    pub pid: u32,
    pub name: String,
    /// Plain words from fixed rules, first in the inspector.
    pub verdict: Verdict,
    /// Another user's process: only name, CPU and memory are shown.
    pub limited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppid: Option<u32>,
    /// The parents up to the user manager (or pid 1), nearest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<ProcRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    /// Seconds since it started, from the machine's uptime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_sec: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_time_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cmdline: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The executable, without the kernel's ` (deleted)` suffix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    /// The file was replaced or removed since the process started; left
    /// out when it was not, or when the link cannot be read.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exe_deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<Package>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<Arch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatpak: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// The group of the `apps` stream it is counted in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<Sandbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<ProcMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<OpenFiles>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuMemory>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Verdict {
    /// `kernel`, `other-user`, `replaced`, `flatpak`, `container`,
    /// `packaged`, `home`, `unowned` or `unknown`.
    pub code: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProcRef {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Package {
    pub name: String,
    /// The sync repository that provides it; None for a package built
    /// locally or from the AUR.
    pub repo: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Arch {
    /// `x86-64`, `aarch64`, `i386`, … from the ELF header.
    pub machine: String,
    pub bits: u32,
    /// Not the daemon's own machine type: emulated (qemu-user).
    pub foreign: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sandbox {
    /// `none`, `strict` or `filter`.
    pub seccomp: Option<String>,
    pub no_new_privs: bool,
    /// Effective capabilities by name, lower case without `cap_`.
    pub capabilities: Vec<String>,
    /// The namespaces that differ from the daemon's own.
    pub namespaces: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProcMemory {
    pub rss_bytes: u64,
    /// From `smaps_rollup`, which only the owner may read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenFiles {
    pub count: u32,
    pub sockets: u32,
    /// The first 50 open files that are paths.
    pub paths: Vec<String>,
}

/// GPU memory from the DRM fdinfo keys (amdgpu: `drm-memory-*`), counted
/// once per DRM client.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GpuMemory {
    pub vram_bytes: Option<u64>,
    pub gtt_bytes: Option<u64>,
    pub cpu_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ui_events_have_the_documented_shape() {
        let show = UiEvent::Show {
            tab: Some("system".into()),
            mode: Some("popover".into()),
        };
        assert_eq!(
            serde_json::to_value(&show).unwrap(),
            json!({"event": "show", "tab": "system", "mode": "popover"})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::Hide).unwrap(),
            json!({"event": "hide"})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::Toggle { tab: None }).unwrap(),
            json!({"event": "toggle", "tab": null})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::ConfigChanged).unwrap(),
            json!({"event": "config-changed"})
        );
    }

    #[test]
    fn ui_events_match_the_golden_file() {
        let golden: Vec<UiEvent> =
            serde_json::from_str(include_str!("../../../tests/golden/ui.json")).unwrap();
        assert_eq!(
            golden,
            [
                UiEvent::Show {
                    tab: Some("system".into()),
                    mode: Some("popover".into())
                },
                UiEvent::Hide,
                UiEvent::Toggle { tab: None },
                UiEvent::ConfigChanged,
            ]
        );
    }

    #[test]
    fn stream_names_are_unique() {
        let mut names = STREAMS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), STREAMS.len());
    }
}
