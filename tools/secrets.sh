#!/usr/bin/env bash
# tools/secrets.sh — run before every push to the public repository
# (CLAUDE.md "The repository is public", architecture §10, plan 001 step 9).
#   1. gitleaks over every ref a push can reach;
#   2. privacy patterns gitleaks does not know, over tracked files, every
#      added line in history and every commit message: IPv4 hosts outside the
#      documentation ranges
#      (private, tailnet and public alike), IPv6 addresses outside ::, ::1
#      and 2001:db8::/32, the hexadecimal addresses of the fixtures' /proc/net
#      tables, MAC addresses, coordinates finer than a
#      city, token shapes, home paths, personal e-mail addresses, private keys,
#      and the literal strings of tools/secrets-deny.txt plus the local file
#      named by MIRQAB_SECRETS_DENY (a machine's own hostnames), each matched
#      as a whole word;
#   3. tools/secrets-deny.txt itself holds nothing but comments, in the tree
#      and in history: the scans skip it, so a value there would be published.
# The history scan, messages included, skips the commits
# tools/secrets-allow.txt lists, each with its reason: commits already pushed whose matches were reviewed and found
# synthetic. A full 40-character hash per line; nothing else is skippable.
# GITLEAKS overrides the binary. Exit 1 on any finding.
#
# Adapted from haseen tools/secrets.sh (MIT, (c) 2026 t1nk33r; NOTICE.md).
set -Eeuo pipefail
cd "$(dirname "$0")/.."

GITLEAKS=${GITLEAKS:-$(command -v gitleaks || true)}
# Only refs a push can reach; local scratch refs are not candidates.
REFS=(--branches --remotes --tags)
fail=0

if [[ -n $GITLEAKS && -x $GITLEAKS ]]; then
    "$GITLEAKS" git --no-banner --redact --log-opts="${REFS[*]}" . || fail=1
else
    echo "secrets: gitleaks not found (set GITLEAKS); the history scan cannot run" >&2
    fail=1
fi

# report LABEL — print matches read from stdin under LABEL. It runs at the
# end of a pipeline, in a subshell, so a finding is recorded in a file.
found="$(mktemp)"
report() {
    local hits
    hits="$(sed '/^$/d')"
    [[ -n $hits ]] || return 0
    echo "secrets: $1" >&2
    head -20 <<<"$hits" | sed 's/^/  /' >&2
    echo "$1" >>"$found"
}

# Every line added anywhere in history, read once, outside the allowed
# commits. This file and the deny list are excluded: they hold the patterns
# and the denied strings themselves (the deny list is checked to hold none,
# below). The allow list is scanned like any file.
allow="$(sed -n 's/^\([0-9a-f]\{40\}\)\([[:space:]].*\)\{0,1\}$/\1/p' tools/secrets-allow.txt 2>/dev/null || true)"
added="$(mktemp)"
trap 'rm -f -- "$found" "$added"' EXIT
# history_added PATHSPEC... — the lines added under PATHSPEC in history,
# each with its leading "+".
history_added() {
    git log -p --format='@@commit %H' "${REFS[@]}" -- "$@" |
        awk -v allow="$allow" '
            BEGIN { n = split(allow, a, "\n"); for (i = 1; i <= n; i++) ok[a[i]] = 1 }
            /^@@commit / { skip = ($2 in ok); hunk = 0; next }
            # From "diff --git" to the first "@@ " is a file header ("+++ b/x"
            # among it); added lines are those inside hunks, whatever they start
            # with ("++ x" and "--- x" are content there).
            /^diff --git / { hunk = 0; next }
            /^@@ / { hunk = 1; next }
            hunk && !skip && /^\+/ { print }
        '
}
history_added . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' >"$added"
# Commit messages, of every commit a push can reach: a squash merge copies
# the branch's messages into main's, so an address pasted into one while
# checking an Unverified item would reach the public history unscanned.
# Identity trailers (Co-authored-by, Signed-off-by) are left out: git and
# GitHub write them from the committers' own addresses, which every commit's
# author and committer fields publish anyway.
git log --format='@@commit %H%n%B' "${REFS[@]}" |
    awk -v allow="$allow" '
        BEGIN { n = split(allow, a, "\n"); for (i = 1; i <= n; i++) ok[a[i]] = 1 }
        /^@@commit [0-9a-f]+$/ && length($2) == 40 { skip = ($2 in ok); next }
        skip { next }
        tolower($0) ~ /^(co-authored-by|signed-off-by):[^<>]*<[^<>]*>[[:space:]]*$/ { next }
        { print }
    ' >>"$added"

# scan ERE — matches in tracked files and in lines added in history.
scan() {
    git grep -nIE -e "$1" -- . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' || true
    grep -E -e "$1" "$added" || true
}

