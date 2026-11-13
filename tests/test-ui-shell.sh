# tests/test-ui-shell.sh — the window against a real daemon on root-desktop
# (plan 001, Tests): `mirqab-ui --selftest home` under Broadway shows the
# sixteen tiles, the footer's cost and machine line, and builds no page but
# Home; a hidden interface quits after its exit delay; `open` and `close`
# reach it as events.
# mirqab:covers crates/mirqab-ui/src/{main,lib,app,client,window,pages,style,selftest}.rs
# mirqab:covers crates/mirqab-ui/src/widgets/{sidebar,footer}.rs crates/mirqab-ui/src/tabs/**
# mirqab:covers crates/mirqab-ui/resources/** tests/fixtures/root-desktop/**
# mirqab:covers-shared

sandbox
build_bins
start_daemon root-desktop
start_broadway || not_ok "gtk4-broadwayd starts"

capture timeout 20 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest home
status_is "--selftest home exits 0" 0 "$STATUS"
tree="$(grep -m1 '^{' <<<"$OUTPUT" || true)"
q() { jq -r "$1" <<<"$tree" 2>/dev/null; }
is "only Home's page was built" '["home"]' "$(q '.pages | tojson')"
is "sixteen tiles" 16 "$(q '[.. | objects | select((.classes // []) | index("mirqab-tile"))] | length')"
is "Home's tile is active" Home "$(q '[.. | objects | select((.classes // []) | index("active"))][0] | [.. | objects | select(.text? and (.classes | index("caption")))][0].text')"
like "the footer shows the cost" "$(q '[.. | objects | select((.classes // []) | index("mirqab-cost"))][0].text')" "self "
is "the footer shows the machine line" "Example Desktop · up 1h 0m" "$(q '[.. | objects | select((.classes // []) | index("mirqab-machine"))][0].text')"
is "the page slot holds one page" 1 "$(q '[.. | objects | select((.classes // []) | index("mirqab-page"))] | length')"
like "Home's placeholder names its plan" "$(q '[.. | objects | .text? // empty] | join("|")')" "Plan 003 fills this tab."
is "no tile tooltip is missing" 0 "$(q '[.. | objects | select((.classes // []) | index("mirqab-tile")) | select(.tooltip == null)] | length')"

capture timeout 20 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest nonsense
status_is "an unknown tab still shows the window" 0 "$STATUS"

# A started interface waits hidden, opens on `open`, hides on `close`, and
# quits after its exit delay (5 s here, honoured only under MIRQAB_TEST).
MIRQAB_UI_EXIT_MS=5000 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --background >"$SANDBOX/ui.out" 2>&1 &
ui=$!
attached=""
for ((i = 0; i < 100; i++)); do
    attached="$("$BIN/mirqab" status --json 2>/dev/null | jq -r '.ui.pid // empty')"
    [[ -n $attached ]] && break
    # Started hidden, it quits after its exit delay unless shown before.
    sleep 0.02
done
is "the interface attached as the ui client" "$ui" "$attached"
capture "$BIN/mirqab" open system --window
status_is "open reaches it" 0 "$STATUS"
# Past the exit delay of its hidden start: `open` cancelled that timer.
sleep 6
kill -0 "$ui" 2>/dev/null && ok || not_ok "it stays while shown, past the delay"
capture "$BIN/mirqab" close
status_is "close reaches it" 0 "$STATUS"
gone=""
for ((i = 0; i < 160; i++)); do
    kill -0 "$ui" 2>/dev/null || { gone=1; break; }
    sleep 0.05
done
is "a hidden interface quits after its exit delay" 1 "$gone"
wait "$ui" 2>/dev/null || true
unlike "no GTK criticals" "$(<"$SANDBOX/ui.out")" "CRITICAL"
stop_broadway
