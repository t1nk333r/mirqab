//! The `system` sampler on fixture roots (plan 002, Tests): rates from the
//! `.t1` twin, the thermal word at each threshold, throttling, hybrid
//! cores, a suspended GPU that is never read, battery power, the restart
//! advice, and a minimal machine where every missing file is `None`. From
//! the audit: the minute reads no disk sensor, an awake dGPU is read at
//! most every 10 s, and so are the disk sensors for a live reader. From
//! the audit of 2026-11-21: page counts are per minute whatever the gap
//! (none under 10 s), and a stub `nvidia-smi`'s temperature reaches the
//! thermal word and `temp.gpu`. From plan 024: `get` waits for its second
//! reading outside the lock, the disks are read and `nvidia-smi` run only
//! for a subscriber that asks, a gap restarts the pressure advice's hour,
//! and the unavailable reason comes from `strings.rs`.

use mirqab_core::Root;
use mirqab_core::proto::stream::{SystemData, Temp};
use mirqab_core::proto::strings;
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::system::{System, temps};
use serde_json::json;
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
fn page_counts_are_per_minute_whatever_the_gap() {
    // Audit 2026-11-21, L24: the first minute after start covers what is
    // left of it, and a late tick up to 90 s. The same counters 30 s
    // apart are twice the per-minute count.
    let t0 = Instant::now();
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    s.minute_at(&fixture("root-desktop"), WALL, t0);
    let m = s.minute_at(
        &fixture("root-desktop.t1"),
        WALL + 30,
        t0 + Duration::from_secs(30),
    );
    let get = |k: &str| m.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    assert_eq!(get("page.in"), Some(1200));
    assert_eq!(get("page.out"), Some(2400));
    assert_eq!(get("swap.in"), Some(12));
    assert_eq!(get("swap.out"), Some(24));
}

