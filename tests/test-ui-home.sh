# tests/test-ui-home.sh — Home against a real daemon (plan 003, Tests): a
# fresh config seeds three cards and stores them; a card pinned with
# `config.set` (what the pin control sends) shows on Home and its control
# on the System page reads "Unpin from Home"; unpinning everything leaves
# the empty state, and Home is not seeded again.
# mirqab:covers crates/mirqab-ui/src/tabs/home/** crates/mirqab-ui/src/tabs/mod.rs
# mirqab:covers crates/mirqab-view/src/tabs/home.rs crates/mirqab-view/src/cards.rs
# mirqab:covers crates/mirqab-ui/src/app.rs crates/mirqab-core/src/config.rs
# mirqab:covers-shared

sandbox
build_bins
start_broadway || not_ok "gtk4-broadwayd starts"
start_daemon root-desktop
# A daemon on a fixture root keeps its config beside its socket.
CONFIG="$(dirname "$MIRQAB_SOCKET")/config.json"

# selftest TAB — the widget tree of TAB's page against the running daemon.
selftest() {
    capture timeout 20 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest "$1"
    status_is "--selftest $1 exits 0" 0 "$STATUS"
    tree="$(grep -m1 '^{' <<<"$OUTPUT" || true)"
    errors="$(grep -ciE 'critical|error' <<<"$(grep -v '^{' <<<"$OUTPUT")" || true)"
    is "no error lines on $1" 0 "$errors"
}

# set KEY JSON — one `config.set` on the daemon's socket.
set_config() {
    local reply
    reply="$(printf '{"id":1,"method":"config.set","params":{"key":"%s","value":%s}}\n' "$1" "$2" |
        timeout 5 socat -t 2 - "UNIX-CONNECT:$MIRQAB_SOCKET")"
    like "config.set $1 is answered" "$reply" '"type":"reply"'
    unlike "config.set $1 is accepted" "$reply" '"error"'
}

# stored KEY — a value in config.json, waiting up to 2 s for the write.
stored() {
    local v=""
    for _ in $(seq 20); do
        v="$(jq -c "$1" "$CONFIG" 2>/dev/null || true)"
        [[ -n $v && $v != null ]] && break
        sleep 0.1
    done
    printf '%s' "$v"
}

q() { jq -r "$1" <<<"$tree" 2>/dev/null; }
cards() { q '[.. | objects | select(.visible and ((.classes // []) | index("mirqab-card")))] | length'; }
texts() { q '[.. | objects | select(.visible) | .text? // empty] | join("|")'; }
# pickers — visible range pickers: a linked box of toggle buttons (the
# window's own pin toggle stands alone).
pickers() { q '[.. | objects | select(.visible and ((.classes // []) | index("linked")) and any(.children[]?; .type == "GtkToggleButton"))] | length'; }
unpins() { q '[.. | objects | select(.visible and .tooltip == "Unpin from Home")] | length'; }

selftest home
is "only Home was built" '["home"]' "$(q '.pages | tojson')"
is "three seeded cards" 3 "$(cards)"
like "the overview strip" "$(texts)" "Cool"
like "the memory card" "$(texts)" "17 GB of 67 GB"
is "the seed is stored" '["system.overview","system.cpu","system.memory"]' "$(stored .home.pinned)"
is "seeded once" true "$(stored .home.seeded)"
is "no range picker on Home" 0 "$(pickers)"

set_config home.pinned '["system.overview","system.cpu","system.memory","system.gpu"]'
selftest home
is "four cards with the GPUs pinned" 4 "$(cards)"
like "the graphics card" "$(texts)" "AMD GPU (card1)"
selftest system
is "the System page marks the four pinned cards" 4 "$(unpins)"
is "the System page has range pickers (the control for Home's check)" true "$(q '[.. | objects | select(.visible and ((.classes // []) | index("linked")) and any(.children[]?; .type == "GtkToggleButton"))] | length > 0')"

set_config home.pinned '[]'
selftest home
is "no cards when everything is unpinned" 0 "$(cards)"
like "the empty state" "$(texts)" "Nothing is pinned"
is "not seeded again" '[]' "$(stored .home.pinned)"

# Pinned from System before Home ever opened: that counts as seeded, so
# emptying Home later leaves it empty (review of PR #9).
stop_daemon
rm -f "$CONFIG" "$SANDBOX/daemon.out"
start_daemon root-desktop
set_config home.pinned '["system.cpu"]'
selftest home
is "one card pinned from a tab" 1 "$(cards)"
is "marked seeded" true "$(stored .home.seeded)"
set_config home.pinned '[]'
selftest home
is "an emptied Home stays empty" 0 "$(cards)"

# Subscriptions follow what is on screen (plan 003, acceptance 3): Home
# holds `system` while shown; with System shown instead the daemon has
# exactly one `system` subscriber; after `close` it has none.
set_config home.pinned '["system.overview","system.cpu"]'
subs() { "$BIN/mirqab" status --json 2>/dev/null | jq -r '.streams.system.subscribers // 0'; }
# until N — wait up to 5 s for N `system` subscribers.
until_subs() {
    local n=""
    for _ in $(seq 100); do
        n="$(subs)"
        [[ $n == "$1" ]] && break
        sleep 0.05
    done
    printf '%s' "$n"
}
MIRQAB_UI_EXIT_MS=60000 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --background >"$SANDBOX/ui.out" 2>&1 &
ui=$!
for _ in $(seq 100); do
    [[ -n "$("$BIN/mirqab" status --json 2>/dev/null | jq -r '.ui.pid // empty')" ]] && break
    sleep 0.02
done
"$BIN/mirqab" open --window >/dev/null
is "Home on screen holds system" 1 "$(until_subs 1)"
"$BIN/mirqab" open system --window >/dev/null
sleep 1
is "System shown, Home hidden: one system subscriber" 1 "$(until_subs 1)"
"$BIN/mirqab" close >/dev/null
is "closed: no system subscriber" 0 "$(until_subs 0)"
kill "$ui" 2>/dev/null || true
wait "$ui" 2>/dev/null || true
unlike "no GTK criticals" "$(<"$SANDBOX/ui.out")" "CRITICAL"

stop_daemon
stop_broadway
