//! Home's view model (plan 003, Tests): the seed, moving up and down,
//! dragging,
//! pinning, unknown and unserved ids skipped, the union of streams, and
//! the two-column layout.

use mirqab_view::cards;
use mirqab_view::tabs::home;

fn v(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_first_run_is_seeded_once() {
    assert_eq!(
        home::seed(&[], false),
        Some(v(&["system.overview", "system.cpu", "system.memory"]))
    );
    assert_eq!(
        home::seed(&[], true),
        None,
        "emptied by the user: stays empty"
    );
    assert_eq!(home::seed(&v(&["system.cpu"]), false), None);
    for id in home::SEED {
        assert!(cards::find(id).is_some(), "{id} is a card");
    }
}

#[test]
fn cards_move_up_and_down_and_toggle() {
    let p = v(&["a", "b", "c"]);
    assert_eq!(home::moved(&p, "b", true), v(&["b", "a", "c"]));
    assert_eq!(home::moved(&p, "b", false), v(&["a", "c", "b"]));
    assert_eq!(home::moved(&p, "a", true), p, "first stays first");
    assert_eq!(home::moved(&p, "c", false), p, "last stays last");
    assert_eq!(home::moved(&p, "zz", true), p);
    assert_eq!(
        home::dropped(&p, "c", "a"),
        v(&["c", "a", "b"]),
        "dragged up"
    );
    assert_eq!(
        home::dropped(&p, "a", "c"),
        v(&["b", "c", "a"]),
        "dragged down"
    );
    assert_eq!(home::dropped(&p, "a", "zz"), p);
    assert_eq!(home::toggled(&p, "d", true), v(&["a", "b", "c", "d"]));
    assert_eq!(home::toggled(&p, "b", false), v(&["a", "c"]));
    assert_eq!(
        home::toggled(&p, "a", true),
        v(&["b", "c", "a"]),
        "re-pinning moves it last"
    );
}

#[test]
fn unknown_and_unserved_cards_are_skipped_not_dropped() {
    let pinned = v(&["system.cpu", "gone.card", "system.memory"]);
    let shown = home::shown(&pinned, &v(&["system", "self", "ui"]));
    let ids: Vec<&str> = shown.iter().map(|c| c.id).collect();
    assert_eq!(ids, ["system.cpu", "system.memory"]);
    assert!(
        home::shown(&pinned, &v(&["self"])).is_empty(),
        "no system stream"
    );
    assert_eq!(home::streams(&shown), ["system"]);
    assert!(home::streams(&[]).is_empty());
}

#[test]
fn wide_cards_take_a_row_and_half_cards_pair_up() {
    let cs = home::shown(
        &v(&[
            "system.memory",
            "system.cpu",
            "system.temps",
            "system.power",
            "system.gpu",
        ]),
        &v(&["system"]),
    );
    // memory (half), cpu (wide), temps + power (half, half), gpu (half).
    assert_eq!(
        home::layout(&cs),
        [(0, 0, 1), (1, 0, 2), (2, 0, 1), (2, 1, 1), (3, 0, 1)]
    );
}
