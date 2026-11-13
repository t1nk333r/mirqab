#!/usr/bin/env bash
# tools/affected-tests.sh — the tests/test-*.sh files a change affects.
#
#   tools/affected-tests.sh [--committed] [--quiet] [BASE]
#       Prints the affected test files, one per line and sorted, or the word
#       `all`. The change is every path that differs between HEAD and its merge
#       base with BASE (default origin/main), deleted and renamed paths by their
#       old and their new name. Without --committed, staged, unstaged and
#       untracked (not ignored) files count too; CI passes --committed. Why each
#       path selects what it does goes to stderr unless --quiet.
#   tools/affected-tests.sh --paths [--committed] [BASE]
#       Prints the changed paths themselves, one per line.
#   tools/affected-tests.sh --paths-for TEST [--committed] [BASE]
#       Prints the changed paths that select TEST, one per line, or `all`
#       when the selection is every test (tests/test-rust.sh narrows cargo
#       with it). Why each path selects what it does is printed unless
#       --quiet; there is no separate --why.
#   tools/affected-tests.sh --check
#       Fails, listing them, on a file that is neither global, exempt nor
#       covered by a test, on a covers glob or a GLOBAL/EXEMPT entry that matches
#       no file, on a malformed glob, on a test without a covers line and on a
#       file name with a control character.
# All stop with status 2 when git cannot read this checkout (another user's,
# without safe.directory) or lists no file: never an empty, passing answer.
#
# Adapted from haseen tools/affected-tests.sh (MIT, (c) 2026 t1nk33r;
# NOTICE.md).
#
# Each tests/test-*.sh names what it exercises in its leading comment block
# (the lines before its first command), one or more lines of
#   # mirqab:covers GLOB [GLOB...]
# and, when it depends on the shared infrastructure (SHARED below), the line
#   # mirqab:covers-shared
# A changed path selects, in this order:
#   0. every test, if its name has a control character (a tab, a newline):
#      the formats below cannot carry it, so it is never matched, nor skipped;
#   1. every test, if it matches GLOBAL;
#   2. itself, if it is a tests/test-*.sh;
#   3. nothing, if it matches NOWIDEN: many tests read it, none depends on it;
#   4. every test whose covers match it, and every covers-shared test if it
#      matches SHARED;
#   5. nothing, if it matches EXEMPT (tools/coverage-exempt.txt);
#   6. every test otherwise: a file no test claims is never skipped silently.
# The full suite still runs on every push to main (.github/workflows/ci.yml), so
# a declaration that misses something shows up red there.
#
# Globs are matched against repository-relative paths, whole path only:
#   *      any characters except /        ?      one character except /
#   [abc]  one of the characters ([!a] negates)
#   **     a whole path segment: zero or more directories (a/**/b, **/b);
#          trailing, everything below (a/**)
#   {a,b}  either alternative (not nested; may contain /, not **)
# Nothing else is special. A glob cannot contain a space, ^ or \ (use ?).
set -Eeuo pipefail
cd "$(dirname "$0")/.."
# sort, comm and the regexes all byte-wise, whatever the caller's locale.
export LC_ALL=C

# Paths every test depends on: a change here runs the whole suite.
GLOBAL=(
    tests/lib.sh                 # sourced into every test file's subshell
    tests/run.sh                 # the runner itself
    tools/affected-tests.sh      # this selector: a fault here can skip any test
    tools/coverage-exempt.txt    # the selector's EXEMPT list
    '.github/workflows/**'       # CI: what runs, where and as whom
)

# Shared infrastructure (architecture §16, CLAUDE.md): a change here selects
# every test that declares `# mirqab:covers-shared`, on top of its own covers.
# These globs may match nothing yet; a plan's later phase adds the files.
SHARED=(
    'crates/mirqab-proto/**'     # the protocol every crate speaks
    'crates/mirqab-core/src/{root,exec,act,server}.rs'
    Cargo.toml                   # workspace members, shared versions, profiles
    Cargo.lock                   # every dependency's exact version
    deny.toml                    # what cargo deny lets in
    rust-toolchain.toml          # the compiler
)

