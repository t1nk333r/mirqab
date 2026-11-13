# tests/test-bench.sh — tools/bench-idle.sh measures and judges (architecture
# §9.1): a short idle run is within budget and prints its numbers; a live
# subscription is measured; a budget it cannot meet fails with status 1; bad
# options are usage errors. The real 60 s runs are CI's and the owner's.
# mirqab:covers tools/bench-idle.sh

sandbox
build_bins

capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 2
status_is "a short idle run is within budget" 0 "$STATUS"
like "it prints the CPU share" "$OUTPUT" "no subscriber: cpu "
like "and the verdict" "$OUTPUT" "within budget"

if command -v socat >/dev/null 2>&1; then
    capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 2 --subscribe self --live
    status_is "a live subscription is measured" 0 "$STATUS"
    like "against its own budget" "$OUTPUT" "< 1%"
else
    not_ok "socat is installed (bench-idle.sh --subscribe needs it)"
fi

capture "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 1 --max-rss 0.5 --runs 2
status_is "an unmet budget fails" 1 "$STATUS"
like "and says so" "$OUTPUT" "OVER BUDGET"
like "after every run" "$OUTPUT" "run 2/2"

capture "$REPO/tools/bench-idle.sh" --seconds 0
status_is "--seconds 0 is a usage error" 64 "$STATUS"

# A daemon that never makes its socket cannot be measured, and the
# benchmark leaves nothing running after giving up.
mkdir -p "$SANDBOX/fakebin"
printf '#!/bin/sh\nexec sleep 300\n' >"$SANDBOX/fakebin/mirqab"
chmod +x "$SANDBOX/fakebin/mirqab"
capture "$REPO/tools/bench-idle.sh" --bin "$SANDBOX/fakebin" --seconds 1
status_is "a daemon that does not start cannot measure" 2 "$STATUS"
like "and says so" "$OUTPUT" "did not start"
pgrep -f "$SANDBOX/fakebin/mirqab" >/dev/null && not_ok "the stub daemon is left running" || ok

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

# Stopped by a signal, it leaves no daemon and no socat behind.
if command -v socat >/dev/null 2>&1; then
    "$REPO/tools/bench-idle.sh" --bin "$BIN" --seconds 30 --subscribe self >/dev/null 2>&1 &
    bench=$!
    sleep 6.5
    kids="$(children "$bench")"
    is "it holds a daemon and a socat" 2 "$(wc -w <<<"$kids" | tr -d ' ')"
    kill -TERM "$bench"
    wait "$bench" 2>/dev/null || true
    left=0
    for k in $kids; do kill -0 "$k" 2>/dev/null && left=$((left + 1)); done
    is "SIGTERM leaves nothing running" 0 "$left"
fi
