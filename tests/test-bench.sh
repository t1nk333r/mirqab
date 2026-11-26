# tests/test-bench.sh — tools/bench-idle.sh measures and judges (architecture
# §9.1): a short idle run is within budget and prints its numbers; a live
# subscription is measured; the tray on is measured; --procs measures a
# scratch root of cloned processes; a budget it cannot meet fails with
# status 1; bad options are usage errors. The real 60 s runs are CI's and
# the owner's.
# mirqab:covers tools/bench-idle.sh tools/mkfixture.sh tests/fixtures/procs.tsv
# mirqab:covers tests/fixtures/root-desktop/** crates/mirqab-core/src/samplers/apps/**
# mirqab:covers-shared

sandbox
build_bins

# One clock tick (10 ms) over 2 s is 0.5 %, over the 0.3 % idle budget, so
# the idle checks run 4 s (one tick is 0.25 %) and judge the better of two
# runs: a single stray tick on a shared CI machine failed PR #12.
# $BIN is the debug build: as for the tray below, the 15 MiB budget is the
# release build's, gated by CI's `bench`; here a regression ceiling (debug
# RSS 15.2 MiB on CI and 15.4 locally on 2026-11-21, so 18).
capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 4 --runs 2 --max-rss 18
status_is "a short idle run is within budget" 0 "$STATUS"
like "it prints the CPU share" "$OUTPUT" "no subscriber: cpu "
like "and the verdict" "$OUTPUT" "within budget"

if command -v socat >/dev/null 2>&1; then
    # As the idle checks: two ticks over 2 s are exactly 1.000 %, which the
    # strict budget test fails, and a debug daemon on a live 1 s
    # subscription can spend them (main's full run at 1e6a6df, plan 024 C8).
    capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 4 --runs 2 --subscribe self --live --max-rss 18
    status_is "a live subscription is measured" 0 "$STATUS"
    like "against its own budget" "$OUTPUT" "< 1%"
else
    not_ok "socat is installed (bench-idle.sh --subscribe needs it)"
fi

# With the tray registered on a private bus (plan 004).
if command -v dbus-daemon >/dev/null 2>&1; then
    # $BIN is the debug build, about twice the release build's RSS: the
    # 15 MiB budget is the release build's, gated by CI's `bench --tray`
    # (review of PR #20, S5). Here a regression ceiling: the measured debug
    # RSS plus about 20 % (15.2 MiB on 2026-11-14, so 18).
    capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 4 --runs 2 --tray --max-rss 18
    status_is "the tray on, the window closed: within budget" 0 "$STATUS"
    like "it measured the idle row" "$OUTPUT" "within budget"
else
    not_ok "dbus-daemon is installed (bench-idle.sh --tray needs it)"
fi

capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 1 --max-rss 0.5 --runs 2
status_is "an unmet budget fails" 1 "$STATUS"
like "and says so" "$OUTPUT" "OVER BUDGET"
like "after every run" "$OUTPUT" "run 2/2"

# A scratch root of at least 400 processes (plan 006, part 4), with the
# apps walk subscribed: measured against its own budget.
if command -v socat >/dev/null 2>&1; then
    # Debug build: a regression ceiling of the measured RSS plus about
    # 20 % (17.1 MiB on 2026-11-14, so 21); CI gates the release build at
    # 1.5 % CPU and 15 MiB. The debug walk is slower: it measured 2.0 % on
    # CI twice (2026-11-14, 2026-11-21), so its ceiling is 3 %, judged on
    # the better of two runs as the idle checks are.
    capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 2 --runs 2 --subscribe apps --max-cpu 3 --max-rss 21 --procs 400
    status_is "apps at 400 processes is measured" 0 "$STATUS"
    # The count is the fixture's, read the way bench-idle.sh reads it, so a
    # row added to procs.tsv does not break this check (plan 024 C9).
    user=$(awk -F'\t' '!/^#/ && NF > 3 && ($1 == "both" || $1 == "t0") && $4 == 1000 { u++ } END { print u + 0 }' "$REPO/tests/fixtures/procs.tsv")
    like "on a cloned root it names" "$OUTPUT" "clones of $user user processes"
    like "against the debug ceiling" "$OUTPUT" "< 3%"
    n=$(sed -n 's/^procs: \([0-9]*\) processes.*/\1/p' <<<"$OUTPUT")
    ((${n:-0} >= 400)) && ok || not_ok "at least 400 processes (got ${n:-none})"
fi
capture "$REPO/tools/bench-idle.sh" --procs 0
status_is "--procs 0 is a usage error" 64 "$STATUS"

