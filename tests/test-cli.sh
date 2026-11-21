# tests/test-cli.sh — the `mirqab` command line: exit codes, help, the
# generated docs and completions, output into a closed pipe, and
# `status --json` against a daemon on a fixture root. The window commands
# reaching an interface are tested in crates/mirqab/tests/cli.rs.
# mirqab:covers crates/mirqab/src/{main,cli,daemon,out,apps,quit,ports}.rs crates/mirqab-proto/src/strings.rs docs/cli.md
# mirqab:covers tests/fixtures/root-desktop/** tests/fixtures/bin/pacman
# mirqab:covers crates/mirqab-core/src/samplers/ports/** crates/mirqab-core/src/samplers/apps/**
# mirqab:covers-shared

sandbox
build_bins
# The process inspector asks pacman who owns an executable (plan 006): the
# fixture's stub answers for the fixture's paths.
cp "$FIXTURES/bin/pacman" "$SANDBOX/stubs/pacman"

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
for c in daemon status open toggle close history ports port help; do
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

# Quit and Force quit (plan 006, part 2): a daemon on a fixture root gets
# the recording sender, which writes each signal and command to
# MIRQAB_ACT_LOG and sends or runs nothing. The caller is the fixture's
# python3 in a terminal: the CLI's real pid has no proc entry in the tree.
export MIRQAB_ACT_LOG="$SANDBOX/act.log" MIRQAB_CALLER_PID=3060
: >"$MIRQAB_ACT_LOG"
start_daemon root-desktop
capture "$BIN/mirqab" status --json
status_is "status --json exits 0" 0 "$STATUS"
is "status --json parses" "0.1.0" "$(jq -r .version <<<"$OUTPUT")"
is "status names the socket" "$MIRQAB_SOCKET" "$(jq -r .socket <<<"$OUTPUT")"
is "nobody subscribes yet" "0" "$(jq '[.streams[].subscribers] | add' <<<"$OUTPUT")"
is "status lists the ports stream, unsubscribed" "0" "$(jq '.streams.ports.subscribers' <<<"$OUTPUT")"
is "the daemon's memory comes from the fixture" "3145728" "$(jq .daemon.rssBytes <<<"$OUTPUT")"
capture "$BIN/mirqab" status
status_is "status exits 0" 0 "$STATUS"
like "status prints the version" "$OUTPUT" "mirqab 0.1.0"
like "status prints the subscriber count" "$OUTPUT" "subscribers 0"
# Plan 006, part 4: no focus source on a fixture root (no Hyprland in its
# runtime dir, and the environment is never read there).
is "status --json has a null focus" "null" "$("$BIN/mirqab" status --json | jq -c .focus)"
like "status prints the focus line" "$OUTPUT" "focus       none"
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

# The Apps tab's commands (plan 006).
capture "$BIN/mirqab" apps --json
status_is "apps --json exits 0" 0 "$STATUS"
is "apps --json sorts by cpu" '"cpu"' "$(jq -c .sort <<<"$OUTPUT")"
is "the System group comes last" '"system"' "$(jq -c '.groups[-1].id' <<<"$OUTPUT")"
is "processes only with --all" 0 "$(jq '[.groups[] | select(.processes)] | length' <<<"$OUTPUT")"
groups="$(jq '.groups | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" apps --all --json
is "--all expands every group" "$groups" "$(jq '[.groups[] | select(.processes)] | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" apps --sort name
status_is "apps exits 0" 0 "$STATUS"
like "apps names the browser from its desktop entry" "$OUTPUT" "Example Browser"
like "and a terminal's job by itself" "$OUTPUT" "vite"
like "a long name is cut to its column" "$OUTPUT" "postgres (container c0ffee0…"
capture "$BIN/mirqab" apps --sort size
status_is "an unknown --sort is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" what 2001
status_is "what exits 0" 0 "$STATUS"
is "what heads with the verdict" "Packaged by extra/example-browser" "$(head -n1 <<<"$OUTPUT")"
like "and names the parents" "$OUTPUT" "bash (2000), Hyprland (1050)"
like "and the GPU memory" "$OUTPUT" "671 MB VRAM"
capture "$BIN/mirqab" what 6201 --json
is "what --json gives the verdict's code" '"home"' "$(jq -c .verdict.code <<<"$OUTPUT")"
capture "$BIN/mirqab" what 410
like "another user's process is limited" "$OUTPUT" "only its name, CPU and memory"
unlike "and shows no executable" "$OUTPUT" "executable"
capture "$BIN/mirqab" what 99999
status_is "a missing process exits 1" 1 "$STATUS"
like "and says so" "$OUTPUT" "no process 99999"
capture "$BIN/mirqab" what
status_is "what needs a pid" 64 "$STATUS"
capture "$BIN/mirqab" what abc
status_is "what needs a number" 64 "$STATUS"
like "and says so" "$OUTPUT" "not a process id: abc"