# Paths no test reads: a change here alone runs no test. A covers glob wins
# over this list. One glob per line in tools/coverage-exempt.txt, each with a
# reason after `#`.
EXEMPT=()
while IFS= read -r line || [[ -n $line ]]; do
    line=${line%%#*}
    line=${line//[[:space:]]/}
    [[ -z $line ]] || EXEMPT+=("$line")
done <tools/coverage-exempt.txt

# Paths many tests read but whose content never changes what they do. Empty
# for now; the mechanism is kept from haseen for a version file.
NOWIDEN=()

usage() { sed -n '2,25s/^# \{0,1\}//p' "$0"; }

# glob_regex GLOB — sets REGEX to the anchored POSIX ERE for GLOB; fails on
# bad syntax. (A variable, not output: no subshell per glob.)
glob_regex() {
    local g=$1 re='' c j body i=0 n=${#1} brace=0
    REGEX=
    [[ -n $g ]] || return 1
    while ((i < n)); do
        c=${g:i:1}
        case $c in
        '*')
            if [[ ${g:i:2} == '**' ]]; then
                ((brace == 0)) || return 1
                ((i == 0)) || [[ ${g:i-1:1} == / ]] || return 1
                if ((i + 2 == n)); then
                    re+='.+'
                    i=$((i + 2))
                    continue
                fi
                [[ ${g:i+2:1} == / ]] || return 1
                re+='([^/]+/)*'
                i=$((i + 3))
                continue
            fi
            re+='[^/]*'
            ;;
        '?') re+='[^/]' ;;
        '[')
            j=$((i + 1))
            [[ ${g:j:1} == '!' || ${g:j:1} == '^' ]] && j=$((j + 1))
            [[ ${g:j:1} == ']' ]] && j=$((j + 1))
            while ((j < n)) && [[ ${g:j:1} != ']' ]]; do j=$((j + 1)); done
            ((j < n)) || return 1
            body=${g:i+1:j-i-1}
            [[ $body == '!'* ]] && body="^${body:1}"
            [[ $body == *\\* || $body == *'['* ]] && return 1
            re+="[$body]"
            i=$j
            ;;
        '{')
            ((brace == 0)) || return 1
            brace=1
            re+='('
            ;;
        ',') if ((brace)); then re+='|'; else re+=','; fi ;;
        '}')
            ((brace)) || return 1
            brace=0
            re+=')'
            ;;
        '.' | '+' | '(' | ')' | '|' | '$') re+="[$c]" ;;
        '^' | \\ | ' ' | $'\t') return 1 ;;
        *) re+=$c ;;
        esac
        i=$((i + 1))
    done
    ((brace == 0)) || return 1
    REGEX="^$re\$"
}

# covers — "TEST<TAB>GLOB" for every glob in a test file's leading comments.
covers() {
    awk 'FNR == 1 { head = 1 }
        head && !/^#/ && !/^[[:space:]]*$/ { head = 0 }
        head && /^# mirqab:covers-shared[[:space:]]*$/ { print FILENAME "\t@shared"; next }
        head && /^# mirqab:covers[[:space:]]/ {
            sub(/^# mirqab:covers[[:space:]]+/, "")
            n = split($0, g, /[[:space:]]+/)
            for (i = 1; i <= n; i++) if (g[i] != "") print FILENAME "\t" g[i]
        }' tests/test-*.sh
}

