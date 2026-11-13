//! Number formatting against known input (plan 001, Tests).

use mirqab_view::format::{self, TempUnit};

#[test]
fn bytes_use_si_units_and_three_figures() {
    for (n, want) in [
        (0, "0 B"),
        (999, "999 B"),
        (1000, "1.0 kB"),
        (1500, "1.5 kB"),
        (14_000_000, "14 MB"),
        (14_680_064, "15 MB"),
        (999_449, "999 kB"),
        (999_949, "1.0 MB"),
        (999_960, "1.0 MB"),
        (9_960_000, "10 MB"),
        (1_234_567_890, "1.2 GB"),
    ] {
        assert_eq!(format::bytes(n), want, "{n}");
    }
    // Past the last unit the number grows instead of the unit.
    assert_eq!(format::bytes(u64::MAX), "18447 PB");
}

#[test]
fn rates_append_per_second_and_refuse_nonsense() {
    assert_eq!(format::rate(0.0), "0 B/s");
    assert_eq!(format::rate(1_200_000.0), "1.2 MB/s");
    assert_eq!(format::rate(-1.0), "\u{2014}");
    assert_eq!(format::rate(f64::NAN), "\u{2014}");
}

#[test]
fn percent_has_one_decimal_under_ten() {
    assert_eq!(format::percent(0.0), "0.0%");
    assert_eq!(format::percent(0.24), "0.2%");
    assert_eq!(format::percent(9.94), "9.9%");
    assert_eq!(format::percent(9.96), "10%");
    assert_eq!(format::percent(37.4), "37%");
    assert_eq!(format::percent(-3.0), "0.0%");
    assert_eq!(format::percent(f64::INFINITY), "\u{2014}");
}

#[test]
fn durations_show_the_two_largest_units() {
    assert_eq!(format::duration(0), "0s");
    assert_eq!(format::duration(45), "45s");
    assert_eq!(format::duration(12 * 60 + 5), "12m");
    assert_eq!(format::duration(5 * 3600 + 12 * 60), "5h 12m");
    assert_eq!(format::duration(3 * 86_400 + 4 * 3600 + 59), "3d 4h");
}

#[test]
fn temperatures_round_in_the_chosen_unit() {
    assert_eq!(format::temperature(54.4, TempUnit::Celsius), "54 \u{b0}C");
    assert_eq!(
        format::temperature(54.4, TempUnit::Fahrenheit),
        "130 \u{b0}F"
    );
    assert_eq!(format::temperature(-0.4, TempUnit::Celsius), "0 \u{b0}C");
    assert_eq!(TempUnit::from_config("f"), TempUnit::Fahrenheit);
    assert_eq!(TempUnit::from_config("c"), TempUnit::Celsius);
    assert_eq!(TempUnit::from_config("kelvin"), TempUnit::Celsius);
}
