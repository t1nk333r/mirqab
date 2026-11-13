# tests/test-cli.sh — the `mirqab` command line: exit codes, help, the
# generated docs and completions, output into a closed pipe, and
# `status --json` against a daemon on a fixture root. The window commands
# reaching an interface are tested in crates/mirqab/tests/cli.rs.
# mirqab:covers crates/mirqab/src/{main,cli,daemon,out}.rs crates/mirqab-proto/src/strings.rs docs/cli.md
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
for c in daemon status open toggle close history help; do
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
is "open --dry-run says what it would do" '{"event":"show","mode":null,"tab":"system"}' "$(jq -c '.wouldDo[0]' <<<"$OUTPUT")"
capture "$BIN/mirqab" open system --window --dry-run
is "only --window names a mode" '"window"' "$(jq -c '.wouldDo[0].mode' <<<"$OUTPUT")"
capture "$BIN/mirqab" open
status_is "open without an interface exits 1" 1 "$STATUS"
like "and says why" "$OUTPUT" "could not start mirqab-ui.service"
like "through the stubbed systemctl, never the real one" "$OUTPUT" "STUB-CALLED: systemctl --user start mirqab-ui.service"

# History: a daemon on a fixture root keeps its file beside its socket, never
# in XDG_STATE_HOME (phase E); no minute has passed.
like "status reports history on" "$("$BIN/mirqab" status --json | jq -c .history)" '"enabled":true'
ok_file="$(dirname "$MIRQAB_SOCKET")/history.db"
[[ -f $ok_file ]] && ok || not_ok "history.db created beside the socket"
[[ ! -e $XDG_STATE_HOME/mirqab/history.db ]] && ok || not_ok "and not under XDG_STATE_HOME"
capture "$BIN/mirqab" history --list
status_is "history --list exits 0" 0 "$STATUS"
like "history --list names cpu.total" "$OUTPUT" "cpu.total"

# The System tab's commands (plan 002).
capture "$BIN/mirqab" load --json
status_is "load --json exits 0" 0 "$STATUS"
is "load --json has the cores" 16 "$(jq '.cpu.cores | length' <<<"$OUTPUT")"
is "and the thermal word" '"cool"' "$(jq -c '.thermal.state' <<<"$OUTPUT")"
capture "$BIN/mirqab" load
like "load prints memory" "$OUTPUT" "memory    17 GB used of 67 GB"
like "and the thermal word" "$OUTPUT" "thermal   Cool"
capture "$BIN/mirqab" sensors --json
is "sensors --json lists the temperatures" 7 "$(jq '.temps | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" sensors
like "sensors names the CPU sensor" "$OUTPUT" "Tctl"
like "and the GPUs" "$OUTPUT" "AMD GPU (card1)"
capture "$BIN/mirqab" load --dry-run
status_is "load takes no --dry-run" 64 "$STATUS"
capture "$BIN/mirqab" history cpu.total --range 24h --json
status_is "history --json exits 0" 0 "$STATUS"
is "a fresh history has no points" "0" "$(jq '.series["cpu.total"].points | length' <<<"$OUTPUT")"
is "24h buckets are four minutes" "240" "$(jq .bucketSec <<<"$OUTPUT")"
capture "$BIN/mirqab" history cpu.total --range 2y
status_is "an unknown range is refused" 1 "$STATUS"
capture "$BIN/mirqab" history
status_is "history needs a series" 64 "$STATUS"
# --json is one line, and nothing piped carries an escape code.
capture "$BIN/mirqab" status --json
is "status --json is one line" 1 "$(wc -l <<<"$OUTPUT" | tr -d ' ')"
capture "$BIN/mirqab" load
unlike "no escape codes when piped" "$OUTPUT" $'\e['
stop_daemon

# Help, docs and completions come from one table (plan 005).
capture "$BIN/mirqab" help --markdown
is "docs/cli.md is the generated markdown" "$(<"$REPO/docs/cli.md")" "$OUTPUT"
for shell in bash zsh fish; do
    capture "$BIN/mirqab" completions "$shell"
    status_is "completions $shell" 0 "$STATUS"
    like "the $shell script names bar" "$OUTPUT" "bar"
