//! Graph geometry: points in, line and fill outlines out (architecture
//! §7.4). The `Graph` widget only turns these into one stroked and one
//! filled path; scaling, gaps and the fill shape are decided and tested
//! here, with no display.
//!
//! Points are `(unix seconds, value)`, as `history.query` returns buckets
//! and as the live ring keeps samples. Buckets with no data are absent, so
//! two neighbours more than one and a half steps apart break the line.

/// The vertical scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    /// A fixed range, such as 0 to 100 for a percentage.
    Fixed { min: f64, max: f64 },
    /// From 0 to a round number at or above the largest value (at least
    /// `floor`, so a flat zero line has a scale).
    Auto { floor: f64 },
}

/// One drawable piece of a graph, in widget coordinates (y grows down).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The line through the points, left to right.
    pub line: Vec<(f32, f32)>,
}

impl Run {
    /// The fill under the line: the line, then down to the baseline at its
    /// last x and back along the baseline to its first x. A closed outline.
    pub fn fill(&self, height: f32) -> Vec<(f32, f32)> {
        let mut out = self.line.clone();
        if let (Some(&(x0, _)), Some(&(x1, _))) = (self.line.first(), self.line.last()) {
            out.push((x1, height));
            out.push((x0, height));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub runs: Vec<Run>,
    /// The value at the top edge, for the axis label.
    pub top: f64,
    /// The value at the bottom edge.
    pub bottom: f64,
}

/// The window of time a graph shows and how far apart its points are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Left edge, unix seconds.
    pub from: i64,
    /// Right edge, unix seconds.
    pub to: i64,
    /// Seconds between points (1 for live, the bucket for a stored range).
    pub step: i64,
}

impl Window {
    /// The window of a stored range ending at `now`: `1h`, `24h`, `7d`,
    /// `30d` (architecture §6.4), or the 2-minute live range, `2m`.
    pub fn for_range(range: &str, now: i64) -> Option<Window> {
        let (span, step) = match range {
            "2m" => (120, 1),
            "1h" => (3600, 60),
            "24h" => (86_400, 240),
            "7d" => (7 * 86_400, 1800),
            "30d" => (30 * 86_400, 7200),
            _ => return None,
        };
        Some(Window {
            from: now - span,
            to: now,
            step,
        })
    }
}

/// The ranges a graph's picker offers, in order.
pub const RANGES: &[&str] = &["2m", "1h", "24h", "7d", "30d"];

/// The geometry of `points` in `window` at `width` × `height`. Points
/// outside the window are dropped, values are clamped to the scale, and a
/// run of one point becomes a short flat segment one step wide so it is
/// visible.
pub fn geometry(
    points: &[(i64, f64)],
    window: Window,
    scale: Scale,
    width: f32,
    height: f32,
) -> Geometry {
    let visible: Vec<(i64, f64)> = points
        .iter()
        .copied()
        .filter(|&(t, v)| t >= window.from && t <= window.to && v.is_finite())
        .collect();
    let (bottom, top) = match scale {
        Scale::Fixed { min, max } => (min, max),
        Scale::Auto { floor } => {
            let peak = visible.iter().map(|&(_, v)| v).fold(floor, f64::max);
            (0.0, nice_ceiling(peak))
        }
    };
    let span_t = (window.to - window.from).max(1) as f64;
    let span_v = if top > bottom { top - bottom } else { 1.0 };
    let x = |t: f64| ((t - window.from as f64) / span_t * f64::from(width)) as f32;
    let y = |v: f64| {
        let v = v.clamp(bottom, top);
        (f64::from(height) - (v - bottom) / span_v * f64::from(height)) as f32
    };

    let mut runs = Vec::new();
    let mut current: Vec<(i64, f64)> = Vec::new();
    let gap = window.step * 3 / 2;
    for p in visible {
        if let Some(&(last, _)) = current.last()
            && p.0 - last > gap
        {
            runs.push(std::mem::take(&mut current));
        }
        current.push(p);
    }
    if !current.is_empty() {
        runs.push(current);
    }

    let half = window.step as f64 / 2.0;
    let runs = runs
        .into_iter()
        .map(|r| {
            let line = if r.len() == 1 {
                let (t, v) = r[0];
                let t0 = (t as f64 - half).max(window.from as f64);
                let t1 = (t as f64 + half).min(window.to as f64);
                vec![(x(t0), y(v)), (x(t1), y(v))]
            } else {
                r.iter().map(|&(t, v)| (x(t as f64), y(v))).collect()
            };
            Run { line }
        })
        .collect();
    Geometry { runs, top, bottom }
}

/// The smallest of 1, 2, 2.5, 5 × 10ⁿ at or above `v` (0 for 0 or less).
pub fn nice_ceiling(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        return 0.0;
    }
    let mag = 10f64.powf(v.log10().floor());
    for m in [1.0, 2.0, 2.5, 5.0, 10.0] {
        // A small tolerance so 200.0000001 from float noise stays 200.
        if m * mag >= v * (1.0 - 1e-9) {
            return m * mag;
        }
    }
    10.0 * mag
}