# rules FILE — writes "KIND<TAB>OWNER<TAB>GLOB<TAB>REGEX" lines for GLOBAL,
# EXEMPT and every covers glob to FILE; prints each malformed glob and fails.
rules() {
    local g bad=0 test REGEX
    {
        for g in "${GLOBAL[@]}"; do
            glob_regex "$g" || { echo "  GLOBAL: malformed glob: $g" >&3; bad=1; continue; }
            printf 'global\t-\t%s\t%s\n' "$g" "$REGEX"
        done
        for g in "${EXEMPT[@]}"; do
            glob_regex "$g" || { echo "  EXEMPT: malformed glob: $g" >&3; bad=1; continue; }
            printf 'exempt\t-\t%s\t%s\n' "$g" "$REGEX"
        done
        for g in "${NOWIDEN[@]}"; do
            glob_regex "$g" || { echo "  NOWIDEN: malformed glob: $g" >&3; bad=1; continue; }
            printf 'nowiden\t-\t%s\t%s\n' "$g" "$REGEX"
        done
        while IFS=$'\t' read -r test g; do
            if [[ $g == @shared ]]; then
                # A covers-shared test claims every SHARED glob. Kind "shared"
                # selects like "covers" but is never reported stale: SHARED
                # names files later phases add.
                local s
                for s in "${SHARED[@]}"; do
                    glob_regex "$s" || { echo "  SHARED: malformed glob: $s" >&3; bad=1; continue; }
                    printf 'shared\t%s\t%s\t%s\n' "$test" "$s" "$REGEX"
                done
                continue
            fi
            glob_regex "$g" || { echo "  $test: malformed glob: $g" >&3; bad=1; continue; }
            printf 'covers\t%s\t%s\t%s\n' "$test" "$g" "$REGEX"
        done < <(covers)
    } 3>&1 >"$1"
    return "$bad"
}

# match PATHS RULES — "M<TAB>PATH<TAB>KIND<TAB>OWNER" for every match and
# "Z<TAB>KIND<TAB>OWNER<TAB>GLOB" for every rule that matches no path. The
# rules form the outer loop so each regex is compiled once.
match() {
    awk -F'\t' 'FILENAME == ARGV[1] { p[++n] = $0; next }
        {
            hit = 0
            for (i = 1; i <= n; i++)
                if (p[i] ~ $4) { print "M\t" p[i] "\t" $1 "\t" $2; hit = 1 }
            if (!hit) print "Z\t" $1 "\t" $2 "\t" $3
        }' "$1" "$2"
}

tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

# require_git — git must read this checkout as its own repository. It refuses
# one another user owns ("dubious ownership", until safe.directory names it),
# and a copy outside any checkout would be read as its parent's. Either way no
# file list can be trusted, so stop instead of selecting or checking nothing.
require_git() {
    local top err
    if ! top="$(git rev-parse --show-toplevel 2>"$tmp/git.err")"; then
        err="$(<"$tmp/git.err")"
        echo "affected-tests: git cannot read this repository, so nothing can be selected or checked:" >&2
        printf '  %s\n' "${err//$'\n'/$'\n'  }" >&2
        echo "  (a checkout another user owns needs: git config --global --add safe.directory \"$PWD\")" >&2
        exit 2
    fi
    if [[ $top != "$(pwd -P)" ]]; then
        echo "affected-tests: git's repository is $top, not this checkout ($(pwd -P)); refusing to guess" >&2
        exit 2
    fi
}

# nul_paths FILE — FILE's NUL-separated paths as lines. A name with a control
# character (a tab or a newline among them) cannot travel in the line and
# tab formats below, so it goes to $tmp/unsafe instead, printed with %q.
nul_paths() {
    local f
    while IFS= read -r -d '' f; do
        [[ -n $f ]] || continue
        if [[ $f == *[[:cntrl:]]* ]]; then
            printf '%q\n' "$f" >>"$tmp/unsafe"
        else
            printf '%s\n' "$f"
        fi
    done <"$1"
}

