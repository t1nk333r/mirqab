//! GPUs from DRM sysfs (plan 002): AMD busy percent and VRAM, Intel clock,
//! NVIDIA through `nvidia-smi` when it is installed. A card whose device is
//! runtime-suspended is reported as such and nothing else is read from it:
//! reading its files would wake it (haseen's sysusage plugin learned this
//! on hybrid laptops).

use crate::exec;
use crate::proto::stream::Gpu;
use crate::proto::strings;
use crate::root::Root;
use std::time::Duration;

/// `card` followed by digits; connectors (`card1-DP-1`) share the directory.
fn is_card(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn read_trim(root: &Root, rel: &str) -> Option<String> {
    let s = root.read(rel).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn read_num<T: std::str::FromStr>(root: &Root, rel: &str) -> Option<T> {
    read_trim(root, rel)?.parse().ok()
}

/// The cards with a PCI vendor, in card order. A virtual card (simpledrm,
/// vkms, a USB display adapter) has none and would take a history slot
/// from a real GPU (review of PR #7).
pub fn cards(root: &Root) -> Vec<String> {
    let mut cards: Vec<String> = root
        .read_dir("sys/class/drm")
        .unwrap_or_default()
        .into_iter()
        .filter(|c| is_card(c))
        .filter(|c| read_trim(root, &format!("sys/class/drm/{c}/device/vendor")).is_some())
        .collect();
    cards.sort_by_key(|c| c[4..].parse::<u32>().unwrap_or(u32::MAX));
    cards
}

/// Whether the device under `dev` (a sysfs path) is runtime-suspended:
/// reading most of its files, or asking its driver, would wake it.
pub fn suspended(root: &Root, dev: &str) -> bool {
    read_trim(root, &format!("{dev}/power/runtime_status")).as_deref() == Some("suspended")
}

/// Whether `nvidia-smi` may run: an NVIDIA card exists and none is
/// suspended, since the query wakes a sleeping card (review of PR #7).
pub fn nvidia_wanted(root: &Root) -> bool {
    let nvidia: Vec<String> = cards(root)
        .into_iter()
        .map(|c| format!("sys/class/drm/{c}/device"))
        .filter(|dev| read_trim(root, &format!("{dev}/vendor")).as_deref() == Some("0x10de"))
        .collect();
    !nvidia.is_empty() && !nvidia.iter().any(|dev| suspended(root, dev))
}

/// One reading of every card. `nvidia` holds the last `nvidia-smi` rows,
/// refreshed by the caller no more than every 2 s.
pub fn read(root: &Root, nvidia: &[NvidiaRow]) -> Vec<Gpu> {
    let mut nv = nvidia.iter();
    cards(root)
        .into_iter()
        .map(|card| {
            let dev = format!("sys/class/drm/{card}/device");
            let vendor = match read_trim(root, &format!("{dev}/vendor")).as_deref() {
                Some("0x1002") => "amd",
                Some("0x8086") => "intel",
                Some("0x10de") => "nvidia",
                _ => "other",
            };
            let mut g = Gpu {
                id: card.clone(),
                name: strings::gpu_name(vendor, &card),
                vendor: vendor.to_string(),
                ..Gpu::default()
            };
            if suspended(root, &dev) {
                g.suspended = true;
                return g;
            }
            match vendor {
                "amd" => {
                    g.busy_percent = read_num(root, &format!("{dev}/gpu_busy_percent"));
                    g.vram_used_bytes = read_num(root, &format!("{dev}/mem_info_vram_used"));
                    g.vram_total_bytes = read_num(root, &format!("{dev}/mem_info_vram_total"));
                }
                "intel" => {
                    g.mhz = read_num(root, &format!("sys/class/drm/{card}/gt_cur_freq_mhz"));
                }
                "nvidia" => {
                    if let Some(row) = nv.next() {
                        g.busy_percent = row.busy;
                        g.vram_used_bytes = row.vram_used;
                        g.vram_total_bytes = row.vram_total;
                        g.celsius = row.celsius;
                        g.watts = row.watts;
                    }
                }
                _ => {}
            }
            // The card's own hwmon: temperature, power (µW), clock (Hz).
            if let Some(hw) = root
                .read_dir(&format!("{dev}/hwmon"))
                .ok()
                .and_then(|h| h.into_iter().find(|d| d.starts_with("hwmon")))
            {
                let base = format!("{dev}/hwmon/{hw}");
                g.celsius = g.celsius.or_else(|| {
                    read_num::<i64>(root, &format!("{base}/temp1_input")).map(|v| v as f64 / 1000.0)
                });
                g.watts = g.watts.or_else(|| {
                    read_num::<u64>(root, &format!("{base}/power1_average"))
                        .or_else(|| read_num(root, &format!("{base}/power1_input")))
                        .map(|uw| (uw as f64 / 1e5).round() / 10.0)
                });
                g.mhz = g.mhz.or_else(|| {
                    read_num::<u64>(root, &format!("{base}/freq1_input"))
                        .map(|hz| (hz / 1_000_000) as u32)
                });
            }
            g
        })
        .collect()
}

/// One GPU from `nvidia-smi`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NvidiaRow {
    pub busy: Option<f64>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub celsius: Option<f64>,
    pub watts: Option<f64>,
}

pub const NVIDIA_QUERY: &str =
    "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw";

/// `nvidia-smi --query-gpu=… --format=csv,noheader,nounits` output: one
/// line per GPU; memory in MiB; `[N/A]` or `[Not Supported]` is None.
pub fn parse_nvidia(text: &str) -> Vec<NvidiaRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
            NvidiaRow {
                busy: num(0),
                vram_used: num(1).map(|m| (m * 1_048_576.0) as u64),
                vram_total: num(2).map(|m| (m * 1_048_576.0) as u64),
                celsius: num(3),
                watts: num(4),
            }
        })
        .collect()
}

/// Runs `nvidia-smi` (an experimental path: no NVIDIA machine has
/// confirmed the field names, plan 002 Unverified).
pub fn query_nvidia() -> Vec<NvidiaRow> {
    match exec::run(
        &["nvidia-smi", NVIDIA_QUERY, "--format=csv,noheader,nounits"],
        None,
        Duration::from_secs(2),
    ) {
        Ok(out) if out.success() => parse_nvidia(&out.stdout),
        _ => Vec::new(),
    }
}