done
bash -n <("$BIN/mirqab" completions bash) && ok || not_ok "the bash script parses"
# The bash script completes the command after a global flag's argument,
# each flag's values from its usage line, and paths after --socket.
"$BIN/mirqab" completions bash >"$SANDBOX/c.bash"
complete_bash() {
    # shellcheck disable=SC1091
    bash -c 'source "$1"; shift; COMP_WORDS=("$@"); COMP_CWORD=$(($# - 1)); _mirqab; printf "%s\n" "${COMPREPLY[@]}"' \
        _ "$SANDBOX/c.bash" "$@" | sort | tr '\n' ' '
}
like "commands after --socket PATH" "$(complete_bash mirqab --socket /x "")" "load "
like "completions offers the shells" "$(complete_bash mirqab completions "")" "bash fish"
like "help offers the commands" "$(complete_bash mirqab help "")" "--markdown"
like "help offers sensors" "$(complete_bash mirqab help "")" "sensors"
is "bar --format offers the formats" "$(complete_bash mirqab bar --format "")" "json plain waybar "
is "history --range offers the ranges" "$(complete_bash mirqab history --range "")" "1h 24h 30d 7d "
like "bar offers the items" "$(complete_bash mirqab bar "")" "combo"
mkdir -p "$SANDBOX/sockdir"
is "--socket completes a path" "$(complete_bash mirqab --socket "$SANDBOX/sockd")" "$SANDBOX/sockdir "
zsh -n <("$BIN/mirqab" completions zsh) && ok || not_ok "the zsh script parses"
fish -n <("$BIN/mirqab" completions fish) && ok || not_ok "the fish script parses"

# Every command in the table reaches its own parser: an unknown flag is
# that command's usage error, never "unknown command" (review of PR #12).
while IFS= read -r name; do
    capture "$BIN/mirqab" "$name" --no-such-flag
    status_is "mirqab $name --no-such-flag is a usage error" 64 "$STATUS"
    unlike "mirqab $name is dispatched" "$OUTPUT" "unknown command"
done < <("$BIN/mirqab" help --markdown | sed -n 's/^## \([a-z]*\)$/\1/p')
capture "$BIN/mirqab" completions tcsh
status_is "an unknown shell is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" bar --help
like "a command's help is its table entry" "$OUTPUT" "usage: mirqab bar ITEM"

# A reader that went away ends the command quietly, never with a panic
# (the release profile turns one into an abort). perl hands mirqab a pipe
# whose read end is already closed, so every write fails with EPIPE; a
# closed stdout would not do, since Rust treats EBADF as success.
broken_pipe() { perl -e 'pipe(my $r, my $w) or die; close $r; open(STDOUT, ">&", $w) or die; exec @ARGV' "$@"; }
for cmd in "bar --list" "help --markdown" "completions bash"; do
    code=0
    # shellcheck disable=SC2086
    broken_pipe "$BIN/mirqab" $cmd 2>"$SANDBOX/pipe.err" || code=$?
    is "mirqab $cmd into a broken pipe exits 0" 0 "$code"
    is "mirqab $cmd prints no panic" "" "$(<"$SANDBOX/pipe.err")"
done
# A failed write that is not a broken pipe (a full disk) is an error, so a
# truncated file never looks like success.
code=0
"$BIN/mirqab" help --markdown >/dev/full 2>"$SANDBOX/full.err" || code=$?
is "a full disk exits 1" 1 "$code"
like "a full disk says so" "$(<"$SANDBOX/full.err")" "could not write the output"
# With stderr gone too, a usage error still exits 64, not with a panic.
code=0
perl -e 'pipe(my $r, my $w) or die; close $r; open(STDERR, ">&", $w) or die; exec @ARGV' "$BIN/mirqab" nonsense >/dev/null || code=$?
is "a usage error into a broken stderr exits 64" 64 "$code"
