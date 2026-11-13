//! The `system` sampler on fixture roots (plan 002, Tests): rates from the
//! `.t1` twin, the thermal word at each threshold, throttling, hybrid
//! cores, a suspended GPU that is never read, battery power, the restart
//! advice, and a minimal machine where every missing file is `None`.

use mirqab_core::Root;
use mirqab_core::proto::stream::{SystemData, Temp};
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::system::{System, temps};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
}

fn data(v: serde_json::Value) -> SystemData {
    serde_json::from_value(v).expect("the stream's shape")
}

const WALL: u64 = 1_800_000_000;

#[test]
fn a_minute_of_counters_gives_the_rates_and_series() {
    let t0 = Instant::now();
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    // The first minute_at only sets the baseline at a known wall time.
    s.minute_at(&fixture("root-desktop"), WALL, t0);
    let m = s.minute_at(
        &fixture("root-desktop.t1"),
        WALL + 60,
        t0 + Duration::from_secs(60),
    );
    let get = |k: &str| m.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    // Each of 16 CPUs: +150 user, +50 system, +800 idle: 20 % busy.
    assert_eq!(get("cpu.total"), Some(2000));
    assert_eq!(get("cpu.user"), Some(1500));
    assert_eq!(get("cpu.system"), Some(500));
    assert_eq!(get("cpu.iowait"), Some(0));
    assert_eq!(get("load.one"), Some(150));
    assert_eq!(get("page.in"), Some(600));
    assert_eq!(get("page.out"), Some(1200));
    assert_eq!(get("swap.in"), Some(6));
    assert_eq!(get("swap.out"), Some(12));
    // 3.6 kJ over 60 s.
    assert_eq!(get("power.package"), Some(600));
    assert_eq!(get("psi.memory"), Some(20));
    assert_eq!(get("temp.cpu"), Some(520));
    assert_eq!(get("temp.gpu"), Some(450));
    assert_eq!(get("gpu.0.busy"), Some(0));
    assert_eq!(get("gpu.1.busy"), Some(300));
    assert_eq!(get("gpu.1.vram"), Some(1_073_741_824));
    assert_eq!(get("cpu.mhz"), Some(3968));
    // Cache is available − free, so used + cache + free is the total.
    assert_eq!(get("mem.cache"), Some((48_000_000 - 24_000_000) * 1024));
    for (k, _) in &m {
        assert!(
            mirqab_core::history::SERIES.iter().any(|s| s.0 == *k),
            "{k} is not a known series"
        );
    }
}

#[test]
fn live_samples_give_shares_and_their_means_fill_the_minute() {
    let t0 = Instant::now();
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let first = data(s.sample(&fixture("root-desktop"), t0));
    assert_eq!(first.cpu.as_ref().unwrap().percent, None, "no baseline yet");
    let second = data(s.sample(&fixture("root-desktop.t1"), t0 + Duration::from_secs(2)));
    let cpu = second.cpu.unwrap();
    assert_eq!(cpu.percent, Some(20.0));
    assert_eq!(cpu.cores.len(), 16);
    assert!(cpu.cores.iter().all(|c| c.percent.is_some()));
    // 600 KiB over 2 s.
    assert_eq!(second.mem.unwrap().page_in_per_sec, Some(300.0));
    // A baseline older than ten seconds is dropped, not averaged.
    let stale = data(s.sample(&fixture("root-desktop"), t0 + Duration::from_secs(60)));
    assert_eq!(stale.cpu.unwrap().percent, None);
}

#[test]
fn get_reads_against_the_minute_baseline() {
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let t0 = Instant::now();
    let snap = data(s.snapshot(&fixture("root-desktop.t1"), t0 + Duration::from_secs(30)));
    assert_eq!(snap.cpu.unwrap().percent, Some(20.0));
    // The live baseline did not move: the next live sample has none.
    let live = data(s.sample(&fixture("root-desktop.t1"), Instant::now()));
    assert_eq!(live.cpu.unwrap().percent, None);
}

#[test]
fn get_never_averages_over_a_stale_baseline() {
    // History off or after a sleep: the probe's baseline is 600 s old.
    // `get` takes its own second reading instead (here the same counters,
    // so no rate), never the 600 s average (review of PR #7).
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let late = Instant::now() + Duration::from_secs(600);
    let snap = data(s.snapshot(&fixture("root-desktop.t1"), late));
    assert_eq!(snap.cpu.unwrap().percent, None);
}

#[test]
fn the_first_live_event_reads_against_a_fresh_minute_baseline() {
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let first = data(s.sample(
        &fixture("root-desktop.t1"),
        Instant::now() + Duration::from_secs(30),
    ));
    assert_eq!(first.cpu.unwrap().percent, Some(20.0));
}

#[test]
fn a_stale_minute_baseline_gives_the_first_event_no_rate() {
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let late = data(s.sample(
        &fixture("root-desktop.t1"),
        Instant::now() + Duration::from_secs(600),
    ));
    assert_eq!(late.cpu.unwrap().percent, None);
}