patterns=(
    'MAC address|\b[0-9a-fA-F]{2}(:[0-9a-fA-F]{2}){5}\b'
    'coordinates finer than a city|(lat|lon|lng|latitude|longitude)"?[[:space:]]*[:=][[:space:]]*-?[0-9]{1,3}\.[0-9]{3,}'
    'token shape|\b(sk-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|xox[abp]-[A-Za-z0-9-]{10,})'
    'absolute home path|/home/[a-z_][a-z0-9_-]*/'
    'private key|-----BEGIN [A-Z ]*PRIVATE KEY-----'
    'personal e-mail address|[A-Za-z0-9._%+-]+@(gmail|outlook|hotmail|proton(mail)?|icloud|yahoo)\.[a-z]+'
)
for entry in "${patterns[@]}"; do
    scan "${entry#*|}" | report "${entry%%|*}"
done

# IPv4 addresses outside the documentation ranges (RFC 5737), loopback,
# 0.0.0.0 and the 255.x masks; private, CGNAT (tailnet) and public hosts alike.
# A CIDR is allowed only when its host bits are zero (100.64.0.0/10 names a
# range); `inet 10.20.30.40/24`, as `ip addr` prints it, is a host. 1.1.1.1 is
# the latency target architecture §10 names.
ipv4='\b([0-9]{1,3}\.){3}[0-9]{1,3}\b'
allowed='^(192\.0\.2\.|198\.51\.100\.|203\.0\.113\.|127\.|0\.0\.0\.0$|255\.|1\.1\.1\.1$)'
{
    git grep -nIoE -e "$ipv4(/[0-9]+)?" -- . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' || true
    grep -oE -e "$ipv4(/[0-9]+)?" "$added" || true
} | while IFS= read -r hit; do
    addr="${hit##*:}"
    host="${addr%/*}"
    grep -qE "$allowed" <<<"$host" && continue
    if [[ $addr == */* ]]; then
        bits="${addr#*/}"
        IFS=. read -r a b c d <<<"$host"
        if ((bits < 32 && a <= 255 && b <= 255 && c <= 255 && d <= 255)); then
            n=$(((a << 24) | (b << 16) | (c << 8) | d))
            ((n & ((1 << (32 - bits)) - 1))) || continue
        fi
    fi
    printf '%s\n' "$hit"
done | report "IPv4 address outside the documentation ranges"

# Address classes for the two scans below, as awk functions. IPv4: the
# ranges the scan above allows. IPv6 (eight 16-bit words): :: and ::1, the
# documentation range 2001:db8::/32 (RFC 3849), an IPv4-mapped ::ffff:a.b.c.d
# by its IPv4 address, and a link- or site-local fe80::/9 address (fe80 to
# feff) whose words 2 to 7 are zero, so only a 16-bit identifier is left
# (fe80::1 names no machine; a subnet, or a MAC-derived or random identifier,
# does). Multicast (ff00::/8) is not allowed: the repository uses none, and a
# solicited-node group carries the low bits of a host's address.
addr_lib='
function hexval(s,    i, v) {
    v = 0
    s = tolower(s)
    for (i = 1; i <= length(s); i++) v = v * 16 + index("0123456789abcdef", substr(s, i, 1)) - 1
    return v
}
function v4ok(a, b, c, d) {
    return a == 127 || a == 255 || (a == 0 && b == 0 && c == 0 && d == 0) ||
        (a == 1 && b == 1 && c == 1 && d == 1) || (a == 192 && b == 0 && c == 2) ||
        (a == 198 && b == 51 && c == 100) || (a == 203 && b == 0 && c == 113)
}
function v6ok(w,    zero5) {
    zero5 = w[1] == 0 && w[2] == 0 && w[3] == 0 && w[4] == 0 && w[5] == 0
    if (zero5 && w[6] == 0 && w[7] == 0 && w[8] <= 1) return 1
    if (w[1] == 8193 && w[2] == 3512) return 1
    if (zero5 && w[6] == 65535) return v4ok(int(w[7] / 256), w[7] % 256, int(w[8] / 256), w[8] % 256)
    return w[1] >= 65152 && w[1] <= 65279 && w[2] == 0 && w[3] == 0 && w[4] == 0 &&
        w[5] == 0 && w[6] == 0 && w[7] == 0
}'

# IPv6 addresses as text. A token of letters, digits, dots and colons is
# read with one trailing colon dropped (`...::1: refused`) and a trailing
# dotted quad read as two words. A token that is not all hex and colons is
# an address joined to a word (`addr:2a01:...`, `host_2a01:...`) only when
# what follows its last other character and that word's separator (the
# character itself when it is `_` or `.`, else the word's hex tail and one
# colon, as in `inet6:` or `src:`) is an address of at least two hex
# groups: Rust paths such as std::fs and Kind::abc, and times such as
# at:12:30:45, are not.
v6re='[0-9A-Za-z_.:]*:[0-9A-Za-z_.:]*'
{
    git grep -nIoE -e "$v6re" -- . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' |
        sed 's/^\([^:]*:[0-9]*\):/\1\t/' || true
    grep -oE -e "$v6re" "$added" | sed 's/^/history\t/' || true
} | awk -F'\t' "$addr_lib"'
    {
        t = $2
        sub(/\.+$/, "", t)
        if (t ~ /[^:]:$/) t = substr(t, 1, length(t) - 1)
        if (match(t, /:[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/)) {
            split(substr(t, RSTART + 1), q, ".")
            t = substr(t, 1, RSTART) sprintf("%x:%x", q[1] * 256 + q[2], q[3] * 256 + q[4])
        }
        joined = 0
        if (t ~ /[^0-9A-Fa-f:]/) {
            match(t, /^.*[^0-9A-Fa-f:]/)
            sep = substr(t, RLENGTH, 1)
            t = substr(t, RLENGTH + 1)
            if (sep != "_" && sep != ".") {
                if (!sub(/^[0-9A-Fa-f]*:/, "", t)) next
            }
            joined = 1
        }
        if (t !~ /^[0-9A-Fa-f:]+$/ || t ~ /:::/ || t ~ /^:[^:]/ || t ~ /[^:]:$/) next
        c = t
        dbl = gsub(/::/, "", c)
        n = split(t, g, ":")
        filled = 0
        for (i = 1; i <= n; i++) {
            if (length(g[i]) > 4) next
            if (g[i] != "") filled++
        }
        if (dbl > 1 || (dbl == 0 && filled != 8) || (dbl == 1 && filled > 7)) next
        if (joined && filled < 2) next
        m = 0
        for (i = 1; i <= n; i++) {
            if (g[i] != "") w[++m] = hexval(g[i])
            else if (i == 1 || g[i - 1] != "") for (k = 0; k < 8 - filled; k++) w[++m] = 0
        }
        if (!v6ok(w)) print $1 ": " $2
    }' | report "IPv6 address outside the documentation range"

# Addresses in the hexadecimal /proc/net tables of the fixtures, which the
# text scans cannot read: each 32-bit word is in the kernel's (little-endian)
# byte order. Columns 2 and 3 are the local and remote ADDRESS:PORT.
procnet=()
for t in tcp tcp6 udp udp6; do procnet+=(":(glob)tests/fixtures/**/proc/net/$t"); done
{
    git grep -nIE -e '^[[:space:]]*[0-9]+:' -- "${procnet[@]}" | sed 's/^\([^:]*:[0-9]*\):/\1\t/' || true
    history_added "${procnet[@]}" | sed 's/^+/history\t/'
} | awk -F'\t' "$addr_lib"'
    function word(h, at, k) { # the 32-bit word at hex offset at, byte k (1..4) in network order
        return hexval(substr(h, at + 8 - 2 * k, 2))
    }
    {
        split($2, f, " ")
        for (col = 2; col <= 3; col++) {
            h = f[col]
            sub(/:[0-9A-Fa-f]+$/, "", h)
            if (h !~ /^[0-9A-Fa-f]+$/ || (length(h) != 8 && length(h) != 32)) continue
            if (length(h) == 8) {
                a = word(h, 1, 1); b = word(h, 1, 2); c = word(h, 1, 3); d = word(h, 1, 4)
                if (!v4ok(a, b, c, d)) print $1 ": " f[col] " (" a "." b "." c "." d ")"
                continue
            }
            s = ""
            for (i = 0; i < 4; i++) {
                w[2 * i + 1] = word(h, 1 + 8 * i, 1) * 256 + word(h, 1 + 8 * i, 2)
                w[2 * i + 2] = word(h, 1 + 8 * i, 3) * 256 + word(h, 1 + 8 * i, 4)
                s = s sprintf("%s%x:%x", i ? ":" : "", w[2 * i + 1], w[2 * i + 2])
            }
            if (!v6ok(w)) print $1 ": " f[col] " (" s ")"
        }
    }' | report "address in a /proc/net table outside the documentation ranges"

# The tracked deny list stays empty, in the tree and in its history: every
# scan above skips it, so a hostname written there instead of into the local
# file would pass them all and be published (plan 024 L19).
{
    git grep -nIvE -e '^[[:space:]]*(#|$)' -- tools/secrets-deny.txt || true
    history_added tools/secrets-deny.txt | grep -vE '^\+[[:space:]]*(#|$)' || true
} | report "a value in the tracked tools/secrets-deny.txt (keep it empty; list a machine's names in the file MIRQAB_SECRETS_DENY names)"

# Literal strings: the repository's list and the machine's local one, each
# matched as a whole word, so a short hostname does not flag every longer
# word that contains it (plan 024 L20).
deny_files=(tools/secrets-deny.txt)
[[ -z ${MIRQAB_SECRETS_DENY:-} ]] || deny_files+=("$MIRQAB_SECRETS_DENY")
for f in "${deny_files[@]}"; do
    [[ -f $f ]] || { echo "secrets: deny list $f not found" >&2; fail=1; continue; }
    while IFS= read -r word || [[ -n $word ]]; do
        [[ -n $word && $word != '#'* ]] || continue
        {
            git grep -nIwF -e "$word" -- . ':!tools/secrets-deny.txt' || true
            grep -wF -e "$word" "$added" || true
        } | report "a denied string from $f"
    done <"$f"
done

[[ ! -s $found ]] || fail=1
((fail == 0)) && echo "secrets OK"
exit "$fail"
