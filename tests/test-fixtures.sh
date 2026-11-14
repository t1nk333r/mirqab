# tests/test-fixtures.sh — the generated fixture files are what their table
# says: tools/mkfixture.sh procs, run into a scratch directory, writes
# exactly the proc/<pid>/ trees and proc/self/ns links committed under
# tests/fixtures/root-desktop and root-desktop.t1 (plan 006). Edit
# tests/fixtures/procs.tsv and run the generator; never the files. With
# --clones K it adds K offset copies of the user's processes (part 4).
# mirqab:covers tools/mkfixture.sh tests/fixtures/procs.tsv

sandbox

out="$SANDBOX/fixtures"
capture "$REPO/tools/mkfixture.sh" procs --out "$out"
status_is "mkfixture.sh procs exits 0" 0 "$STATUS"

for tree in root-desktop root-desktop.t1; do
    # The generated part of each tree: the numbered proc dirs and self/ns.
    listing() {
        (cd "$1/proc" && find . -path './[0-9]*' -o -path './self/ns*' | LC_ALL=C sort)
    }
    is "$tree: the generator writes the committed proc entries" \
        "$(listing "$FIXTURES/$tree")" "$(listing "$out/$tree")"
    # Links are compared as links: their targets name a live machine's
    # paths, which must never be followed.
    bad=""
    for d in "$out/$tree"/proc/[0-9]* "$out/$tree/proc/self/ns"; do
        rel=${d#"$out/$tree/"}
        diff -r --no-dereference "$FIXTURES/$tree/$rel" "$d" >/dev/null 2>&1 || bad+=" $rel"
    done
    is "$tree: every generated file matches" "" "$bad"
done

# The table is the only input: an unknown extra is refused, not skipped.
mkdir -p "$SANDBOX/bad/tools" "$SANDBOX/bad/tests/fixtures"
cp "$REPO/tools/mkfixture.sh" "$SANDBOX/bad/tools/"
printf 'both\t9\t1\t1000\tS\tx\t9\t9\t-\t1\t0\t0\t0\t0\t1\t1\t1\t/\t-\tx\tbogus\n' \
    >"$SANDBOX/bad/tests/fixtures/procs.tsv"
capture "$SANDBOX/bad/tools/mkfixture.sh" procs
status_is "an unknown extra fails the generator" 1 "$STATUS"
like "and names it" "$OUTPUT" "unknown extra: bogus"

# --clones 1: every user's process once more, 100000 on, its parents and
# session offset with it, its unit suffixed (plan 006, part 4).
clones="$SANDBOX/clones"
capture "$REPO/tools/mkfixture.sh" procs --out "$clones" --clones 1
status_is "mkfixture.sh procs --clones 1 exits 0" 0 "$STATUS"
count() { find "$1/proc" -mindepth 1 -maxdepth 1 -name '[0-9]*' | wc -l | tr -d ' '; }
users=$(awk -F'\t' '!/^#/ && NF > 3 && ($1 == "both" || $1 == "t0") && $4 == 1000 { n++ } END { print n }' "$FIXTURES/procs.tsv")
is "the user's processes are written twice" "$(($(count "$FIXTURES/root-desktop") + users))" "$(count "$clones/root-desktop")"
is "a clone's pid, parent, group and session are offset" \
    "102001 (browser) S 102000 102000 102000" "$(cut -d' ' -f1-6 "$clones/root-desktop/proc/102001/stat")"
like "and its unit is suffixed" "$(<"$clones/root-desktop/proc/102001/cgroup")" "app-Hyprland-browser.sh-5650fedc-c1.scope"
is "a parent outside the user's stays" "1" "$(cut -d' ' -f4 "$clones/root-desktop/proc/101100/stat")"
[[ ! -e $clones/root-desktop/proc/100410 ]] && ok || not_ok "another user's process is not cloned"
capture "$REPO/tools/mkfixture.sh" procs --out "$clones" --clones x
status_is "--clones takes a number" 64 "$STATUS"