#[test]
fn gpu_history_keys_follow_the_card_not_the_awake_count() {
    // card0 suspends between a live sample and the minute: its slot stays
    // empty and card1 keeps gpu.1 (review of PR #7).
    let d = scratch("gpukeys", "root-desktop");
    let root = Root::new(&d.0);
    let mut s = System::new();
    s.probe(&root).unwrap();
    let t0 = Instant::now();
    s.minute_at(&root, WALL, t0);
    s.sample(&root, t0 + Duration::from_secs(10));
    let power = d.0.join("sys/class/drm/card0/device/power");
    std::fs::create_dir_all(&power).unwrap();
    std::fs::write(power.join("runtime_status"), "suspended\n").unwrap();
    let m = s.minute_at(&root, WALL + 60, t0 + Duration::from_secs(60));
    let get = |k: &str| m.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    assert_eq!(get("gpu.0.busy"), None);
    assert_eq!(get("gpu.0.vram"), None);
    assert_eq!(get("gpu.1.busy"), Some(300));
    assert_eq!(get("gpu.1.vram"), Some(1_073_741_824));
}

#[test]
fn a_card_without_a_pci_vendor_is_not_a_gpu() {
    let d = scratch("virtual", "root-desktop");
    std::fs::create_dir_all(d.0.join("sys/class/drm/card9/device")).unwrap();
    let root = Root::new(&d.0);
    assert_eq!(
        mirqab_core::samplers::system::gpu::cards(&root),
        ["card0", "card1"]
    );
}

#[test]
fn nvidia_smi_never_wakes_a_suspended_card() {
    use mirqab_core::samplers::system::gpu::nvidia_wanted;
    let d = scratch("nvidia", "root-laptop");
    let root = Root::new(&d.0);
    assert!(!nvidia_wanted(&root), "no NVIDIA card");
    std::fs::write(d.0.join("sys/class/drm/card1/device/vendor"), "0x10de\n").unwrap();
    assert!(!nvidia_wanted(&root), "the NVIDIA card is suspended");
    std::fs::write(
        d.0.join("sys/class/drm/card1/device/power/runtime_status"),
        "active\n",
    )
    .unwrap();
    assert!(nvidia_wanted(&root));
}

#[test]
fn power_needs_the_helper_only_where_rapl_is_unreadable() {
    use mirqab_core::samplers::system::power;
    assert!(!power::needs_helper(&fixture("root-desktop")), "readable");
    assert!(!power::needs_helper(&fixture("root-minimal")), "no RAPL");
    // A directory in place of the counter: it exists and cannot be read,
    // even by root.
    let d = scratch("rapl", "root-minimal");
    std::fs::create_dir_all(d.0.join("sys/class/powercap/intel-rapl:0/energy_uj")).unwrap();
    assert!(power::needs_helper(&Root::new(&d.0)));
}

#[test]
fn a_signed_discharge_current_is_still_a_draw() {
    use mirqab_core::samplers::system::power;
    let d = scratch("battery", "root-laptop");
    std::fs::write(
        d.0.join("sys/class/power_supply/BAT0/power_now"),
        "-9000000\n",
    )
    .unwrap();
    assert_eq!(power::battery(&Root::new(&d.0)), Some(9.0));
}

#[test]
fn cpulist_ranges_and_an_overflowing_one() {
    use mirqab_core::samplers::system::cpu::cpulist;
    assert_eq!(cpulist("0-3,8,10-11\n"), [0, 1, 2, 3, 8, 10, 11]);
    assert_eq!(cpulist("4294967295-4294967295"), [u32::MAX]);
}

#[test]
fn a_hybrid_laptop_with_a_suspended_dgpu_and_a_battery() {
    let mut s = System::new();
    let root = fixture("root-laptop");
    s.probe(&root).unwrap();
    let d = data(s.sample(&root, Instant::now()));
    let cpu = d.cpu.unwrap();
    let kinds: Vec<Option<&str>> = cpu.cores.iter().map(|c| c.kind.as_deref()).collect();
    assert_eq!(&kinds[..4], [Some("p"); 4]);
    assert_eq!(&kinds[4..], [Some("e"); 4]);
    assert_eq!(cpu.mhz_max, Some(4700));
    assert_eq!(d.gpus.len(), 2);
    assert_eq!(d.gpus[0].vendor, "intel");
    assert_eq!(d.gpus[0].mhz, Some(800));
    assert!(d.gpus[1].suspended);
    assert_eq!(d.gpus[1].busy_percent, None, "a suspended card is not read");
    assert!(
        !d.temps.iter().any(|t| t.id.starts_with("amdgpu")),
        "nor is its sensor chip under class/hwmon"
    );
    let p = d.power.unwrap();
    assert_eq!(p.battery_watts, Some(9.0));
    assert_eq!(p.source, "battery");
    // coretemp at 91 of 100: hot.
    assert_eq!(d.thermal.unwrap().state, "hot");
}

fn temp(kind: &str, celsius: f64, critical: Option<f64>) -> Temp {
    Temp {
        id: format!("{kind}/t"),
        label: String::new(),
        celsius,
        critical,
        kind: kind.to_string(),
    }
}

