//! Graph geometry for a known series, a gap and an empty series (plan 001,
//! Tests).

use mirqab_view::graph::{self, Scale, Window, geometry, nice_ceiling};

const PCT: Scale = Scale::Fixed {
    min: 0.0,
    max: 100.0,
};

fn win() -> Window {
    win_step(60)
}

fn win_step(step: i64) -> Window {
    Window {
        from: 0,
        to: 600,
        step,
    }
}

#[test]
fn a_known_series_maps_onto_the_widget() {
    let pts = [(0, 0.0), (300, 50.0), (600, 100.0)];
    let g = geometry(&pts, win_step(300), PCT, 600.0, 100.0);
    assert_eq!(g.runs.len(), 1);
    assert_eq!(
        g.runs[0].line,
        vec![(0.0, 100.0), (300.0, 50.0), (600.0, 0.0)]
    );
    assert_eq!((g.bottom, g.top), (0.0, 100.0));
}

#[test]
fn neighbours_more_than_a_step_and_a_half_apart_break_the_line() {
    // 60 s steps: 0, 60, 120 then nothing until 300.
    let pts = [(0, 1.0), (60, 2.0), (120, 3.0), (300, 4.0), (360, 5.0)];
    let g = geometry(&pts, win(), PCT, 600.0, 100.0);
    assert_eq!(g.runs.len(), 2);
    assert_eq!(g.runs[0].line.len(), 3);
    assert_eq!(g.runs[1].line.len(), 2);
    // 90 s apart is still one step and a half: no break.
    let g = geometry(&[(0, 1.0), (90, 2.0)], win(), PCT, 600.0, 100.0);
    assert_eq!(g.runs.len(), 1);
}

#[test]
fn an_empty_series_draws_nothing() {
    let g = geometry(&[], win(), PCT, 600.0, 100.0);
    assert!(g.runs.is_empty());
    let g = geometry(&[(-60, 5.0), (660, 5.0)], win(), PCT, 600.0, 100.0);
    assert!(g.runs.is_empty(), "points outside the window are dropped");
}

#[test]
fn a_lone_point_is_a_short_flat_segment() {
    let g = geometry(&[(300, 50.0)], win(), PCT, 600.0, 100.0);
    assert_eq!(g.runs[0].line, vec![(270.0, 50.0), (330.0, 50.0)]);
}

#[test]
fn values_are_clamped_and_the_fill_closes_on_the_baseline() {
    let g = geometry(
        &[(0, -10.0), (600, 150.0)],
        win_step(600),
        PCT,
        600.0,
        100.0,
    );
    assert_eq!(g.runs[0].line, vec![(0.0, 100.0), (600.0, 0.0)]);
    let fill = g.runs[0].fill(100.0);
    assert_eq!(
        fill,
        vec![(0.0, 100.0), (600.0, 0.0), (600.0, 100.0), (0.0, 100.0)]
    );
}

#[test]
fn auto_scale_rounds_up_to_a_nice_number() {
    let g = geometry(
        &[(0, 3.0), (60, 170.0)],
        win(),
        Scale::Auto { floor: 1.0 },
        600.0,
        100.0,
    );
    assert_eq!(g.top, 200.0);
    let flat = geometry(&[(0, 0.0)], win(), Scale::Auto { floor: 1.0 }, 600.0, 100.0);
    assert_eq!(flat.top, 1.0);
    for (v, want) in [
        (0.0, 0.0),
        (1.0, 1.0),
        (1.1, 2.0),
        (2.2, 2.5),
        (3.0, 5.0),
        (7.0, 10.0),
        (0.03, 0.05),
    ] {
        assert!(
            (nice_ceiling(v) - want).abs() < 1e-12,
            "{v} -> {}",
            nice_ceiling(v)
        );
    }
}

#[test]
fn every_range_has_the_bucket_of_architecture_6_4() {
    let now = 1_800_000_000;
    let points = |r| {
        let w = Window::for_range(r, now).unwrap();
        (w.to - w.from) / w.step
    };
    assert_eq!(points("2m"), 120);
    assert_eq!(points("1h"), 60);
    assert_eq!(points("24h"), 360);
    assert_eq!(points("7d"), 336);
    assert_eq!(points("30d"), 360);
    assert!(Window::for_range("1y", now).is_none());
    assert_eq!(graph::RANGES, ["2m", "1h", "24h", "7d", "30d"]);
}
