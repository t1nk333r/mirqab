# tests/test-affected.sh — tools/affected-tests.sh selects exactly the test
# files that cover a changed path, widens on global and shared paths, skips
# exempt ones, and its --check fails on an uncovered file.
# mirqab:covers tools/affected-tests.sh tools/coverage-exempt.txt tests/run.sh

sandbox

# repo — a scratch git repository with the selector, a base commit and three
# tests: one covering crates/a, one covering crates/b, one covers-shared.
repo="$SANDBOX/repo"
mkdir -p "$repo"/{tools,tests,crates/a,crates/b,crates/mirqab-proto,plans,.github/workflows}
cp "$REPO/tools/affected-tests.sh" "$repo/tools/"
printf 'plans/**  # the record\n' >"$repo/tools/coverage-exempt.txt"
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

printf 'ok\n' >"$repo/tests/test-bare.sh"
capture bash -c "cd '$repo' && tools/affected-tests.sh --check"
status_is "--check fails on a test without a covers line" 1 "$STATUS"
like "--check names the undeclared test" "$OUTPUT" "tests/test-bare.sh"