# The Ports tab's commands (plan 007, part 1a).
capture "$BIN/mirqab" ports --json
status_is "ports --json exits 0" 0 "$STATUS"
is "ports --json lists the 11 TCP listeners" 11 "$(jq '.ports | length' <<<"$OUTPUT")"
is "and no UDP without --udp" 0 "$(jq '[.ports[] | select(.proto == "udp")] | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" ports --udp --json
is "ports --udp --json adds the bound UDP sockets" 13 "$(jq '.ports | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" ports
status_is "ports exits 0" 0 "$STATUS"
like "ports names the port" "$OUTPUT" ":3000"
like "and its process" "$OUTPUT" "node (6410)"
like "and its reach" "$OUTPUT" "local"
like "and root's port by its owner" "$OUTPUT" ":22     tcp    0.0.0.0          all      root"
like "and warns of the exposed server" "$OUTPUT" "exposed: :8000/tcp python3 (6600) listens on 0.0.0.0"
unlike "but not of root's" "$OUTPUT" "exposed: :22/"
capture "$BIN/mirqab" port 3000
status_is "port 3000 exits 0" 0 "$STATUS"
like "port 3000 says only this machine reaches it" "$OUTPUT" "Reach      Only this machine can connect."
like "and what it serves" "$OUTPUT" "Label      next dev"
capture "$BIN/mirqab" port 8000
like "port 8000 says every interface reaches it" "$OUTPUT" "Listening on every interface, so other devices on your network can connect"
capture "$BIN/mirqab" port 22
like "root's port names no process" "$OUTPUT" "owned by root; mirqab cannot see which process"
capture "$BIN/mirqab" port 41234 --proto udp --json
is "port --proto udp finds the UDP socket" '[6610]' "$(jq -c '[.[].pid]' <<<"$OUTPUT")"
capture "$BIN/mirqab" port 1
status_is "port 1 exits 1" 1 "$STATUS"
like "and says nothing listens" "$OUTPUT" "nothing listens on port 1/tcp"
capture "$BIN/mirqab" port abc
status_is "port abc is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" port 70000
status_is "a port past 65535 is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" port 3000 --proto sctp
status_is "an unknown --proto is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" status --json
is "no ports subscriber lingers after the commands" "0" "$(jq '.streams.ports.subscribers' <<<"$OUTPUT")"

# perl hands mirqab a pipe whose read end is already closed, so every
# write fails with EPIPE; a closed stdout would not do, since Rust treats
# EBADF as success.
broken_pipe() { perl -e 'pipe(my $r, my $w) or die; close $r; open(STDOUT, ">&", $w) or die; exec @ARGV' "$@"; }

# mirqab quit (plan 006, part 2), against the recording sender.
act_log() { cat "$MIRQAB_ACT_LOG"; }
capture "$BIN/mirqab" quit 2001 --dry-run
status_is "quit --dry-run exits 0" 0 "$STATUS"
like "quit --dry-run lists the process" "$OUTPUT" "2001  browser"
is "a dry run sends nothing" "" "$(act_log)"
capture "$BIN/mirqab" quit 2001 --dry-run --json
is "quit --dry-run --json is the wouldDo" '[{"name":"browser","pid":2001,"signal":"TERM","start":5010}]' \
    "$(jq -c .wouldDo <<<"$OUTPUT")"
