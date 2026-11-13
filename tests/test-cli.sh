# tests/test-cli.sh — the `mirqab` command line: exit codes, help, and
# `status --json` against a daemon on a fixture root. The window commands
# reaching an interface are tested in crates/mirqab/tests/cli.rs.
# mirqab:covers crates/mirqab/src/{main,cli,daemon}.rs crates/mirqab-proto/src/strings.rs
# mirqab:covers tests/fixtures/root-desktop/**

sandbox
build_bins

# No daemon: exit 2 and a hint.
export MIRQAB_SOCKET="$XDG_RUNTIME_DIR/mirqab/daemon.sock"
capture "$BIN/mirqab" status
status_is "no daemon exits 2" 2 "$STATUS"
like "no daemon names the socket" "$OUTPUT" "$MIRQAB_SOCKET"
like "no daemon says how to start one" "$OUTPUT" "systemctl --user start mirqab.socket"

# Usage errors exit 64; help exits 0, for every command.
capture "$BIN/mirqab"
status_is "no command is a usage error" 64 "$STATUS"
like "no command prints the usage" "$OUTPUT" "usage: mirqab"
capture "$BIN/mirqab" frobnicate
status_is "an unknown command exits 64" 64 "$STATUS"
capture "$BIN/mirqab" status --bogus
status_is "an unknown option exits 64" 64 "$STATUS"
capture "$BIN/mirqab" --version
status_is "--version exits 0" 0 "$STATUS"
like "--version prints the version" "$OUTPUT" "mirqab 0."
for c in daemon status open toggle close help; do
    capture "$BIN/mirqab" "$c" --help
    status_is "$c --help exits 0" 0 "$STATUS"
    like "$c --help prints its usage" "$OUTPUT" "usage: mirqab $c"
    capture "$BIN/mirqab" help "$c"
    status_is "help $c exits 0" 0 "$STATUS"
done

# The daemon refuses a runtime dir that is missing.
capture env -u XDG_RUNTIME_DIR "$BIN/mirqab" daemon
status_is "the daemon needs XDG_RUNTIME_DIR" 1 "$STATUS"
like "and says so" "$OUTPUT" "XDG_RUNTIME_DIR is not set"

start_daemon root-desktop
capture "$BIN/mirqab" status --json
status_is "status --json exits 0" 0 "$STATUS"
is "status --json parses" "0.1.0" "$(jq -r .version <<<"$OUTPUT")"
is "status names the socket" "$MIRQAB_SOCKET" "$(jq -r .socket <<<"$OUTPUT")"
is "nobody subscribes yet" "0" "$(jq '[.streams[].subscribers] | add' <<<"$OUTPUT")"
is "the daemon's memory comes from the fixture" "3145728" "$(jq .daemon.rssBytes <<<"$OUTPUT")"
capture "$BIN/mirqab" status
status_is "status exits 0" 0 "$STATUS"
like "status prints the version" "$OUTPUT" "mirqab 0.1.0"
like "status prints the subscriber count" "$OUTPUT" "subscribers 0"
capture "$BIN/mirqab" open system --dry-run
status_is "open --dry-run exits 0 without an interface" 0 "$STATUS"
is "open --dry-run says what it would do" '{"event":"show","mode":"popover","tab":"system"}' "$(jq -c '.wouldDo[0]' <<<"$OUTPUT")"
capture "$BIN/mirqab" open
status_is "open without an interface exits 1" 1 "$STATUS"
like "and says why" "$OUTPUT" "no interface is running"
stop_daemon
