# tests/test-bar.sh — `mirqab bar` (plan 004, Tests): each item's one-shot
# output in the three formats against root-desktop; `--watch` is silent
# while the reading is unchanged and prints when counters swapped under it
# change it; an interval under 2 s is raised; a closed pipe ends it;
# `--list` names the items; usage errors exit 64.
# mirqab:covers crates/mirqab/src/bar/** crates/mirqab/src/{main,cli}.rs
# mirqab:covers crates/mirqab-proto/src/strings.rs crates/mirqab-core/src/sys.rs
# mirqab:covers tests/fixtures/root-desktop/** tests/fixtures/root-desktop.t1/**

sandbox
build_bins

capture "$BIN/mirqab" bar --list
status_is "--list exits 0" 0 "$STATUS"
is "--list names the items" "cpu mem temp gpu load combo" "$(awk '{print $1}' <<<"$OUTPUT" | xargs)"
capture "$BIN/mirqab" bar
status_is "no item is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" bar nope
status_is "an unknown item is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" bar cpu --format xml
status_is "an unknown format is a usage error" 64 "$STATUS"

# A copy of root-desktop, so the watch case can swap counters under it.
ROOT="$SANDBOX/root"
cp -r "$FIXTURES/root-desktop" "$ROOT"
start_daemon "$ROOT"

for item in cpu mem temp gpu load combo; do
    capture "$BIN/mirqab" bar "$item"
    status_is "bar $item exits 0" 0 "$STATUS"
    is "bar $item is Waybar JSON naming the item" "$item" "$(jq -r .alt <<<"$OUTPUT" 2>/dev/null)"
    capture "$BIN/mirqab" bar "$item" --format json
    is "bar $item --format json has a class" true "$(jq -r 'has("class") and has("percentage")' <<<"$OUTPUT" 2>/dev/null)"
done
capture "$BIN/mirqab" bar mem --format plain
is "plain memory" "25%" "$OUTPUT"
capture "$BIN/mirqab" bar temp --format plain
is "plain temperature" "52°" "$OUTPUT"
capture "$BIN/mirqab" bar combo --format plain
like "combo joins memory and temperature" "$OUTPUT" "25% 52°"
capture "$BIN/mirqab" bar temp
is "temperature class from the thermal word" cool "$(jq -r .class <<<"$OUTPUT")"
unlike "no markup in the text" "$(jq -r .text <<<"$OUTPUT")" "<"

# --watch: one line for an unchanged reading; a second once the CPU
# counters move; an interval of 1 s is raised to the 2 s floor.
"$BIN/mirqab" bar cpu --watch --interval 1 --format plain >"$SANDBOX/watch.out" &
watcher=$!
sleep 1
is "the interval is raised to 2 s" 2000 "$("$BIN/mirqab" status --json | jq -r '.streams.system.intervalMs')"
sleep 4
is "one line while unchanged" 1 "$(wc -l <"$SANDBOX/watch.out" | tr -d ' ')"
cp "$FIXTURES/root-desktop.t1/proc/stat" "$ROOT/proc/stat"
sleep 5
kill "$watcher" 2>/dev/null || true
wait "$watcher" 2>/dev/null || true
# Then the copy's counters stand still again, so a third line may say
# there is no rate; real counters always move.
is "the first line has no rate yet" "—" "$(sed -n 1p "$SANDBOX/watch.out")"
is "a second line once the counters moved" "20%" "$(sed -n 2p "$SANDBOX/watch.out")"

# A closed pipe ends a watcher whose reading never changes.
start=$SECONDS
timeout 20 "$BIN/mirqab" bar mem --watch --format plain | head -1 >/dev/null
is "a closed pipe ends --watch" 0 "${PIPESTATUS[0]}"
((SECONDS - start < 15)) && ok || not_ok "it ended within a few intervals"
is "no subscriber is left" 0 "$("$BIN/mirqab" status --json | jq -r '.streams.system.subscribers')"

stop_daemon
