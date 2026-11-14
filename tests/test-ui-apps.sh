# tests/test-ui-apps.sh — the Apps page against a real daemon on
# root-desktop (plan 006, part 3, Tests): `mirqab-ui --selftest apps --do
# STEPS` under Broadway lists the golden frame's groups with System last;
# a search leaves one group; selecting it opens the inspector with the
# verdict; Quit shows the dialog naming its five pids, and Go ahead sends
# five TERMs to the recording sender and says it is done; Quit on the
# compositor shows the daemon's refusal in words and no dialog; another
# user's process offers no buttons; and the `apps` and `process`
# subscriptions last only while the page and a process's graph are on
# screen.
# mirqab:covers crates/mirqab-ui/src/tabs/apps/** crates/mirqab-view/src/tabs/apps.rs
# mirqab:covers crates/mirqab-ui/src/selftest.rs crates/mirqab-ui/src/client.rs
# mirqab:covers crates/mirqab-ui/src/widgets/confirm_dialog.rs
# mirqab:covers crates/mirqab-core/src/samplers/process.rs
# mirqab:covers-shared

sandbox
# The daemon on a fixture root gets the recording sender, which writes each
# signal to MIRQAB_ACT_LOG and sends nothing; the caller it refuses is the
# fixture's python3 in a terminal (the interface's real pid has no proc
# entry in the tree).
export MIRQAB_ACT_LOG="$SANDBOX/act.log" MIRQAB_CALLER_PID=3060
: >"$MIRQAB_ACT_LOG"
build_bins
start_broadway || not_ok "gtk4-broadwayd starts"
start_daemon root-desktop

BROWSER='scope:app-Hyprland-browser.sh-5650fedc'

# apps [STEPS] — the Apps page's widget tree after STEPS.
apps() {
    if [[ -n ${1:-} ]]; then
        capture timeout 60 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps --do "$1"
    else
        capture timeout 60 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps
    fi
    status_is "--selftest apps --do '${1:-}' exits 0" 0 "$STATUS"
    tree="$(grep -m1 '^{' <<<"$OUTPUT" || true)"
    errors="$(grep -ciE 'critical|error' <<<"$(grep -v '^{' <<<"$OUTPUT")" || true)"
    is "no error lines after '${1:-}'" 0 "$errors"
    ((errors == 0)) || printf '%s\n' "$OUTPUT" | grep -v '^{' | head -20 >&2
}

q() { jq -r "$1" <<<"$tree" 2>/dev/null; }
texts() { q '[.. | objects | select(.visible) | .text? // empty] | join("|")'; }
groups() { q '[.. | objects | select(.visible and ((.name // "") | startswith("apps-group:"))) | .name] | join(" ")'; }
named() { q "[.. | objects | select(.name? == \"$1\")][0]"; }
act_log() { cat "$MIRQAB_ACT_LOG"; }

apps
is "only the Apps page was built" '["apps"]' "$(q '.pages | tojson')"
is "the golden frame's 24 groups" 24 "$(wc -w <<<"$(groups)")"
like "System is listed" "$(groups)" "apps-group:system"
is "System last" "apps-group:system" "$(awk '{ print $NF }' <<<"$(groups)")"
# The fixture's icon name is in no theme: the kind's glyph stands in
# (plan 006, decision 8; whether a real theme icon shows is an owner check).
row="$(named "apps-group:$BROWSER")"
is "the browser row shows a glyph" "󰘔" "$(jq -r '[.. | objects | select(.visible and .type == "GtkLabel" and ((.classes // []) | index("icon")) and ((.classes // []) | index("mirqab-app-icon")))][0].text' <<<"$row")"
is "and no theme image" 0 "$(jq '[.. | objects | select(.visible and .type == "GtkImage")] | length' <<<"$row")"
is "no inspector before a selection" false "$(q '[.. | objects | select(.name? == "apps-quit")][0].visible')"

apps "type apps-search Browser"
is "a search leaves one group" "apps-group:$BROWSER" "$(groups)"

SELECT="type apps-search Browser; activate apps-group:$BROWSER; wait apps-verdict"
apps "$SELECT; pause 500"
is "the inspector names the group" true "$(q '[.. | objects | select(.visible and .text? == "Example Browser" and ((.classes // []) | index("title-4")))] | length > 0')"
is "with the verdict of its main process" "Packaged by core/bash" "$(named apps-verdict | jq -r .text)"
is "and offers Quit" true "$(named apps-quit | jq -r .visible)"