#[test]
fn page_counts_over_a_gap_under_ten_seconds_are_skipped() {
    // Review of the L24 fix: a probe at :59 leaves the first minute about
    // 1 s, and an NTP step can shorten an Instant gap; scaling either to
    // 60 s would multiply a burst up to 60 times. Under 10 s the page and
    // swap series are skipped; at 10 s they are scaled six times.
    let get = |m: &[(&str, i64)], k: &str| m.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    for (secs, page_in) in [(1, None), (9, None), (10, Some(3600))] {
        let t0 = Instant::now();
        let mut s = System::new();
        s.probe(&fixture("root-desktop")).unwrap();
        s.minute_at(&fixture("root-desktop"), WALL, t0);
        let m = s.minute_at(
            &fixture("root-desktop.t1"),
            WALL + 60,
            t0 + Duration::from_secs(secs),
        );
        assert_eq!(get(&m, "page.in"), page_in, "{secs} s");
        assert_eq!(get(&m, "swap.out").is_some(), page_in.is_some(), "{secs} s");
        assert!(
            get(&m, "cpu.total").is_some(),
            "the other series stay: {secs} s"
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
    // Without a primed reading of its own `get` gives no rate, never the
    // 600 s average (review of PR #7), and never sleeps for one.
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let late = Instant::now() + Duration::from_secs(600);
    let snap = data(s.snapshot(&fixture("root-desktop.t1"), late));
    assert_eq!(snap.cpu.unwrap().percent, None);
}

#[test]
fn get_waits_for_its_second_reading_without_the_lock() {
    // Plan 024 C6: with no baseline 1-90 s old, `get` slept 200 ms in
    // `snapshot`, under the sampler's lock, and stamped the second reading
    // 200 ms on whatever the sleep took. Now `prime` takes the first and
    // asks the server to wait without the lock; `snapshot` answers at once
    // against it, stamped with the instant it is given.
    let mut s = System::new();
    s.probe(&fixture("root-desktop")).unwrap();
    let late = Instant::now() + Duration::from_secs(600);
    assert_eq!(
        s.prime(&fixture("root-desktop"), late),
        Some(Duration::from_millis(200))
    );
    let t = Instant::now();
    let snap = data(s.snapshot(
        &fixture("root-desktop.t1"),
        late + Duration::from_millis(400),
    ));
    assert!(
        t.elapsed() < Duration::from_millis(150),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(snap.cpu.unwrap().percent, Some(20.0));
    // 600 KiB over the 400 ms that passed, not over 200 ms.
    assert_eq!(snap.mem.unwrap().page_in_per_sec, Some(1500.0));
    // A minute baseline 30 s old serves: no wait.
    let fresh = Instant::now() + Duration::from_secs(30);
    assert_eq!(s.prime(&fixture("root-desktop"), fresh), None);
    let snap = data(s.snapshot(&fixture("root-desktop.t1"), fresh));
    assert_eq!(snap.cpu.unwrap().percent, Some(20.0));
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
fn an_nvidia_smi_temperature_reaches_the_thermal_word_and_temp_gpu() {
    // Audit 2026-11-21, L25: NVIDIA's driver has no hwmon chip, so its
    // temperature comes only from nvidia-smi; a stub answers 85 °C for
    // one awake NVIDIA card.
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("nv85", "root-minimal");
    let dev = d.0.join("sys/class/drm/card0/device");
    std::fs::create_dir_all(dev.join("power")).unwrap();
    std::fs::write(dev.join("vendor"), "0x10de\n").unwrap();
    std::fs::write(dev.join("power/runtime_status"), "active\n").unwrap();
    let stub = d.0.join("nvidia-smi");
    std::fs::write(&stub, "#!/bin/sh\nprintf '5, 100, 8192, 85, 40.00\\n'\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = Root::new(&d.0);
    let mut s = System::with_nvidia_smi(stub.to_string_lossy());
    s.probe(&root).unwrap();
    s.wants(&[json!({"gpus": true})]);
    let t0 = Instant::now();
    let first = data(s.sample(&root, t0));
    assert_eq!(first.gpus[0].celsius, Some(85.0));
    let thermal = first.thermal.expect("a thermal word");
    assert_eq!(thermal.state, "hot", "85 of 95 °C");
    assert_eq!(thermal.hottest.as_deref(), Some("nvidia.0"));
    assert!(
        first.temps.iter().all(|t| !t.id.starts_with("nvidia")),
        "the streamed list stays hwmon's"
    );
    s.minute_at(&root, WALL, t0);
    let m = s.minute_at(&root, WALL + 60, t0 + Duration::from_secs(60));
    let get = |k: &str| m.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    assert_eq!(get("temp.gpu"), Some(850));
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
        let kind = e.file_type().unwrap();
        if kind.is_symlink() {
            // The process fixtures' links (exe, cwd, fd) name paths of a
            // live machine: copied as links, never followed (plan 006).
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
        } else if kind.is_dir() {
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

#[test]
fn the_minute_never_reads_a_disk_sensor() {
    // On some drives a temperature read restarts the spin-down timer
    // (kernel Documentation/hwmon/drivetemp.rst, Known issues), and no
    // series stores a disk temperature. A FIFO in place of the NVMe
    // sensor blocks whoever opens it.
    let d = scratch("disks", "root-desktop");
    let sensor = d.0.join("sys/class/hwmon/hwmon1/temp1_input");
    std::fs::remove_file(&sensor).unwrap();
    let st = std::process::Command::new("mkfifo")
        .arg(&sensor)
        .status()
        .expect("mkfifo runs");
    assert!(st.success());
    let base = d.0.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let root = Root::new(&base);
        let mut s = System::new();
        s.probe(&root).unwrap();
        // Even while a subscriber shows the disks.
        s.wants(&[json!({"disks": true})]);
        let t0 = Instant::now();
        s.minute_at(&root, WALL, t0);
        tx.send(s.minute_at(&root, WALL + 60, t0 + Duration::from_secs(60)))
            .unwrap();
    });
    let m = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the minute reads no disk sensor");
    assert!(m.iter().any(|(k, _)| *k == "temp.cpu"));
    // A live sample or a `get` that asks still shows them: someone is
    // looking.
    let root = fixture("root-desktop");
    let mut s = System::new();
    s.probe(&root).unwrap();
    s.wants(&[json!({"disks": true})]);
    let live = data(s.sample(&root, Instant::now()));
    let disks: Vec<&str> = live
        .temps
        .iter()
        .filter(|t| t.kind == "disk")
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(disks, ["nvme/temp1", "nvme.1/temp1"]);
    let snap = data(s.snapshot(&root, Instant::now()));
    assert_eq!(snap.temps.iter().filter(|t| t.kind == "disk").count(), 2);
}

#[test]
fn a_subscriber_that_shows_no_disk_never_reads_a_disk_sensor() {
    // Plan 024 C18: a bar item subscribes to `system` all day and shows
    // no disk temperature; a read every 10 s still kept a drivetemp disk
    // from spinning down. Only a subscriber (or a `get`) whose params say
    // `disks` has them read. The FIFO blocks whoever opens it.
    let d = scratch("bar-disks", "root-desktop");
    let sensor = d.0.join("sys/class/hwmon/hwmon1/temp1_input");
    std::fs::remove_file(&sensor).unwrap();
    let st = std::process::Command::new("mkfifo")
        .arg(&sensor)
        .status()
        .expect("mkfifo runs");
    assert!(st.success());
    let base = d.0.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let root = Root::new(&base);
        let mut s = System::new();
        s.probe(&root).unwrap();
        let t0 = Instant::now();
        let mut shown = Vec::new();
        for secs in [0, 5, 11, 22] {
            // A bar's params, and one that names the flag false.
            s.wants(&[json!({}), json!({"disks": false})]);
            shown.push(data(s.sample(&root, t0 + Duration::from_secs(secs))));
        }
        s.wants(&[json!({})]);
        shown.push(data(s.snapshot(&root, t0 + Duration::from_secs(30))));
        tx.send(shown).unwrap();
    });
    let shown = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("no disk sensor is read");
    for d in &shown {
        assert!(d.temps.iter().all(|t| t.kind != "disk"));
        assert!(d.temps.iter().any(|t| t.kind == "cpu"), "the rest is read");
    }
    // With the System page open beside the bar, they are read.
    let root = fixture("root-desktop");
    let mut s = System::new();
    s.probe(&root).unwrap();
    s.wants(&[json!({}), json!({"disks": true})]);
    let both = data(s.sample(&root, Instant::now()));
    assert_eq!(both.temps.iter().filter(|t| t.kind == "disk").count(), 2);
}

#[test]
fn nvidia_smi_runs_only_for_a_gpu_on_screen() {
    // Plan 024 C19: a `cpu` bar item made the daemon run nvidia-smi every
    // event. It runs for a subscriber whose params say `gpus` (every 2 s
    // at most) and for the minute's series; without one the minute's
    // reading is repeated while under 90 s old, so the thermal word still
    // counts the card, and nothing is run.
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("nv-shown", "root-minimal");
    let dev = d.0.join("sys/class/drm/card0/device");
    std::fs::create_dir_all(dev.join("power")).unwrap();
    std::fs::write(dev.join("vendor"), "0x10de\n").unwrap();
    std::fs::write(dev.join("power/runtime_status"), "active\n").unwrap();
    let runs = d.0.join("runs");
    let stub = d.0.join("nvidia-smi");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\necho run >> '{}'\nprintf '5, 100, 8192, 85, 40.00\\n'\n",
            runs.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let count = || {
        std::fs::read_to_string(&runs)
            .map(|t| t.lines().count())
            .unwrap_or(0)
    };
    let root = Root::new(&d.0);
    let mut s = System::with_nvidia_smi(stub.to_string_lossy());
    s.probe(&root).unwrap();
    let t0 = Instant::now();
    let at = |secs: u64| t0 + Duration::from_secs(secs);
    s.wants(&[json!({})]);
    for secs in [0, 2, 4] {
        let bar = data(s.sample(&root, at(secs)));
        assert_eq!(bar.gpus[0].celsius, None, "nothing read yet");
    }
    let get = data(s.snapshot(&root, at(5)));
    assert_eq!(get.gpus[0].celsius, None);
    assert_eq!(count(), 0, "no subscriber shows a GPU");
    s.minute_at(&root, WALL, at(6));
    assert_eq!(count(), 1, "the minute's series");
    let held = data(s.sample(&root, at(8)));
    assert_eq!(held.gpus[0].celsius, Some(85.0), "the minute's reading");
    assert_eq!(held.thermal.unwrap().hottest.as_deref(), Some("nvidia.0"));
    assert_eq!(count(), 1);
    // The GPU card opens: every 2 s at most.
    s.wants(&[json!({}), json!({"gpus": true})]);
    s.sample(&root, at(9));
    assert_eq!(count(), 2);
    s.sample(&root, at(10));
    assert_eq!(count(), 2, "under 2 s: repeated");
    s.sample(&root, at(11));
    assert_eq!(count(), 3);
    // It closes; the last reading ages out after 90 s.
    s.wants(&[json!({})]);
    assert_eq!(data(s.sample(&root, at(100))).gpus[0].celsius, Some(85.0));
    assert_eq!(data(s.sample(&root, at(102))).gpus[0].celsius, None);
    assert_eq!(count(), 3);
}

#[test]
fn pressure_advice_restarts_after_a_wall_gap() {
    // Plan 024 L4: "pressure above 10 % for the last hour" counted the
    // last 60 minutes sampled, so 59 minutes before a suspend and one
    // after it made an hour. A minute after a gap starts the hour over.
    let d = scratch("pressure", "root-desktop");
    std::fs::write(d.0.join("proc/uptime"), format!("{}.00 0.00\n", 8 * 86_400)).unwrap();
    std::fs::write(
        d.0.join("proc/pressure/memory"),
        "some avg10=20.00 avg60=20.00 avg300=20.00 total=5000\n\
         full avg10=1.00 avg60=1.00 avg300=1.00 total=2500\n",
    )
    .unwrap();
    let root = Root::new(&d.0);
    let advice = |last_gap: u64| {
        let mut s = System::new();
        s.probe(&root).unwrap();
        let t0 = Instant::now();
        let mut wall = WALL;
        for n in 0..60 {
            wall += if n == 59 { last_gap } else { 60 };
            s.minute_at(&root, wall, t0 + Duration::from_secs(wall - WALL));
        }
        let after = t0 + Duration::from_secs(wall - WALL + 1);
        data(s.sample(&root, after)).advice
    };
    let hour = advice(60);
    assert_eq!(hour.len(), 1, "an hour under pressure");
    assert_eq!(hour[0].reason, "uptime-7d-pressure");
    assert!(advice(3600).is_empty(), "59 minutes, a sleep, one minute");
}

#[test]
fn a_root_without_a_cpu_line_is_unavailable_in_the_strings_words() {
    // Plan 024 L16: the reason `status` shows comes from strings.rs.
    let d = scratch("nocpu", "root-minimal");
    std::fs::write(d.0.join("proc/stat"), "intr 0\nbtime 1700000000\n").unwrap();
    let mut s = System::new();
    assert_eq!(
        s.probe(&Root::new(&d.0)),
        Err(strings::SYSTEM_UNAVAILABLE.to_string())
    );
}

#[test]
fn an_awake_dgpu_is_read_at_most_every_ten_seconds() {
    // Each read of an awake amdgpu card's busy or hwmon files restarts
    // its 5 s autosuspend timer, so a 1-2 s stream would keep an idle
    // dGPU powered. card0 is not the boot display (boot_vga 0): it is
    // read once per 10 s and repeated between; card1 is read every time.
    let d = scratch("dgpu", "root-desktop");
    let control = d.0.join("sys/class/drm/card0/device/power/control");
    std::fs::write(&control, "auto\n").unwrap();
    let root = Root::new(&d.0);
    let mut s = System::new();
    s.probe(&root).unwrap();
    let edge = |d: &SystemData| {
        d.temps
            .iter()
            .find(|t| t.id == "amdgpu.1/temp1")
            .map(|t| t.celsius)
    };
    let t0 = Instant::now();
    let first = data(s.sample(&root, t0));
    assert_eq!(first.gpus[0].busy_percent, Some(0.0));
    assert_eq!(first.gpus[0].celsius, Some(40.0));
    assert_eq!(edge(&first), Some(40.0));
    let card0 = d.0.join("sys/class/drm/card0/device");
    std::fs::write(card0.join("gpu_busy_percent"), "55\n").unwrap();
    std::fs::write(card0.join("hwmon/hwmon6/temp1_input"), "70000\n").unwrap();
    std::fs::write(d.0.join("sys/class/hwmon/hwmon6/temp1_input"), "70000\n").unwrap();
    std::fs::write(
        d.0.join("sys/class/drm/card1/device/gpu_busy_percent"),
        "77\n",
    )
    .unwrap();
    let second = data(s.sample(&root, t0 + Duration::from_secs(2)));
    assert_eq!(second.gpus[0].busy_percent, Some(0.0), "repeated, not read");
    assert_eq!(second.gpus[0].celsius, Some(40.0));
    assert_eq!(edge(&second), Some(40.0), "nor its chip under class/hwmon");
    assert_eq!(second.gpus[1].busy_percent, Some(77.0), "the boot display");
    let minute = s.minute_at(&root, WALL, t0 + Duration::from_secs(4));
    assert!(
        minute.contains(&("gpu.0.busy", 0)),
        "the minute repeats it too: {minute:?}"
    );
    let third = data(s.sample(&root, t0 + Duration::from_secs(10)));
    assert_eq!(third.gpus[0].busy_percent, Some(55.0), "read again at 10 s");
    assert_eq!(third.gpus[0].celsius, Some(70.0));
    assert_eq!(edge(&third), Some(70.0));
    // A card whose runtime power management is off never suspends: the
    // pace gains nothing, so it is read every time.
    std::fs::write(&control, "on\n").unwrap();
    std::fs::write(card0.join("gpu_busy_percent"), "66\n").unwrap();
    let held_off = data(s.sample(&root, t0 + Duration::from_secs(11)));
    assert_eq!(
        held_off.gpus[0].busy_percent,
        Some(66.0),
        "power/control on"
    );
    // A card that went to sleep shows as suspended at once, its last
    // values dropped and nothing read.
    for dev in [card0.clone(), d.0.join("sys/class/hwmon/hwmon6/device")] {
        std::fs::create_dir_all(dev.join("power")).unwrap();
        std::fs::write(dev.join("power/runtime_status"), "suspended\n").unwrap();
    }
    let fourth = data(s.sample(&root, t0 + Duration::from_secs(12)));
    assert!(fourth.gpus[0].suspended);
    assert_eq!(fourth.gpus[0].busy_percent, None);
    assert_eq!(edge(&fourth), None);
}

#[test]
fn disk_sensors_are_read_at_most_every_ten_seconds() {
    // A drivetemp read is an ATA command, and on some drives it restarts
    // the spin-down timer (kernel Documentation/hwmon/drivetemp.rst,
    // Known issues): a bar item or the System page at 2 s would read the
    // disks on every tick. They are read at most once per 10 s, the last
    // rows repeated between; a value is never shown more than 10 s old.
    let d = scratch("disk-pace", "root-desktop");
    let root = Root::new(&d.0);
    let hwmon = d.0.join("sys/class/hwmon");
    let set = |dir: &str, milli: u32| {
        std::fs::write(hwmon.join(dir).join("temp1_input"), format!("{milli}\n")).unwrap();
    };
    let disks = |d: &SystemData| -> Vec<(String, f64)> {
        d.temps
            .iter()
            .filter(|t| t.kind == "disk")
            .map(|t| (t.id.clone(), t.celsius))
            .collect()
    };
    let shown = |pairs: &[(&str, f64)]| -> Vec<(String, f64)> {
        pairs.iter().map(|(i, c)| (i.to_string(), *c)).collect()
    };
    let mut s = System::new();
    s.probe(&root).unwrap();
    s.wants(&[json!({"disks": true})]);
    let t0 = Instant::now();
    // The minute reads no disk and starts no pace.
    s.minute_at(&root, WALL, t0);
    set("hwmon1", 42_000);
    set("hwmon5", 40_000);
    // The first tick that wants the disks reads them at once.
    let first = data(s.sample(&root, t0 + Duration::from_secs(1)));
    assert_eq!(
        disks(&first),
        shown(&[("nvme/temp1", 42.0), ("nvme.1/temp1", 40.0)])
    );
    // Between reads the rows are repeated, the CPU sensor still read.
    set("hwmon1", 50_000);
    set("hwmon5", 48_000);
    set("hwmon0", 77_000);
    // A disk that appears between reads waits for the next one.
    std::fs::create_dir_all(hwmon.join("hwmon7")).unwrap();
    std::fs::write(hwmon.join("hwmon7/name"), "drivetemp\n").unwrap();
    set("hwmon7", 33_000);
    for secs in [3, 5, 7, 9] {
        let held = data(s.sample(&root, t0 + Duration::from_secs(secs)));
        assert_eq!(
            disks(&held),
            shown(&[("nvme/temp1", 42.0), ("nvme.1/temp1", 40.0)]),
            "repeated at {secs} s"
        );
        let cpu = held.temps.iter().find(|t| t.id == "k10temp/temp1");
        assert_eq!(cpu.map(|t| t.celsius), Some(77.0));
    }
    // Neither the minute nor a `get` reads them or moves the pace.
    s.minute_at(&root, WALL + 60, t0 + Duration::from_secs(10));
    let get = data(s.snapshot(&root, t0 + Duration::from_secs(10)));
    assert_eq!(
        disks(&get),
        shown(&[("nvme/temp1", 42.0), ("nvme.1/temp1", 40.0)])
    );
    // 10 s after the last read: read again, the new disk with them.
    let again = data(s.sample(&root, t0 + Duration::from_secs(11)));
    assert_eq!(
        disks(&again),
        shown(&[
            ("nvme/temp1", 50.0),
            ("nvme.1/temp1", 48.0),
            ("drivetemp/temp1", 33.0)
        ])
    );
    // The readers leave and come back within 10 s of the last read: the
    // held rows, still under 10 s old.
    set("hwmon1", 55_000);
    let back = data(s.sample(&root, t0 + Duration::from_secs(16)));
    assert_eq!(disks(&back)[0], ("nvme/temp1".to_string(), 50.0));
    // Later than that: read at once.
    let later = data(s.sample(&root, t0 + Duration::from_secs(60)));
    assert_eq!(disks(&later)[0], ("nvme/temp1".to_string(), 55.0));
}