capture "$BIN/mirqab" quit 2001 </dev/null
status_is "quit with no terminal and no -y exits 3" 3 "$STATUS"
like "and says why" "$OUTPUT" "no terminal"
is "and sends nothing" "" "$(act_log)"
capture "$BIN/mirqab" quit 2001 -y
status_is "quit -y exits 0" 0 "$STATUS"
like "and says what it sent" "$OUTPUT" "sent TERM to 2001 browser"
is "the recording has the signal and the start time" "TERM 2001 5010" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
# With --json, stdout holds the answer alone; the list goes to stderr.
code=0
out="$("$BIN/mirqab" quit 2001 -y --json 2>/dev/null)" || code=$?
status_is "quit -y --json exits 0" 0 "$code"
jq -e '.done[0].pid == 2001 and .recorded == true' <<<"$out" >/dev/null && ok || not_ok "quit -y --json prints only the result on stdout" "$out"
is "and the recording has the signal" "TERM 2001 5010" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit 7100 -y --force
status_is "quit --force exits 0" 0 "$STATUS"
is "--force sends KILL" "KILL 7100 34000" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit 410 -y
status_is "a root process is refused" 1 "$STATUS"
like "with the reason" "$OUTPUT" "belongs to another user"
capture "$BIN/mirqab" quit 1 -y
status_is "pid 1 is refused" 1 "$STATUS"
like "with the reason" "$OUTPUT" "never signalled"
capture "$BIN/mirqab" quit 3000 -y
status_is "the caller's own terminal is refused" 1 "$STATUS"
like "with the reason" "$OUTPUT" "the program asking"
is "nothing refused was sent" "" "$(act_log)"
capture "$BIN/mirqab" quit "example BROWSER" -y
status_is "quit by name exits 0" 0 "$STATUS"
is "a name quits the app, its main process first" "TERM 2001 5010
TERM 2000 5000
TERM 2010 5100
TERM 2011 5200
TERM 2012 5300" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit "No Such App" -y
status_is "an unknown name exits 1" 1 "$STATUS"
like "and says so" "$OUTPUT" "no app is named No Such App"
capture "$BIN/mirqab" quit python3 -y
status_is "a name two apps share exits 1" 1 "$STATUS"
like "and lists both" "$OUTPUT" "session:3050"
like "with their pids" "$OUTPUT" "scope:app-Hyprland-script-1a2b3c4d  pids 7500"
# Either is then quit by its id with --group, every process of it, not
# by a pid that would signal that process alone (audit 2026-11-21, L47).
like "and says to choose one with --group" "$OUTPUT" "with --group ID"
unlike "not by a pid of it" "$OUTPUT" "by a pid"
capture "$BIN/mirqab" quit --group scope:app-Hyprland-script-1a2b3c4d --dry-run
status_is "quit --group --dry-run exits 0" 0 "$STATUS"
like "and plans that group" "$OUTPUT" "7500  python3"
is "a dry run by group sends nothing" "" "$(act_log)"
capture "$BIN/mirqab" quit --group scope:app-Hyprland-script-1a2b3c4d -y
status_is "quit --group -y exits 0" 0 "$STATUS"
is "--group quits the group it names" "TERM 7500 36000" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit --group "scope:app-Hyprland-browser.sh-5650fedc" -y
status_is "quit --group on a group of five exits 0" 0 "$STATUS"
is "--group quits every process of it, its main process first" "TERM 2001 5010
TERM 2000 5000
TERM 2010 5100
TERM 2011 5200
TERM 2012 5300" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit --group scope:app-none-0 -y
status_is "an unknown group exits 1" 1 "$STATUS"
like "and says so" "$OUTPUT" "no app group scope:app-none-0"
capture "$BIN/mirqab" quit --group
status_is "--group needs an id" 64 "$STATUS"
capture "$BIN/mirqab" quit 2001 --group scope:app-Hyprland-script-1a2b3c4d
status_is "--group and a target together are a usage error" 64 "$STATUS"
is "nothing was sent for them" "" "$(act_log)"
capture "$BIN/mirqab" quit Hyprland --dry-run
status_is "a group with a refused member exits 0" 0 "$STATUS"
like "and lists that member as left alone" "$OUTPUT" "left alone    1050  Hyprland: pid 1050 is the session's compositor"
capture "$BIN/mirqab" quit 1300 -y
status_is "quitting a user service exits 0" 0 "$STATUS"
is "a user service is stopped with systemctl" \
    "RUN systemctl --user stop --no-block -- app-org.example.Sync@autostart.service" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
# Part 3: the units a stop takes with it (the fixture's answer for
# pipewire.service), and the sound server's warning (owner, 2026-11-14).
capture "$BIN/mirqab" quit 1210 --dry-run
status_is "quit on a service with a dependent exits 0" 0 "$STATUS"
like "and names the unit it also stops" "$OUTPUT" "also stops wireplumber.service (1 process)"
unlike "but not an inactive one" "$OUTPUT" "pipewire-pulse"
like "and warns about the sound" "$OUTPUT" "warning: pipewire.service is part of your sound"
capture "$BIN/mirqab" quit 1210 --dry-run --json
is "--json carries alsoStops" '[{"pids":[1211],"unit":"wireplumber.service"}]' \
    "$(jq -c '.wouldDo[0].alsoStops' <<<"$OUTPUT")"
