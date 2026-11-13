# shellcheck shell=bash disable=SC2034  # OUTPUT/STATUS/FIXTURES are read by the test files
# tests/lib.sh — assertions and sandbox helpers for tests/test-*.sh.
# Sourced by tests/run.sh into each test file's subshell.
#
# Adapted from haseen tests/lib.sh (MIT, (c) 2026 t1nk33r; NOTICE.md).
#
# Every test runs hermetically:
#   - fixture trees under tests/fixtures/ stand for `/` (architecture §5.3),
#     so nothing reads the live machine;
#   - HOME, XDG_*, XDG_RUNTIME_DIR and TMPDIR point into a scratch directory
#     under the caller's ${TMPDIR:-/tmp}, never inside the checkout, removed
#     when the file ends;
#   - PATH starts with a stub directory where every state-changing binary
#     prints STUB-CALLED and exits 97, so a dry run's purity is "the output
#     never contains STUB-CALLED";
#   - the session bus and the compositor are unreachable.

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="$REPO/tests/fixtures"

TESTS_RUN=0
TESTS_FAILED=0

# Binaries a test must never execute for real. Read-only tools the tests use
# (jq, sed, grep, git, cargo) are deliberately absent.
STUBBED_CMDS=(sudo pkexec systemctl systemd-run systemd-inhibit loginctl
    busctl notify-send xdg-open curl wget hyprctl pactl wpctl kill pkill)

# The toolchain lives in the real HOME. Capture it before the sandbox moves
# HOME, so cargo neither downloads the registry again nor writes into it.
: "${CARGO_HOME:=${HOME:-}/.cargo}"
: "${RUSTUP_HOME:=${HOME:-}/.rustup}"
export CARGO_HOME RUSTUP_HOME
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"

# Scratch trees go under the caller's TMPDIR, captured once: sandbox points
# TMPDIR into the sandbox, and a second sandbox call must not nest in it.
MIRQAB_SCRATCH_PARENT="${TMPDIR:-/tmp}"

SCRATCH=""
_cleanup_scratch() {
    # A daemon a test left running dies with its sandbox.
    [[ -z ${DAEMON_PID:-} ]] || { kill "$DAEMON_PID" 2>/dev/null || true; wait "$DAEMON_PID" 2>/dev/null || true; }
    [[ -z ${BROADWAY_PID:-} ]] || { kill "$BROADWAY_PID" 2>/dev/null || true; wait "$BROADWAY_PID" 2>/dev/null || true; }
    [[ -z ${MIRQAB_RUN_DIR:-} || ! -d $MIRQAB_RUN_DIR ]] || rm -rf -- "$MIRQAB_RUN_DIR"
    [[ -z $SCRATCH || ! -d $SCRATCH ]] || rm -rf -- "$SCRATCH"
}
trap _cleanup_scratch EXIT

# sandbox — a fresh scratch HOME, runtime dir and stub PATH; sets SANDBOX.
# Call it once per test file, before anything that could read the session.
sandbox() {
    _cleanup_scratch
    SCRATCH="$(mktemp -d "$MIRQAB_SCRATCH_PARENT/mirqab-test.XXXXXX")"
    SANDBOX="$SCRATCH"
    mkdir -p "$SANDBOX/home" "$SANDBOX/stubs" "$SANDBOX/tmp"
    # The runtime dir holds sockets, whose paths must stay under 108 bytes,
    # so it lives under /tmp whatever the caller's TMPDIR, and goes with the
    # sandbox.
    MIRQAB_RUN_DIR="$(mktemp -d /tmp/mqr.XXXXXX)"
    ln -s "$MIRQAB_RUN_DIR" "$SANDBOX/run"
    # What runs under a test writes its temporary files here, not to /tmp.
    export TMPDIR="$SANDBOX/tmp"
    local c
    for c in "${STUBBED_CMDS[@]}"; do
        printf '#!/bin/sh\necho "STUB-CALLED: %s $*" >&2\nexit 97\n' "$c" >"$SANDBOX/stubs/$c"
        chmod +x "$SANDBOX/stubs/$c"
    done
    export HOME="$SANDBOX/home"
    export XDG_CONFIG_HOME="$HOME/.config" XDG_STATE_HOME="$HOME/.local/state"
    export XDG_DATA_HOME="$HOME/.local/share" XDG_CACHE_HOME="$HOME/.cache"
    export XDG_RUNTIME_DIR="$MIRQAB_RUN_DIR"
    unset XDG_DATA_DIRS
    export PATH="$SANDBOX/stubs:$CARGO_HOME/bin:/usr/bin:/bin"
    # Nothing under test may reach the session it runs in: a dead bus address
    # and no compositor sockets make a leak fail inside the test instead.
    export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
    unset DBUS_SYSTEM_BUS_ADDRESS WAYLAND_DISPLAY DISPLAY \
        HYPRLAND_INSTANCE_SIGNATURE MIRQAB_SOCKET NOTIFY_SOCKET LISTEN_FDS LISTEN_PID
    # Coding agents' credential homes: the Agents tab (plan 008) must never
    # reach the live account's.
    unset CLAUDE_CONFIG_DIR CODEX_HOME
    export LC_ALL=C MIRQAB_TEST=1
}

