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

capture "$REPO/tools/bench-idle.sh" --live
status_is "--live without --subscribe is a usage error" 64 "$STATUS"
capture "$REPO/tools/bench-idle.sh" --nonsense
status_is "an unknown option is a usage error" 64 "$STATUS"
