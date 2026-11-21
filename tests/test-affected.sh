# tests/test-affected.sh — tools/affected-tests.sh selects exactly the test
# files that cover a changed path, widens on global and shared paths, skips
# exempt ones, and its --check fails on an uncovered file; and the sandbox
# of tests/lib.sh keeps the caller's git environment out of the scratch
# repositories such tests write.
# mirqab:covers tools/affected-tests.sh tools/coverage-exempt.txt tests/run.sh
# mirqab:covers tests/lib.sh

sandbox

# repo — a scratch git repository with the selector, a base commit and three
# tests: one covering crates/a, one covering crates/b, one covers-shared.
repo="$SANDBOX/repo"
mkdir -p "$repo"/{tools,tests,crates/a,crates/b,crates/mirqab-proto,plans,.github/workflows}
cp "$REPO/tools/affected-tests.sh" "$repo/tools/"
# No trailing newline: the last exempt glob still counts.
printf 'plans/**  # the record' >"$repo/tools/coverage-exempt.txt"
printf '# mirqab:covers crates/a/**\nok\n' >"$repo/tests/test-a.sh"
printf '# mirqab:covers crates/b/**\nok\n' >"$repo/tests/test-b.sh"
printf '# mirqab:covers-shared\nok\n' >"$repo/tests/test-shared.sh"
# Every GLOBAL entry must match a file, or --check calls it stale.
for f in crates/a/x.rs crates/b/y.rs crates/mirqab-proto/frame.rs plans/001-x.md tests/lib.sh \
    tests/run.sh .github/workflows/ci.yml; do
    : >"$repo/$f"
done
(
    cd "$repo" || exit
    git init -q -b main
    git -c user.name=t -c user.email=t@example.invalid add -A
    git -c user.name=t -c user.email=t@example.invalid commit -qm base
) >/dev/null

# select_after PATH — from a clean tree, change PATH and print the selection.
select_after() {
    (
        cd "$repo" || exit
        git checkout -q -- . && git clean -qfd
        mkdir -p "$(dirname "$1")"
        printf 'change\n' >>"$1"
        tools/affected-tests.sh --quiet main | paste -sd' '
    )
}

is "a covered path selects its test" "tests/test-a.sh" "$(select_after crates/a/x.rs)"
is "another covered path selects only its test" "tests/test-b.sh" "$(select_after crates/b/y.rs)"
is "a shared path selects the covers-shared tests" "tests/test-shared.sh" "$(select_after crates/mirqab-proto/frame.rs)"
is "a global path selects everything" "all" "$(select_after tests/lib.sh)"
is "an exempt path selects nothing" "" "$(select_after plans/001-x.md)"
is "an uncovered path selects everything" "all" "$(select_after crates/c/new.rs)"
is "a changed test selects itself" "tests/test-b.sh" "$(select_after tests/test-b.sh)"
is "--paths prints the changed path" "crates/a/x.rs" "$(cd "$repo" && git checkout -q -- . && git clean -qfd && printf 'z\n' >>crates/a/x.rs && tools/affected-tests.sh --paths main)"

# --paths-for names the changed paths that select one test, or `all`.
paths_for() {
    (
        cd "$repo" || exit
        git checkout -q -- . && git clean -qfd
        for p in "$@"; do printf 'change\n' >>"$p"; done
        tools/affected-tests.sh --quiet --paths-for tests/test-a.sh main | paste -sd' '
    )
}
is "--paths-for lists the test's paths" "crates/a/x.rs" "$(paths_for crates/a/x.rs crates/b/y.rs)"
is "--paths-for is empty when nothing selects the test" "" "$(paths_for crates/b/y.rs)"
is "--paths-for says all on a global change" "all" "$(paths_for crates/a/x.rs tests/lib.sh)"

(cd "$repo" && git checkout -q -- . && git clean -qfd)
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
status_is "--check passes when every file is covered" 0 "$STATUS"

: >"$repo/crates/orphan.rs"
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
status_is "--check fails on an uncovered file" 1 "$STATUS"
like "--check names the uncovered file" "$OUTPUT" "crates/orphan.rs"
rm -f "$repo/crates/orphan.rs"

printf '# mirqab:covers crates/gone/**\nok\n' >"$repo/tests/test-stale.sh"
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
status_is "--check fails on a glob that matches nothing" 1 "$STATUS"
like "--check names the stale glob" "$OUTPUT" "crates/gone/**"
rm -f "$repo/tests/test-stale.sh"

# A file deleted from the working tree that sorts last: --check used to stop
# there with status 1 and no word. It must say what is wrong.
printf '# mirqab:covers crates/a/** zz/**\nok\n' >"$repo/tests/test-a.sh"
mkdir -p "$repo/zz" && : >"$repo/zz/last.txt"
(cd "$repo" && git add -A && git -c user.name=t -c user.email=t@example.invalid commit -qm zz)
rm "$repo/zz/last.txt"
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
like "--check explains a file deleted last in order" "$OUTPUT" "zz/**"
(cd "$repo" && git checkout -q -- .)

printf 'ok\n' >"$repo/tests/test-bare.sh"
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
status_is "--check fails on a test without a covers line" 1 "$STATUS"
like "--check names the undeclared test" "$OUTPUT" "tests/test-bare.sh"

# The sandbox drops the caller's git environment (audit of the gates): a
# pre-commit hook exports GIT_INDEX_FILE, and in a linked worktree GIT_DIR,
# and git obeys them over `-C`, so the scratch repositories above would
# write into the real one. A decoy repository stands for it here.
decoy="$SANDBOX/decoy"
mkdir -p "$decoy"
printf 'decoy\n' >"$decoy/kept"
(
    cd "$decoy" || exit
    git init -q -b main
    git add -A
    git -c user.name=t -c user.email=t@example.invalid commit -qm decoy
) >/dev/null
decoy_head="$(git -C "$decoy" rev-parse HEAD)"
# sandbox_git LIB — in a fresh shell whose git variables point at the decoy,
# as a hook's would, source LIB, call sandbox and commit in a scratch
# repository; prints what the scratch repository tracks.
sandbox_git() {
    env GIT_DIR="$decoy/.git" GIT_INDEX_FILE="$decoy/.git/index" GIT_WORK_TREE="$decoy" \
        GIT_CONFIG_PARAMETERS="'user.name=decoy'" bash -c '
        set -Eeuo pipefail
        source "$1"
        sandbox
        for v in GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE GIT_CONFIG_PARAMETERS; do
            [[ -z ${!v+x} ]] || echo "still set: $v"
        done
        s="$SANDBOX/scratch"
        mkdir -p "$s" && printf "scratch\n" >"$s/new"
        git -C "$s" init -q -b main >/dev/null
        git -C "$s" add -A
        git -C "$s" -c user.name=t -c user.email=t@example.invalid commit -qm scratch
        git -C "$s" ls-files
    ' _ "$1" 2>&1
}
capture sandbox_git "$REPO/tests/lib.sh"
status_is "a scratch commit under the sandbox succeeds" 0 "$STATUS"
unlike "the sandbox unsets the caller's git variables" "$OUTPUT" "still set"
is "the scratch repository holds only its own file" "new" "$OUTPUT"
is "the caller's repository keeps its HEAD" "$decoy_head" "$(git -C "$decoy" rev-parse HEAD)"
is "and its index" "kept" "$(git -C "$decoy" ls-files)"