# stub CMD SCRIPT — replace a stub with a scripted fake. SCRIPT is the body
# of a /bin/sh script; $STUB_LOG, when set, is where fakes record their argv.
stub() {
    printf '#!/bin/sh\n%s\n' "$2" >"$SANDBOX/stubs/$1"
    chmod +x "$SANDBOX/stubs/$1"
}

_pass() { TESTS_RUN=$((TESTS_RUN + 1)); }
_fail() {
    TESTS_RUN=$((TESTS_RUN + 1))
    TESTS_FAILED=$((TESTS_FAILED + 1))
    printf '  FAIL %s\n' "$1" >&2
    [[ -z ${2:-} ]] || printf '%s\n' "$2" | sed 's/^/       /' >&2
}

ok() { _pass; }
not_ok() { _fail "$@"; }

is() { # LABEL EXPECTED ACTUAL
    if [[ $2 == "$3" ]]; then _pass; else _fail "$1" "expected: $2"$'\n'"actual:   $3"; fi
}

like() { # LABEL HAYSTACK NEEDLE
    if [[ $2 == *"$3"* ]]; then _pass; else _fail "$1" "missing: $3"$'\n'"in: ${2:0:2000}"; fi
}

unlike() { # LABEL HAYSTACK NEEDLE
    if [[ $2 != *"$3"* ]]; then _pass; else _fail "$1" "unexpected: $3"$'\n'"in: ${2:0:2000}"; fi
}

status_is() { # LABEL EXPECTED_CODE ACTUAL_CODE
    if [[ $2 == "$3" ]]; then _pass; else _fail "$1" "exit: expected $2, got $3"; fi
}

# capture CMD... — run CMD, set OUTPUT (stdout and stderr) and STATUS.
capture() {
    set +e
    OUTPUT="$("$@" 2>&1)"
    STATUS=$?
    set -e
}

# build_bins — build the workspace's binaries once per test file; sets BIN to
# the directory that holds them.
build_bins() {
    [[ -n ${BIN:-} ]] && return 0
    cargo build --locked --quiet --manifest-path "$REPO/Cargo.toml" --bins
    BIN="$CARGO_TARGET_DIR/debug"
}

# start_daemon ROOT — run `mirqab daemon` on the fixture tree ROOT (a name
# under tests/fixtures or a path) in the sandbox; waits for its `ready` line
# and sets DAEMON_PID and MIRQAB_SOCKET. stop_daemon ends it; the scratch
# cleanup on exit ends it too.
start_daemon() {
    local root=$1 line=""
    [[ $root == /* ]] || root="$FIXTURES/$root"
    build_bins
    export MIRQAB_SOCKET="$XDG_RUNTIME_DIR/mirqab/daemon.sock"
    mkfifo "$SANDBOX/daemon.out"
    "$BIN/mirqab" daemon --root "$root" >"$SANDBOX/daemon.out" 2>"$SANDBOX/daemon.err" &
    DAEMON_PID=$!
    # The fifo blocks until the daemon opens it; its first line is `ready`.
    if ! read -r -t 10 line <"$SANDBOX/daemon.out" || [[ $line != "ready $MIRQAB_SOCKET" ]]; then
        echo "start_daemon: no ready line (got '$line'):" >&2
        cat "$SANDBOX/daemon.err" >&2 || true
        return 1
    fi
}

stop_daemon() {
    [[ -n ${DAEMON_PID:-} ]] || return 0
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
    DAEMON_PID=""
}

# start_broadway — run gtk4-broadwayd on a free display number in the
# sandbox (architecture §7.5) and point GTK at it: GDK_BACKEND=broadway,
# BROADWAY_DISPLAY, GSK_RENDERER=cairo, GTK_A11Y=none. Sets BROADWAY_PID;
# stop_broadway or the scratch cleanup ends it. Returns 1 when
# gtk4-broadwayd is missing or does not come up.
start_broadway() {
    command -v gtk4-broadwayd >/dev/null 2>&1 || { echo "start_broadway: gtk4-broadwayd not found" >&2; return 1; }
    local n i
    # Display N listens on TCP port 8080 + N and on a socket in the runtime
    # dir, which is the sandbox's: a number another broadwayd holds makes it
    # exit, and the next number is tried.
    for ((i = 0; i < 20; i++)); do
        n=$((RANDOM % 900 + 50))
        gtk4-broadwayd ":$n" >"$SANDBOX/broadway.log" 2>&1 &
        BROADWAY_PID=$!
        local j
        for ((j = 0; j < 100; j++)); do
            grep -q '^Listening on ' "$SANDBOX/broadway.log" 2>/dev/null && break
            kill -0 "$BROADWAY_PID" 2>/dev/null || break
            sleep 0.05
        done
        if grep -q '^Listening on ' "$SANDBOX/broadway.log" 2>/dev/null; then
            export GDK_BACKEND=broadway BROADWAY_DISPLAY=":$n" GSK_RENDERER=cairo GTK_A11Y=none
            return 0
        fi
        kill "$BROADWAY_PID" 2>/dev/null || true
        wait "$BROADWAY_PID" 2>/dev/null || true
        BROADWAY_PID=""
    done
    cat "$SANDBOX/broadway.log" >&2 || true
    return 1
}

stop_broadway() {
    [[ -n ${BROADWAY_PID:-} ]] || return 0
    kill "$BROADWAY_PID" 2>/dev/null || true
    wait "$BROADWAY_PID" 2>/dev/null || true
    BROADWAY_PID=""
    unset GDK_BACKEND BROADWAY_DISPLAY
}