apps "$SELECT; activate apps-quit; wait apps-dialog"
dialog="$(named apps-dialog)"
body="$(jq -r '[.. | objects | select(.visible) | .text? // empty] | join("|")' <<<"$dialog")"
like "the dialog is in the dump" "$body" "This will quit:"
for pid in 2000 2001 2010 2011 2012; do
    like "the dialog names pid $pid" "$body" "($pid)"
done
is "asking sends nothing" "" "$(act_log)"

apps "$SELECT; activate apps-quit; wait apps-dialog; respond confirm; wait apps-result; pause 300"
is "Go ahead sends five TERMs, the main process first" "TERM 2001 5010
TERM 2000 5000
TERM 2010 5100
TERM 2011 5200
TERM 2012 5300" "$(act_log)"
is "and says so" "Done." "$(named apps-result | jq -r .text)"
: >"$MIRQAB_ACT_LOG"

apps "activate apps-flatten; type apps-search 1050; activate apps-process:1050; activate apps-quit; wait apps-result; pause 300"
like "a refused process: the daemon's reason in words" "$(named apps-result | jq -r .text)" "pid 1050 is the session's compositor"
is "and no dialog" 0 "$(q '[.. | objects | select(.name? == "apps-dialog")] | length')"
is "and nothing sent" "" "$(act_log)"

apps "activate apps-flatten; type apps-search 410; activate apps-process:410; wait apps-verdict"
is "another user's process offers no Quit" false "$(named apps-quit | jq -r .visible)"
like "and says why" "$(texts)" "Another user's processes: mirqab does not quit them"

# --do is for tests only.
capture env MIRQAB_TEST=0 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps --do "pause 1"
status_is "--do outside a test run is refused" 64 "$STATUS"
like "with the reason" "$OUTPUT" "only with MIRQAB_TEST=1"

# Subscriptions follow what is on screen (plan 006, decision 14).
subs() { "$BIN/mirqab" status --json 2>/dev/null | jq -r ".streams.\"$1\".subscribers // 0"; }
# until_subs STREAM N — wait up to 10 s for N subscribers of STREAM.
until_subs() {
    local n=""
    for _ in $(seq 200); do
        n="$(subs "$1")"
        [[ $n == "$2" ]] && break
        sleep 0.05
    done
    printf '%s' "$n"
}
# A process's graph holds one `process` subscriber while it is on screen.
"$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps \
    --do "activate apps-flatten; type apps-search 2001; activate apps-process:2001; wait apps-verdict; pause 6000" \
    >"$SANDBOX/ui1.out" 2>&1 &
ui=$!
is "a selected process: one process subscriber" 1 "$(until_subs process 1)"
is "and one apps subscriber" 1 "$(subs apps)"
wait "$ui" 2>/dev/null || true
is "gone with the window: no process subscriber" 0 "$(until_subs process 0)"
is "and no apps subscriber" 0 "$(until_subs apps 0)"
# A group's graph is fed from the frames: no process subscriber.
"$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps \
    --do "$SELECT; pause 4000" >"$SANDBOX/ui2.out" 2>&1 &
ui=$!
is "a selected group: the page's apps subscriber" 1 "$(until_subs apps 1)"
sleep 1
is "and no process subscriber" 0 "$(subs process)"
wait "$ui" 2>/dev/null || true

# The page holds `apps` while shown and drops it when the window closes.
MIRQAB_UI_EXIT_MS=60000 "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --background >"$SANDBOX/ui.out" 2>&1 &
ui=$!
for _ in $(seq 100); do
    [[ -n "$("$BIN/mirqab" status --json 2>/dev/null | jq -r '.ui.pid // empty')" ]] && break
    sleep 0.02
done
"$BIN/mirqab" open apps --window >/dev/null
is "Apps shown: one apps subscriber" 1 "$(until_subs apps 1)"
"$BIN/mirqab" close >/dev/null
is "closed: no apps subscriber" 0 "$(until_subs apps 0)"
kill "$ui" 2>/dev/null || true
wait "$ui" 2>/dev/null || true
for f in ui.out ui1.out ui2.out; do
    unlike "no GTK criticals in $f" "$(<"$SANDBOX/$f")" "CRITICAL"
done

stop_daemon
stop_broadway