check() {
    local fail=0 f
    if ! rules "$tmp/rules" >"$tmp/bad"; then
        echo "affected-tests: malformed globs (see the syntax in tools/affected-tests.sh):" >&2
        cat "$tmp/bad" >&2
        fail=1
    fi
    # Every file a commit would carry: tracked and new, minus deleted ones.
    require_git
    git ls-files -z --cached --others --exclude-standard >"$tmp/ls" ||
        { echo "affected-tests: git ls-files failed" >&2; return 2; }
    : >"$tmp/unsafe"
    nul_paths "$tmp/ls" | while IFS= read -r f; do
        if [[ -e $f || -L $f ]]; then printf '%s\n' "$f"; fi
    done | sort -u >"$tmp/paths"
    if [[ ! -s $tmp/paths ]]; then
        echo "affected-tests: git listed no files; an empty tree is not a covered one" >&2
        return 2
    fi
    if [[ -s $tmp/unsafe ]]; then
        echo "affected-tests: file names with a control character (tab, newline, ...) cannot be matched; rename them:" >&2
        sed 's/^/  /' "$tmp/unsafe" >&2
        fail=1
    fi
    match "$tmp/paths" "$tmp/rules" >"$tmp/match"

    grep -E '^tests/test-[^/]*\.sh$' "$tmp/paths" | sort >"$tmp/tests"
    awk -F'\t' '$1 == "covers" || $1 == "shared" { print $2 }' "$tmp/rules" | sort -u >"$tmp/declared"
    comm -23 "$tmp/tests" "$tmp/declared" >"$tmp/undeclared"
    if [[ -s $tmp/undeclared ]]; then
        echo "affected-tests: test files without a '# mirqab:covers' line:" >&2
        sed 's/^/  /' "$tmp/undeclared" >&2
        fail=1
    fi

    awk -F'\t' '$1 == "Z" && $2 != "shared" { print "  " ($2 == "covers" ? $3 : toupper($2)) ": " $4 }' "$tmp/match" >"$tmp/stale"
    if [[ -s $tmp/stale ]]; then
        echo "affected-tests: globs that match no file (stale declarations):" >&2
        cat "$tmp/stale" >&2
        fail=1
    fi

    # A test file always covers itself.
    { awk -F'\t' '$1 == "M" { print $2 }' "$tmp/match"; cat "$tmp/tests"; } | sort -u >"$tmp/claimed"
    comm -23 "$tmp/paths" "$tmp/claimed" >"$tmp/uncovered"
    if [[ -s $tmp/uncovered ]]; then
        echo "affected-tests: files no test covers and none of GLOBAL, EXEMPT or NOWIDEN lists" >&2
        echo "  (add a '# mirqab:covers' glob to the tests that exercise them):" >&2
        sed 's/^/  /' "$tmp/uncovered" >&2
        fail=1
    fi
    ((fail == 0)) || return 1
    echo "affected-tests: $(wc -l <"$tmp/paths") files, $(wc -l <"$tmp/tests") tests," \
        "$(grep -c '^covers' "$tmp/rules") covers globs: every file is covered, global or exempt"
}

