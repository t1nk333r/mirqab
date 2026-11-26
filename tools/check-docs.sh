#!/usr/bin/env bash
# tools/check-docs.sh — read-only integrity check of the written record.
#   1. every plans/NNN-*.md has exactly one "| NNN |" row in plans/README.md,
#      and every row has a file;
#   2. every relative link in a tracked Markdown file resolves;
#   3. every stream named in crates/mirqab-proto/src/stream.rs (its STREAMS
#      list) has tests/golden/<stream>.json.
#
# Adapted from haseen tools/check-docs.sh (MIT, (c) 2026 t1nk33r; NOTICE.md).
set -Eeuo pipefail
cd "$(dirname "$0")/.."
fail=0
err() { printf 'check-docs: %s\n' "$*" >&2; fail=1; }

# 1. plans <-> index. The index is the first table, before the first
# second-level heading; later tables may name plans too.
index="$(awk '/^## / { exit } { print }' plans/README.md)"
for f in plans/[0-9][0-9][0-9]-*.md; do
    [[ -e $f ]] || continue
    n="${f#plans/}"
    n="${n%%-*}"
    c="$(grep -c "^| $n |" <<<"$index" || true)"
    [[ $c == 1 ]] || err "plan $n has $c index rows"
done
while read -r n; do
    compgen -G "plans/$n-*.md" >/dev/null || err "index row $n has no plan file"
done < <(sed -n 's/^| \([0-9][0-9][0-9]\) |.*/\1/p' <<<"$index")

# 2. relative links: [text](target), skipping URLs, mail and in-page anchors.
# Fenced code blocks are skipped: their brackets are code, not links.
# The list is taken first: a process substitution's status is ignored, so a
# checkout git refuses (another user's, without safe.directory) checked no
# link and passed (plan 024 L37).
if ! mds="$(git ls-files -co --exclude-standard -- '*.md')"; then
    printf 'check-docs: git cannot list the Markdown files (a checkout another user owns needs safe.directory); nothing checked\n' >&2
    exit 2
fi
[[ -n $mds ]] || err "git listed no Markdown files"
while IFS= read -r md; do
    [[ -f $md ]] || continue
    dir="$(dirname "$md")"
    while IFS=: read -r line target; do
        target="${target%%#*}"
        [[ -n $target ]] || continue
        [[ -e $dir/$target ]] || err "$md:$line: broken link: $target"
    done < <(awk '
        /^```/ { fence = !fence; next }
        !fence {
            s = $0
            while (match(s, /\]\([^)[:space:]]+\)/)) {
                t = substr(s, RSTART + 2, RLENGTH - 3)
                s = substr(s, RSTART + RLENGTH)
                if (t ~ /^(https?:|mailto:|#)/) continue
                print NR ":" t
            }
        }' "$md")
done <<<"$mds"

# 3. a golden file per stream
stream_rs=crates/mirqab-proto/src/stream.rs
if [[ -f $stream_rs ]]; then
    while read -r s; do
        [[ -f tests/golden/$s.json ]] || err "stream $s has no tests/golden/$s.json"
    done < <(awk '/STREAMS/ { on = 1 } on { print } on && /\];/ { exit }' "$stream_rs" |
        grep -oE '"[a-z][a-z0-9-]*"' | tr -d '"')
fi

((fail == 0)) && echo "check-docs OK"
exit "$fail"
