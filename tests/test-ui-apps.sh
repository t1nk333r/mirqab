# tests/test-ui-apps.sh — the Apps page against a real daemon on
# root-desktop (plan 006, part 3, Tests): `mirqab-ui --selftest apps --do
# STEPS` under Broadway lists the golden frame's groups with System last;
# a search leaves one group; selecting it opens the inspector with the
# verdict; Quit shows the dialog naming its five pids, and Go ahead sends
# five TERMs to the recording sender and says it is done; Quit on the
# compositor shows the daemon's refusal in words and no dialog; another
# user's process offers no buttons; the `apps` and `process`
# subscriptions last only while the page and a process's graph are on
# screen; an act's outcome outlives the row it ended (review of PR #19,
# B1); and without MIRQAB_TEST=1 a running interface takes no `--do` and
# no `--selftest` over the session bus (review of PR #19, S1).
#
# Part 4b: the interface asks for a process by pid and start time, in the
# `process` subscription and in `get apps.inspect` (seen on a logging
# proxy in front of the daemon; gap 6 of the review of PR #19); without a
# focus source no row is tagged and `status.focus` is null; with a fake
# Hyprland in a copy of the root focusing the browser, and two busy
# groups, the unfocused one (cargo) shows "busy while not in use" and the
# focused browser does not, `status.focus` says hyprland while the page
# is open, and once it closes `focus` is null and no `focus-hyprland`
# thread is left in the daemon.
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
# A copy of the fixture root, never the committed one: the last check
# removes processes from it between frames.
cp -a "$FIXTURES/root-desktop" "$SANDBOX/root"
start_daemon "$SANDBOX/root"

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
is "no focus source: no row is tagged" 0 "$(q '[.. | objects | select(.visible and .text? == "busy while not in use")] | length')"
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
# A logging proxy in front of the daemon: what the interface asks, line by
# line (gap 6 of the review of PR #19).
cat >"$SANDBOX/proxy.py" <<'PY'
# Forwards a Unix socket to another, appending what clients send to a log.
import socket
import sys
import threading
listen, upstream, log = sys.argv[1:4]
lock = threading.Lock()
srv = socket.socket(socket.AF_UNIX)
srv.bind(listen)
srv.listen(8)
def pump(a, b, record):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            b.sendall(d)
            if record:
                with lock, open(log, "ab") as f:
                    f.write(d)
    except OSError:
        pass
    finally:
        for x in (a, b):
            try:
                x.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
while True:
    c, _ = srv.accept()
    u = socket.socket(socket.AF_UNIX)
    u.connect(upstream)
    threading.Thread(target=pump, args=(c, u, True), daemon=True).start()
    threading.Thread(target=pump, args=(u, c, False), daemon=True).start()
PY
PROXY="$MIRQAB_RUN_DIR/proxy.sock"
: >"$SANDBOX/proxy.log"
python3 -I "$SANDBOX/proxy.py" "$PROXY" "$MIRQAB_SOCKET" "$SANDBOX/proxy.log" &
proxy=$!
HELPER_PIDS+=("$proxy")
for _ in $(seq 100); do
    [[ -S $PROXY ]] && break
    sleep 0.02
done
# A process's graph holds one `process` subscriber while it is on screen.
"$BIN/mirqab-ui" --socket "$PROXY" --selftest apps \
    --do "activate apps-flatten; type apps-search 2001; activate apps-process:2001; wait apps-verdict; pause 6000" \
    >"$SANDBOX/ui1.out" 2>&1 &
ui=$!
is "a selected process: one process subscriber" 1 "$(until_subs process 1)"
is "and one apps subscriber" 1 "$(subs apps)"
is "no focus source: status.focus is null" null "$("$BIN/mirqab" status --json | jq -c .focus)"
wait "$ui" 2>/dev/null || true
kill "$proxy" 2>/dev/null || true
wait "$proxy" 2>/dev/null || true
asked() { jq -c "select($1) | .params" "$SANDBOX/proxy.log" 2>/dev/null | head -n1; }
is "the graph subscribes by pid and start" '{"interval":1000,"live":true,"pid":2001,"start":5010}' \
    "$(asked '.method == "subscribe" and .stream == "process"')"
is "the inspector asks by pid and start" '{"pid":2001,"start":5010}' \
    "$(asked '.method == "get" and .stream == "apps.inspect"')"
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

# Arguments forwarded to a running interface over the session bus (review
# of PR #19, S1): without MIRQAB_TEST=1, `--do` and `--selftest` are
# refused with 64 and nothing reaches the daemon; a plain `--tab` still
# shows. The interface is unique on a private bus with no service
# directories (so nothing is activated), and the call is built with
# GLib's own variant API, the shape a second `mirqab-ui` sends.
if command -v dbus-run-session >/dev/null 2>&1 &&
    /usr/bin/python3 -I -c 'import gi; gi.require_version("Gio", "2.0"); from gi.repository import Gio' 2>/dev/null; then
    cat >"$SANDBOX/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$MIRQAB_RUN_DIR</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
    cat >"$SANDBOX/fwd.py" <<'PY'
