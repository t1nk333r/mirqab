//! The System tab's view model (plan 002, Tests): on the golden event, the
//! memory split sums to the total, labels and groups are chosen, and the
//! graph helpers read events and history replies.

use mirqab_proto::stream::{Core, Cpu, Gpu, Power, SystemData};
use mirqab_view::format::TempUnit;
use mirqab_view::tabs::system::{self, Tone};
use serde_json::json;

fn golden() -> SystemData {
    serde_json::from_str(include_str!("../../../tests/golden/system.json")).unwrap()
}

#[test]
fn the_golden_event_fills_every_card() {
    let v = system::view(&golden(), TempUnit::Celsius);
    assert_eq!(
        v.overview.thermal,
        Some(("Cool".to_string(), Tone::Success))
    );
    assert_eq!(v.overview.uptime.as_deref(), Some("up 1h 0m"));
    assert_eq!(v.overview.load.as_deref(), Some("load 1.20 0.90 0.70"));
    assert!(v.overview.advice.is_none());
    let cpu = v.cpu.unwrap();
    assert_eq!(cpu.percent, "\u{2014}", "no baseline in the first event");
    assert_eq!(cpu.core_groups.len(), 1, "one kind of core, one group");
    assert_eq!(cpu.core_groups[0].len(), 16);
    assert_eq!(cpu.clock.as_deref(), Some("4.0 GHz now, 5.0 GHz max"));
    assert_eq!(cpu.governor.as_deref(), Some("powersave"));
    let mem = v.memory.unwrap();
    let sum = mem.split.used + mem.split.cache + mem.split.free;
    assert!((sum - 1.0).abs() < 1e-9, "{sum}");
    assert_eq!(mem.used_of_total, "17 GB of 67 GB");
    assert_eq!(mem.swap.as_deref(), Some("Swap 0 B of 8.6 GB"));
    assert_eq!(
        mem.zram.as_deref(),
        Some("Compressed swap 4.3 GB in 1.1 GB")
    );
    assert_eq!(v.pressure.len(), 3);
    assert_eq!(v.pressure[2].series, "psi.io");
    assert_eq!(v.gpus.len(), 2);
    assert_eq!(v.gpus[1].busy.as_deref(), Some("3.0%"));
    assert_eq!(v.gpus[1].vram.as_ref().unwrap().0, "1.1 GB of 17 GB");
    let kinds: Vec<&str> = v.temps.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["CPU", "GPU", "Disk", "Memory"]);
    assert_eq!(v.temps[0].1[0].label, "Tctl");
    assert!((v.temps[0].1[0].fraction - 52.0 / 95.0).abs() < 1e-9);
    assert!(v.power.needs_helper, "no package watts in the first event");
}

#[test]
fn hybrid_cores_are_grouped_performance_first() {
    let core = |id, kind: &str| Core {
        id,
        percent: Some(50.0),
        mhz: None,
        kind: Some(kind.into()),
    };
    let d = SystemData {
        cpu: Some(Cpu {
            cores: vec![core(0, "e"), core(1, "p"), core(2, "p"), core(3, "e")],
            ..Cpu::default()
        }),
        ..SystemData::default()
    };
    let cpu = system::view(&d, TempUnit::Celsius).cpu.unwrap();
    let groups: Vec<Vec<&str>> = cpu
        .core_groups
        .iter()
        .map(|g| g.iter().map(|c| c.label.as_str()).collect())
        .collect();
    assert_eq!(groups, [vec!["1", "2"], vec!["0", "3"]]);
    assert_eq!(cpu.core_groups[0][0].fraction, Some(0.5));
}

#[test]
fn the_split_never_exceeds_the_total() {
    // Used and cache that overlap past the total: cache is cut to fit.
    let s = system::mem_split(100, 70, 50);
    assert!((s.used - 0.7).abs() < 1e-9);
    assert!((s.cache - 0.3).abs() < 1e-9);
    assert!(s.free.abs() < 1e-9);
    assert_eq!(system::mem_split(0, 0, 0).free, 1.0);
}

#[test]
fn suspended_gpus_battery_power_and_thermal_tones() {
    let d = SystemData {
        gpus: vec![Gpu {
            id: "card1".into(),
            name: "AMD GPU (card1)".into(),
            vendor: "amd".into(),
            suspended: true,
            ..Gpu::default()
        }],
        power: Some(Power {
            package_watts: Some(12.34),
            battery_watts: Some(9.0),
            source: "both".into(),
        }),
        ..SystemData::default()
    };
    let v = system::view(&d, TempUnit::Fahrenheit);
    assert!(v.gpus[0].suspended);
    assert!(v.gpus[0].busy.is_none());
    assert_eq!(v.power.package.as_deref(), Some("12.3 W"));
    assert_eq!(v.power.battery.as_deref(), Some("9.0 W"));
    assert!(!v.power.needs_helper);
    assert!(v.cpu.is_none() && v.memory.is_none() && v.temps.is_empty());
    assert_eq!(system::thermal_tone("cool", false), Tone::Success);
    assert_eq!(system::thermal_tone("warm", false), Tone::Neutral);
    assert_eq!(system::thermal_tone("hot", false), Tone::Warning);
    assert_eq!(system::thermal_tone("critical", false), Tone::Error);
    assert_eq!(system::thermal_tone("cool", true), Tone::Error);
}

#[test]
fn graphs_read_live_events_and_history_replies() {
    let event = json!({"cpu": {"percent": 12.5}, "mem": {"usedBytes": 1000},
                       "gpus": [{"busyPercent": 3.0}, {"busyPercent": 40.0}],
                       "pressure": {"io": {"some10": 2.0}}});
    assert_eq!((system::CPU_GRAPH.live)(&event), Some(12.5));
    assert_eq!((system::MEMORY_GRAPH.live)(&event), Some(1000.0));
    assert_eq!((system::gpu_graph(1).unwrap().live)(&event), Some(40.0));
    assert!(system::gpu_graph(2).is_none());
    assert_eq!((system::pressure_graph("psi.io").live)(&event), Some(2.0));
    assert_eq!((system::POWER_GRAPH.live)(&event), None);
    let reply = json!({"series": {"cpu.total": {"unit": "percent",
        "points": [[1800000000, 10.0, 30.0], [1800000060, 12.0, 15.0], ["bad"]]}}});
    let (avg, max) = system::history_points(&reply, "cpu.total");
    assert_eq!(avg, [(1_800_000_000, 10.0), (1_800_000_060, 12.0)]);
    assert_eq!(max[0], (1_800_000_000, 30.0));
    assert!(system::history_points(&reply, "nope").0.is_empty());
}