capture "$REPO/tools/bench-idle.sh" --seconds 0
status_is "--seconds 0 is a usage error" 64 "$STATUS"

# A daemon that never makes its socket cannot be measured, and the
# benchmark leaves nothing running after giving up. The stub writes its
# pid, which `exec` keeps: its command line becomes `sleep 300`, so looking
# for it by name found nothing whether it was alive or not (plan 024 L11).
mkdir -p "$SANDBOX/fakebin"
# shellcheck disable=SC2016  # $$ is the stub's, expanded when it runs
printf '#!/bin/sh\necho $$ >"%s"\nexec sleep 300\n' "$SANDBOX/stub.pid" >"$SANDBOX/fakebin/mirqab"
chmod +x "$SANDBOX/fakebin/mirqab"
capture "$REPO/tools/bench-idle.sh" --bin "$SANDBOX/fakebin" --seconds 1
status_is "a daemon that does not start cannot measure" 2 "$STATUS"
like "and says so" "$OUTPUT" "did not start"
stub_pid="$(cat "$SANDBOX/stub.pid" 2>/dev/null || true)"
if [[ -z $stub_pid ]]; then
    not_ok "the stub daemon ran"
elif kill -0 "$stub_pid" 2>/dev/null; then
    not_ok "the stub daemon is left running"
    kill "$stub_pid" 2>/dev/null || true
else
    ok
fi

capture "$REPO/tools/bench-idle.sh" --live
status_is "--live without --subscribe is a usage error" 64 "$STATUS"
capture "$REPO/tools/bench-idle.sh" --nonsense
status_is "an unknown option is a usage error" 64 "$STATUS"

# A refused subscription cannot measure: exit 2, not an idle pass.
if command -v socat >/dev/null 2>&1; then
    capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 1 --subscribe nonesuch
    status_is "a refused subscription cannot measure" 2 "$STATUS"
    like "and says why" "$OUTPUT" "refused"
fi

# children PID — the daemon and socat the benchmark PID started.
children() { pgrep -P "$1" -f 'mirqab daemon|socat' || true; }

# A daemon killed during the measurement is a failure (review of PR #6: it
# passed as 0.0 MiB within budget).
"$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 8 >"$SANDBOX/killed.out" 2>&1 &
bench=$!
sleep 6.5
kids="$(children "$bench")"
[[ -n $kids ]] && ok || not_ok "the benchmark runs a daemon"
kill -KILL $kids 2>/dev/null || true
wait "$bench" && st=0 || st=$?
is "a daemon dying mid-run exits 2" 2 "$st"
like "and says so" "$(<"$SANDBOX/killed.out")" "exited during the measurement"
unlike "it never reports a budget" "$(<"$SANDBOX/killed.out")" "within budget"

# Stopped by a signal, it leaves no daemon and no socat behind, at once:
# the trap waited for the measurement's sleep to end (plan 024 L10).
if command -v socat >/dev/null 2>&1; then
    "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 30 --subscribe self >/dev/null 2>&1 &
    bench=$!
    sleep 6.5
    kids="$(children "$bench")"
    is "it holds a daemon and a socat" 2 "$(wc -w <<<"$kids" | tr -d ' ')"
    asked=$SECONDS
    kill -TERM "$bench"
    wait "$bench" 2>/dev/null && st=0 || st=$?
    took=$((SECONDS - asked))
    is "SIGTERM exits 143" 143 "$st"
    ((took < 5)) && ok || not_ok "SIGTERM stops it at once (took ${took}s of the 30 s measurement)"
    left=0
    for k in $kids; do kill -0 "$k" 2>/dev/null && left=$((left + 1)); done
    is "SIGTERM leaves nothing running" 0 "$left"
fi

# Without --bin it measures the binary it just built, wherever
# CARGO_TARGET_DIR puts it, never a stale one under ./target (audit of the
# gates). A stub cargo "builds" a binary that says so and exits at start.
stub cargo 'mkdir -p "$CARGO_TARGET_DIR/release"
printf "#!/bin/sh\necho FRESH-BUILD >&2\nexit 3\n" >"$CARGO_TARGET_DIR/release/mirqab"
chmod +x "$CARGO_TARGET_DIR/release/mirqab"'
capture env CARGO_TARGET_DIR="$SANDBOX/target" "$REPO/tools/bench-idle.sh" --seconds 1
status_is "the fresh build exits at start: cannot measure" 2 "$STATUS"
like "it ran the binary cargo built in CARGO_TARGET_DIR" "$OUTPUT" "FRESH-BUILD"