# One org.gtk.Application.CommandLine call to the running interface.
import sys
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
args = GLib.Variant("aay", [a.encode() + b"\0" for a in sys.argv[1:]])
params = GLib.Variant.new_tuple(GLib.Variant("o", "/org/gtk/cl"), args, GLib.Variant("a{sv}", {}))
r = bus.call_sync("io.github.t1nk333r.mirqab", "/io/github/t1nk333r/mirqab",
                  "org.gtk.Application", "CommandLine", params, None,
                  Gio.DBusCallFlags.NONE, 10000, None)
print(r.unpack()[0])
PY
    : >"$MIRQAB_ACT_LOG"
    ACTING="activate apps-flatten; type apps-search 2001; activate apps-process:2001; wait apps-verdict; activate apps-quit; wait apps-dialog; respond confirm; wait apps-result"
    export ACTING SANDBOX BIN MIRQAB_SOCKET
    env -u MIRQAB_TEST GTK_USE_PORTAL=0 dbus-run-session --config-file="$SANDBOX/bus.conf" -- bash -c '
        fwd() { /usr/bin/python3 -I "$SANDBOX/fwd.py" "$@" 2>>"$SANDBOX/fwd.err" || echo failed; }
        "$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --window >"$SANDBOX/unique.out" 2>&1 &
        ui=$!
        # Up once it answers a plain forward.
        for _ in $(seq 100); do
            [[ $(fwd mirqab-ui --tab apps) == 0 ]] && break
            sleep 0.1
        done
        echo "tab=$(fwd mirqab-ui --tab apps)"
        echo "do=$(fwd mirqab-ui --selftest apps --do "$ACTING")"
        echo "selftest=$(fwd mirqab-ui --selftest apps)"
        sleep 1
        kill -0 "$ui" 2>/dev/null && echo alive=yes || echo alive=no
        kill "$ui" 2>/dev/null
        wait "$ui" 2>/dev/null
        true
    ' >"$SANDBOX/fwd.out" 2>&1
    fwd_out="$(<"$SANDBOX/fwd.out")"
    like "a forwarded --tab is taken" "$fwd_out" "tab=0"
    like "a forwarded --do is refused" "$fwd_out" "do=64"
    like "a forwarded --selftest is refused" "$fwd_out" "selftest=64"
    like "and the running interface is still there" "$fwd_out" "alive=yes"
    is "nothing reached the daemon" "" "$(act_log)"
    is "no widget tree was dumped" 0 "$(grep -c '^{' "$SANDBOX/unique.out" || true)"
    unlike "no GTK criticals in the unique interface" "$(<"$SANDBOX/unique.out")" "CRITICAL"
    like "the interface said why" "$(<"$SANDBOX/unique.out")" "only with MIRQAB_TEST=1"
elif [[ -n ${CI:-} ]]; then
    # CI installs both (ci.yml), so a skip there would hide this check.
    not_ok "dbus-run-session and python3-gobject are installed for the forwarding check"
else
    echo "# skipped the forwarding check: no dbus-run-session or python3-gobject" >&2
fi

# An act's outcome outlives the row it ended (review of PR #19, B1). The
# recording sender ends nothing, so the processes are taken out of the
# copied root by hand once the TERMs are recorded; the next frames lack
# the row.
: >"$MIRQAB_ACT_LOG"
"$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps \
    --do "$SELECT; activate apps-quit; wait apps-dialog; respond confirm; wait apps-result; pause 6000" \
    >"$SANDBOX/ui3.out" 2>&1 &
ui=$!
for _ in $(seq 400); do
    (($(wc -l <"$MIRQAB_ACT_LOG") >= 5)) && break
    sleep 0.05
done
is "the five TERMs are recorded" 5 "$(wc -l <"$MIRQAB_ACT_LOG")"
rm -r "$SANDBOX/root/proc/"{2000,2001,2010,2011,2012}
wait "$ui" 2>/dev/null || true
tree="$(grep -m1 '^{' "$SANDBOX/ui3.out" || true)"
unlike "the ended app is no longer listed" "$(groups)" "apps-group:$BROWSER"
is "its sheet closed" false "$(named apps-inspector | jq -r .visible)"
is "the outcome is shown above the list" true "$(named apps-notice | jq -r .visible)"
like "naming the app" "$(named apps-notice | jq -r .text)" "Example Browser: "
like "and what came of it" "$(named apps-notice | jq -r .text)" "Done."

