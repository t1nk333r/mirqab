# shellcheck shell=bash disable=SC2034  # OUTPUT/STATUS/FIXTURES are read by the test files
# tests/lib.sh — assertions and sandbox helpers for tests/test-*.sh.
# Sourced by tests/run.sh into each test file's subshell.
#
# Adapted from haseen tests/lib.sh (MIT, (c) 2026 t1nk33r; NOTICE.md).
#
# Every test runs hermetically:
#   - fixture trees under tests/fixtures/ stand for `/` (architecture §5.3),
#     so nothing reads the live machine;
#   - HOME, XDG_* and XDG_RUNTIME_DIR point into a scratch directory under
#     ${TMPDIR:-/tmp}, never inside the checkout, removed when the file ends;
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

SCRATCH=""
_cleanup_scratch() {
    [[ -z $SCRATCH || ! -d $SCRATCH ]] || rm -rf -- "$SCRATCH"
}
trap _cleanup_scratch EXIT

# sandbox — a fresh scratch HOME, runtime dir and stub PATH; sets SANDBOX.
# Call it once per test file, before anything that could read the session.
sandbox() {
    _cleanup_scratch
    SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/mirqab-test.XXXXXX")"
    SANDBOX="$SCRATCH"
    mkdir -p "$SANDBOX/home" "$SANDBOX/stubs" "$SANDBOX/run"
    chmod 0700 "$SANDBOX/run"
    local c
    for c in "${STUBBED_CMDS[@]}"; do
        printf '#!/bin/sh\necho "STUB-CALLED: %s $*" >&2\nexit 97\n' "$c" >"$SANDBOX/stubs/$c"
        chmod +x "$SANDBOX/stubs/$c"
    done
    export HOME="$SANDBOX/home"
    export XDG_CONFIG_HOME="$HOME/.config" XDG_STATE_HOME="$HOME/.local/state"
    export XDG_DATA_HOME="$HOME/.local/share" XDG_CACHE_HOME="$HOME/.cache"
    export XDG_RUNTIME_DIR="$SANDBOX/run"
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
