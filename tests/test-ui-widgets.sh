# tests/test-ui-widgets.sh — the widget tests of crates/mirqab-ui/tests
# (architecture §7.5, layer 2) on a private Broadway display. They are
# #[ignore]d in a plain `cargo test`, which has no display.
# mirqab:covers crates/mirqab-ui/** crates/mirqab-view/**
# mirqab:covers-shared

sandbox
if ! start_broadway; then
    not_ok "gtk4-broadwayd starts"
else
    # #[gtk::test] runs the tests on GLib's thread pool, where a failed
    # assertion aborts; without a backtrace its message stays in the tail.
    # GTK's test accessibility backend, not none: the Apps list's selected
    # state is read back from the list item (plan 006, part 4b, N8). It
    # talks to no bus.
    capture env RUST_BACKTRACE=0 GTK_A11Y=test cargo test --locked --quiet --manifest-path "$REPO/Cargo.toml" -p mirqab-ui --test widgets -- --ignored --test-threads=1
    status_is "widget tests pass under Broadway" 0 "$STATUS"
    ((STATUS == 0)) || printf '%s\n' "$OUTPUT" | tail -60 >&2
    stop_broadway
fi