# "Idle but heavy" with a fake Hyprland (plan 006, part 4b). A second
# daemon on a copy of the root under the short runtime dir, so the fake's
# socket paths stay under 108 bytes; on a fixture root the daemon finds
# Hyprland only in the tree's run/user/1000/hypr/, never the session's.
stop_daemon
HROOT="$MIRQAB_RUN_DIR/r"
cp -a "$FIXTURES/root-desktop" "$HROOT"
HDIR="$HROOT/run/user/1000/hypr/fake"
mkdir -p "$HDIR"
# The lock names the fixture's Hyprland, pid 1050, which has a proc entry.
printf '1050\nwayland-1\n' >"$HDIR/hyprland.lock"
cat >"$SANDBOX/hypr.py" <<'PY'
# A fake Hyprland: the event socket says the browser's window has focus,
# and the request socket answers j/activewindow with its pid. No title or
# class is needed by the reader; the fake sends synthetic ones.
import json
import socket
import sys
import threading
d, pid = sys.argv[1], int(sys.argv[2])
def listener(name):
    s = socket.socket(socket.AF_UNIX)
    s.bind(d + "/" + name)
    s.listen(4)
    return s
events, requests = listener(".socket2.sock"), listener(".socket.sock")
def answer():
    while True:
        c, _ = requests.accept()
        try:
            c.settimeout(2)
            if c.recv(4096).startswith(b"j/activewindow"):
                c.sendall(json.dumps({"address": "0x5633a0b1c2d3", "pid": pid,
                                      "class": "example", "title": "example",
                                      "focusHistoryID": 0}).encode())
        except OSError:
            pass
        finally:
            c.close()
def serve(c):
    try:
        c.sendall(b"activewindowv2>>5633a0b1c2d3\n")
        while c.recv(4096):
            pass
    except OSError:
        pass
    finally:
        c.close()
threading.Thread(target=answer, daemon=True).start()
while True:
    c, _ = events.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
PY
python3 -I "$SANDBOX/hypr.py" "$HDIR" 2001 &
hypr=$!
HELPER_PIDS+=("$hypr")
for _ in $(seq 100); do
    [[ -S $HDIR/.socket.sock ]] && break
    sleep 0.02
done
# Two busy groups: the browser (focused) and cargo's rustc (not), each a
# core's worth of ticks a second. Each stat is replaced whole, so the
# daemon never reads half a file.
bump() {
    local t=0 pid
    while :; do
        t=$((t + 50))
        for pid in 2001 3041; do
            awk -v add="$t" '{ $14 += add; print }' "$FIXTURES/root-desktop/proc/$pid/stat" >"$HROOT/proc/$pid/stat.new"
            mv -f "$HROOT/proc/$pid/stat.new" "$HROOT/proc/$pid/stat"
        done
        sleep 0.5
    done
}
bump &
bumper=$!
HELPER_PIDS+=("$bumper")
rm -f "$SANDBOX/daemon.out"
MIRQAB_HEAVY_WINDOW_MS=4000 start_daemon "$HROOT"
"$BIN/mirqab-ui" --socket "$MIRQAB_SOCKET" --selftest apps --do "pause 11000" >"$SANDBOX/ui4.out" 2>&1 &
ui=$!
focus=""
for _ in $(seq 200); do
    focus="$("$BIN/mirqab" status --json 2>/dev/null | jq -r '.focus.source // empty')"
    [[ -n $focus ]] && break
    sleep 0.05
done
is "the page open: status.focus is hyprland" hyprland "$focus"
is "and the reader thread runs" 1 "$(cat /proc/"$DAEMON_PID"/task/*/comm 2>/dev/null | grep -cx focus-hyprland || true)"
wait "$ui" 2>/dev/null || true
tree="$(grep -m1 '^{' "$SANDBOX/ui4.out" || true)"
tag() { q "[.. | objects | select(.name? == \"apps-group:$1\")][0] | [.. | objects | select(.visible and .text? == \"busy while not in use\")] | length"; }
is "cargo, busy and not in focus, is tagged" 1 "$(tag session:3030)"
is "the focused browser is not" 0 "$(tag "$BROWSER")"
is "closed: status.focus is null" null "$(for _ in $(seq 200); do
    f="$("$BIN/mirqab" status --json 2>/dev/null | jq -c .focus)"
    [[ $f == null ]] && break
    sleep 0.05
done
printf '%s' "$f")"
is "and no focus-hyprland thread is left" 0 "$(cat /proc/"$DAEMON_PID"/task/*/comm 2>/dev/null | grep -cx focus-hyprland || true)"
kill "$bumper" "$hypr" 2>/dev/null || true
wait "$bumper" "$hypr" 2>/dev/null || true

for f in ui.out ui1.out ui2.out ui3.out ui4.out; do
    unlike "no GTK criticals in $f" "$(<"$SANDBOX/$f")" "CRITICAL"
done

stop_daemon
stop_broadway
