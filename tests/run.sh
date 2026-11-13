#!/usr/bin/env bash
# tests/run.sh — hermetic test suite. Runs every tests/test-*.sh (or the ones
# named on the command line, or with --affected [--committed] [BASE] the ones
# a change affects) in its own subshell. Never touches the live system: see
# tests/lib.sh.
#
# Adapted from haseen tests/run.sh (MIT, (c) 2026 t1nk33r; NOTICE.md).
set -Eeuo pipefail

cd "$(dirname "$0")/.."

files=("$@")
# --affected [--committed] [BASE]: only the test files the change since BASE
# affects, as tools/affected-tests.sh chooses them. Pull requests and
# pre-commit runs use it; main runs everything after each merge. The same
# arguments reach tests/test-rust.sh, which narrows cargo to the crates the
# change touched.
export MIRQAB_AFFECTED=""
if [[ ${1:-} == --affected ]]; then
    shift
    selection="$(tools/affected-tests.sh "$@")" || exit
    if [[ -z $selection ]]; then
        echo "no test is affected by this change; nothing to run"
        exit 0
    fi
    MIRQAB_AFFECTED="$*"
    [[ -n $MIRQAB_AFFECTED ]] || MIRQAB_AFFECTED=" "
    files=(tests/test-*.sh)
    total_files=${#files[@]}
    [[ $selection == all ]] || mapfile -t files <<<"$selection"
    echo "affected: ${#files[@]} of $total_files test files"
fi
((${#files[@]} > 0)) || files=(tests/test-*.sh)

total=0
failed=0
# Each file's output goes to a file, not a command substitution: $(…) reads
# to EOF, so a process a test leaves behind holding its stdout would hang
# the run instead of failing it.
result_file=""
trap 'rm -f -- "$result_file"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for f in "${files[@]}"; do
    printf '%s\n' "${f##*/}"
    result_file="$(mktemp)"
    set +e
    (
        set -Eeuo pipefail
        # A test never reads the caller's terminal or pipe.
        exec </dev/null
        # shellcheck source=tests/lib.sh
        source tests/lib.sh
        # shellcheck disable=SC1090
        source "$f"
        printf 'RESULT %d %d\n' "$TESTS_RUN" "$TESTS_FAILED"
    ) >"$result_file"
    rc=$?
    result="$(<"$result_file")"
    rm -f -- "$result_file"
    result_file=""
    set -e
    printf '%s\n' "$result" | grep -v '^RESULT ' || true
    line="$(printf '%s\n' "$result" | grep '^RESULT ' || true)"
    if [[ $rc -ne 0 || -z $line ]]; then
        echo "  FAIL ${f##*/} aborted (exit $rc)" >&2
        failed=$((failed + 1))
        total=$((total + 1))
        continue
    fi
    read -r _ n nf <<<"$line"
    total=$((total + n))
    failed=$((failed + nf))
done

echo "----"
echo "RESULT $((total - failed))/$total passed"
((failed == 0))