is "and the warnings" "2" "$(jq '.warnings | length' <<<"$OUTPUT")"
capture "$BIN/mirqab" quit 1210 -y
status_is "quit -y on it exits 0" 0 "$STATUS"
is "only the unit asked for is stopped by name" \
    "RUN systemctl --user stop --no-block -- pipewire.service" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
# A reader that went away does not end a command that changes something
# before it has done what it was told, and its exit code is its own
# (audit 2026-11-21, L40): -y still quits and exits 0 for what it did; no
# -y and no terminal still exits 3 and sends nothing.
code=0
broken_pipe "$BIN/mirqab" quit 2001 -y 2>"$SANDBOX/pipe.err" || code=$?
is "quit -y into a broken pipe still quits" "TERM 2001 5010" "$(act_log)"
is "and exits 0 for what it did" 0 "$code"
is "and prints no panic" "" "$(<"$SANDBOX/pipe.err")"
: >"$MIRQAB_ACT_LOG"
code=0
broken_pipe "$BIN/mirqab" quit 7100 -y --force --json 2>/dev/null || code=$?
is "quit -y --json into a broken pipe still quits" "KILL 7100 34000" "$(act_log)"
is "and exits 0" 0 "$code"
: >"$MIRQAB_ACT_LOG"
code=0
broken_pipe "$BIN/mirqab" quit 2001 </dev/null 2>"$SANDBOX/pipe.err" || code=$?
is "quit into a broken pipe with no -y exits 3" 3 "$code"
like "and says why" "$(<"$SANDBOX/pipe.err")" "no terminal"
is "and sends nothing" "" "$(act_log)"
capture "$BIN/mirqab" quit 0
status_is "pid 0 is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" quit
status_is "quit needs a target" 64 "$STATUS"
capture "$BIN/mirqab" quit --help
like "quit --help is its table entry" "$OUTPUT" "usage: mirqab quit PID|NAME"
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

# A name too long for `mirqab apps` is shown cut, and quits by that shown
# form: a prefix that must name exactly one app (audit 2026-11-21, L46).
# A copy of root-desktop whose apps have long names, two sharing the part
# that is shown.
LONG="$SANDBOX/root-long"
cp -r "$FIXTURES/root-desktop" "$LONG"
sed -i 's/^Name=Example Browser$/Name=Example Browser Developer Edition X/' \
    "$LONG/usr/share/applications/example-browser.desktop"
sed -i 's/^Name=Example Sync$/Name=Example Notes And Sync Tool One/' \
    "$LONG/usr/share/applications/org.example.Sync.desktop"
sed -i 's/^Name=Example Notes$/Name=Example Notes And Sync Tool Two/' \
    "$LONG/var/lib/flatpak/exports/share/applications/org.example.Notes.desktop"
rm -f "$SANDBOX/daemon.out"
: >"$MIRQAB_ACT_LOG"
start_daemon "$LONG"
capture "$BIN/mirqab" apps --sort name
like "apps cuts a long name" "$OUTPUT" "Example Browser Developer E… "
capture "$BIN/mirqab" quit "Example Browser Developer E…" --dry-run
status_is "quit by a shown name exits 0" 0 "$STATUS"
like "and plans that app" "$OUTPUT" "2001  browser"
capture "$BIN/mirqab" quit "example browser developer e…" -y
status_is "quit -y by a shown name exits 0" 0 "$STATUS"
is "a shown name quits the app, its main process first" "TERM 2001 5010
TERM 2000 5000
TERM 2010 5100
TERM 2011 5200
TERM 2012 5300" "$(act_log)"
: >"$MIRQAB_ACT_LOG"
capture "$BIN/mirqab" quit "Example Notes And Sync Tool…" -y
status_is "a shown name two apps share exits 1" 1 "$STATUS"
like "and lists one" "$OUTPUT" "flatpak:org.example.Notes"
like "and the other" "$OUTPUT" "service:app-org.example.Sync@autostart"
capture "$BIN/mirqab" quit "Example Browser…" -y
status_is "a cut shorter than the shown one names no app" 1 "$STATUS"
like "and says so" "$OUTPUT" "no app is named"
is "nothing was sent for either" "" "$(act_log)"
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
is "apps --sort offers the orders" "$(complete_bash mirqab apps --sort "")" "cpu mem name "
like "the commands include ports and port" "$(complete_bash mirqab "")" "port ports "
is "port --proto offers the protocols" "$(complete_bash mirqab port --proto "")" "tcp udp "
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
# (the release profile turns one into an abort); broken_pipe is above.
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