select_tests() {
    local committed=$1 quiet=$2 base=$3 mb f
    require_git
    git rev-parse --verify --quiet "$base^{commit}" >/dev/null ||
        { echo "affected-tests: unknown base '$base' (fetch it, or name a commit)" >&2; return 2; }
    mb="$(git merge-base "$base" HEAD)" ||
        { echo "affected-tests: '$base' and HEAD share no history" >&2; return 2; }
    git diff -z --name-only --no-renames "$mb" HEAD >"$tmp/diff" ||
        { echo "affected-tests: git diff $mb HEAD failed" >&2; return 2; }
    if ((committed == 0)); then
        { git diff -z --name-only --no-renames HEAD && git ls-files -z --others --exclude-standard; } \
            >>"$tmp/diff" || { echo "affected-tests: reading the working tree failed" >&2; return 2; }
    fi
    : >"$tmp/unsafe"
    nul_paths "$tmp/diff" | sort -u >"$tmp/changed"
    sort -u -o "$tmp/unsafe" "$tmp/unsafe"
    if ((paths_only)); then
        cat "$tmp/changed"
        return 0
    fi
    rules "$tmp/rules" >"$tmp/bad" || {
        echo "affected-tests: malformed globs; run tools/affected-tests.sh --check" >&2
        cat "$tmp/bad" >&2
        return 2
    }
    # One line per changed path: PATH, G if global, E if exempt, the covering
    # tests; "-" for none, as read collapses empty tab-separated fields.
    match "$tmp/changed" "$tmp/rules" | awk -F'\t' '
        FILENAME == ARGV[1] { ch[++n] = $0; next }
        $1 == "M" && $3 == "global" { g[$2] = "G" }
        $1 == "M" && $3 == "exempt" { e[$2] = "E" }
        $1 == "M" && $3 == "nowiden" { w[$2] = "N" }
        ($1 == "M" && ($3 == "covers" || $3 == "shared")) { c[$2] = c[$2] " " $4 }
        END {
            for (i = 1; i <= n; i++) {
                p = ch[i]
                print p "\t" (p in g ? "G" : "-") "\t" (p in e ? "E" : "-") "\t" (p in c ? c[p] : "-") "\t" (p in w ? "N" : "-")
            }
        }
    ' "$tmp/changed" - >"$tmp/classified"

    local -A selected=()
    local all=0 why=() t global exempt cov nowiden tests for_paths=()
    while IFS= read -r f; do
        all=1
        why+=("$f: a control character in the name, which no glob can be matched against, every test")
    done <"$tmp/unsafe"
    while IFS=$'\t' read -r f global exempt cov nowiden; do
        if [[ $global == G ]]; then
            all=1
            why+=("$f: global, every test")
            continue
        fi
        if [[ $nowiden == N ]]; then
            why+=("$f: read by many tests, depended on by none, no test")
            continue
        fi
        tests=()
        [[ $cov == - ]] || read -ra tests <<<"$cov"
        [[ $f =~ ^tests/test-[^/]*\.sh$ && -f $f ]] && tests+=("$f")
        if ((${#tests[@]} > 0)); then
            for t in "${tests[@]}"; do
                selected[$t]=1
                [[ $t != "$paths_for" ]] || for_paths+=("$f")
            done
            why+=("$f: $(printf '%s\n' "${tests[@]}" | sort -u | sed 's#^tests/##' | paste -sd' ')")
        elif [[ $exempt == E ]]; then
            why+=("$f: exempt, no test")
        elif [[ $f =~ ^tests/test-[^/]*\.sh$ ]]; then
            why+=("$f: a deleted test, nothing to run")
        else
            all=1
            why+=("$f: no test covers it and it is neither global nor exempt, every test")
        fi
    done <"$tmp/classified"

    if ((quiet == 0)); then
        {
            printf 'affected-tests: %s changed path(s) since %s (merge base of %s and HEAD)%s\n' \
                "$(($(wc -l <"$tmp/changed") + $(wc -l <"$tmp/unsafe")))" "$(git rev-parse --short "$mb")" "$base" \
                "$( ((committed)) && echo ', commits only' || echo ', commits and working tree')"
            ((${#why[@]} == 0)) || printf '  %s\n' "${why[@]}"
        } >&2
    fi
    if ((all)); then
        echo all
    elif [[ -n $paths_for ]]; then
        ((${#for_paths[@]} == 0)) || printf '%s\n' "${for_paths[@]}" | sort -u
    elif ((${#selected[@]} > 0)); then
        printf '%s\n' "${!selected[@]}" | sort
    fi
}

committed=0 quiet=0 base="" paths_only=0 paths_for=""
while (($# > 0)); do
    case $1 in
    -h | --help) usage; exit 0 ;;
    --check) check; exit ;;
    --committed) committed=1 ;;
    --paths) paths_only=1 ;;
    --paths-for)
        shift
        [[ -n ${1:-} ]] || { echo "affected-tests: --paths-for needs a test file" >&2; exit 2; }
        paths_for=$1
        ;;
    -q | --quiet) quiet=1 ;;
    -*) echo "affected-tests: unknown option $1 (see --help)" >&2; exit 2 ;;
    *)
        [[ -z $base ]] || { echo "affected-tests: one BASE only" >&2; exit 2; }
        base=$1
        ;;
    esac
    shift
done
select_tests "$committed" "$quiet" "${base:-origin/main}"