#[test]
fn the_thermal_word_at_each_threshold() {
    let word = |c: f64| {
        temps::thermal(&[temp("cpu", c, Some(100.0))], false)
            .unwrap()
            .state
    };
    assert_eq!(word(59.9), "cool");
    assert_eq!(word(60.0), "warm");
    assert_eq!(word(79.9), "warm");
    assert_eq!(word(80.0), "hot");
    assert_eq!(word(94.9), "hot");
    assert_eq!(word(95.0), "critical");
    // No critical: 95 °C is assumed. A hot disk does not count.
    let t = temps::thermal(
        &[temp("cpu", 50.0, None), temp("disk", 90.0, Some(70.0))],
        false,
    )
    .unwrap();
    assert_eq!(
        (t.state.as_str(), t.hottest.as_deref()),
        ("cool", Some("cpu/t"))
    );
    assert!(temps::thermal(&[temp("disk", 40.0, None)], false).is_none());
}

/// A scratch root copied from `from`, removed on drop.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn scratch(name: &str, from: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("mirqab-system-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy_tree(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(from),
        &d,
    );
    Scratch(d)
}

#[test]
fn a_rising_throttle_counter_is_throttling() {
    let d = scratch("throttle", "root-laptop");
    let root = Root::new(&d.0);
    let mut s = System::new();
    s.probe(&root).unwrap();
    let t0 = Instant::now();
    assert!(!data(s.sample(&root, t0)).thermal.unwrap().throttling);
    std::fs::write(
        d.0.join("sys/devices/system/cpu/cpu2/thermal_throttle/core_throttle_count"),
        "9\n",
    )
    .unwrap();
    let after = data(s.sample(&root, t0 + Duration::from_secs(2)));
    assert!(after.thermal.unwrap().throttling);
    // Without counters (AMD): clock under 60 % of max near critical.
    assert!(temps::throttling_by_clock(
        &[temp("cpu", 92.0, None)],
        Some(2000),
        Some(5000)
    ));
    assert!(!temps::throttling_by_clock(
        &[temp("cpu", 70.0, None)],
        Some(2000),
        Some(5000)
    ));
    assert!(!temps::throttling_by_clock(
        &[temp("cpu", 92.0, None)],
        Some(4000),
        Some(5000)
    ));
}

#[test]
fn restart_advice_after_a_month_or_a_strained_week() {
    let d = scratch("advice", "root-desktop");
    let root = Root::new(&d.0);
    let advice = |uptime: u64, swap_free_kib: u64| {
        std::fs::write(d.0.join("proc/uptime"), format!("{uptime}.00 0.00\n")).unwrap();
        let meminfo = std::fs::read_to_string(d.0.join("proc/meminfo")).unwrap();
        let meminfo: String = meminfo
            .lines()
            .map(|l| {
                if l.starts_with("SwapFree:") {
                    format!("SwapFree: {swap_free_kib} kB\n")
                } else {
                    format!("{l}\n")
                }
            })
            .collect();
        std::fs::write(d.0.join("proc/meminfo"), meminfo).unwrap();
        let mut s = System::new();
        s.probe(&root).unwrap();
        data(s.sample(&root, Instant::now())).advice
    };
    let day = 86_400;
    assert!(advice(3 * day, 8_388_608).is_empty());
    let month = advice(31 * day, 8_388_608);
    assert_eq!(month[0].code, "restart-recommended");
    assert_eq!(month[0].reason, "uptime-30d");
    assert!(advice(8 * day, 8_388_608).is_empty(), "a quiet week");
    assert_eq!(advice(8 * day, 1_000_000)[0].reason, "uptime-7d-swap");
}

#[test]
fn a_minimal_machine_has_nones_not_errors() {
    let root = fixture("root-minimal");
    let mut s = System::new();
    s.probe(&root).unwrap();
    let d = data(s.sample(&root, Instant::now()));
    assert!(d.cpu.is_some());
    assert!(d.mem.is_some());
    assert!(d.load.is_none());
    assert!(d.pressure.is_none());
    assert!(d.temps.is_empty());
    assert!(d.thermal.is_none());
    assert!(d.gpus.is_empty());
    assert_eq!(d.power.unwrap().source, "none");
    assert!(d.uptime_sec.is_none());
    let cpu = d.cpu.unwrap();
    assert_eq!(cpu.mhz_avg, None);
    assert_eq!(cpu.governor, None);
    assert_eq!(cpu.physical, None);
}

#[test]
fn nvidia_smi_rows_parse() {
    use mirqab_core::samplers::system::gpu::parse_nvidia;
    let rows = parse_nvidia("37, 2048, 8192, 61, 120.50\n[N/A], 10, 8192, 40, [Not Supported]\n");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].busy, Some(37.0));
    assert_eq!(rows[0].vram_used, Some(2048 * 1_048_576));
    assert_eq!(rows[0].watts, Some(120.5));
    assert_eq!(rows[1].busy, None);
    assert_eq!(rows[1].watts, None);
}
