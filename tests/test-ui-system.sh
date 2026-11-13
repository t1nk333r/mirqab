# tests/test-ui-system.sh — the System page against a real daemon (plan 002,
# Tests): `mirqab-ui --selftest system` on root-desktop shows every card
# with its data and both GPUs in the graphics card; on root-minimal the page still loads,
# without the GPU, pressure and temperature cards, and logs no error.
# mirqab:covers crates/mirqab-ui/src/tabs/system/** crates/mirqab-ui/src/tabs/mod.rs crates/mirqab-view/src/tabs/system.rs
# mirqab:covers tests/fixtures/root-desktop/** tests/fixtures/root-minimal/**
# mirqab:covers-shared

sandbox
build_bins
start_broadway || not_ok "gtk4-broadwayd starts"

# selftest ROOT — the System page's widget tree against a daemon on ROOT.
selftest() {
    stop_daemon
    rm -f "$SANDBOX/daemon.out"
    start_daemon "$1"
    capture timeout 20 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest system
    status_is "--selftest system on $1 exits 0" 0 "$STATUS"
    tree="$(grep -m1 '^{' <<<"$OUTPUT" || true)"
    errors="$(grep -ciE 'critical|error' <<<"$(grep -v '^{' <<<"$OUTPUT")" || true)"
    is "no error lines on $1" 0 "$errors"
}

q() { jq -r "$1" <<<"$tree" 2>/dev/null; }
# shown CLASS — how many visible widgets carry CLASS.
cards() { q '[.. | objects | select(.visible and ((.classes // []) | index("mirqab-card")))] | length'; }
texts() { q '[.. | objects | select(.visible) | .text? // empty] | join("|")'; }

selftest root-desktop
is "only the System page was built" '["system"]' "$(q '.pages | tojson')"
# Overview, CPU, memory, pressure, graphics (both GPUs), temperatures,
# power.
is "seven cards on the desktop" 7 "$(cards)"
like "the thermal word" "$(texts)" "Cool"
like "the memory line" "$(texts)" "17 GB of 67 GB"
like "a temperature group" "$(texts)" "Tctl"
like "both GPUs" "$(texts)" "AMD GPU (card1)"
is "two GPU sections" 2 "$(q '[.. | objects | select(.visible and .classes == ["heading"] and ((.text // "") | test("\\(card")))] | length')"
like "the helper line where package power is unknown" "$(texts)" "package power needs the root helper"
is "graphs are present" 1 "$(q '[.. | objects | select(.type == "MirqabGraph")] | length > 0 | if . then 1 else 0 end')"

selftest root-minimal
# Overview, CPU, memory and power; no GPU, pressure or temperatures.
is "four cards on a minimal machine" 4 "$(cards)"
unlike "no GPU card" "$(texts)" "AMD GPU"

stop_broadway
