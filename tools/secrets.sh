#!/usr/bin/env bash
# tools/secrets.sh — run before every push to the public repository
# (CLAUDE.md "The repository is public", architecture §10, plan 001 step 9).
#   1. gitleaks over every ref a push can reach;
#   2. privacy patterns gitleaks does not know, over tracked files and every
#      added line in history: IPv4 addresses outside the documentation ranges,
#      CGNAT (tailnet) addresses, MAC addresses, coordinates finer than a
#      city, token shapes, home paths, personal e-mail addresses, private keys,
#      and the literal strings of tools/secrets-deny.txt plus the local file
#      named by MIRQAB_SECRETS_DENY (a machine's own hostnames).
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
trap 'rm -f -- "$found"' EXIT
report() {
    local hits
    hits="$(sed '/^$/d')"
    [[ -n $hits ]] || return 0
    echo "secrets: $1" >&2
    head -20 <<<"$hits" | sed 's/^/  /' >&2
    echo "$1" >>"$found"
}

# scan ERE — matches in tracked files and in lines added anywhere in history.
# This file and the deny list are excluded: they hold the patterns themselves.
scan() {
    git grep -nIE -e "$1" -- . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' || true
    git log -p "${REFS[@]}" -- . ':!tools/secrets.sh' ':!tools/secrets-deny.txt' |
        grep -E '^\+' | grep -E -e "$1" || true
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
    git grep -nIoE -e "$ipv4(/[0-9]+)?" -- . ':!tools/secrets.sh' || true
    git log -p "${REFS[@]}" -- . ':!tools/secrets.sh' | grep -E '^\+' | grep -oE -e "$ipv4(/[0-9]+)?" || true
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

# Literal strings: the repository's list and the machine's local one.
deny_files=(tools/secrets-deny.txt)
[[ -z ${MIRQAB_SECRETS_DENY:-} ]] || deny_files+=("$MIRQAB_SECRETS_DENY")
for f in "${deny_files[@]}"; do
    [[ -f $f ]] || { echo "secrets: deny list $f not found" >&2; fail=1; continue; }
    while IFS= read -r word || [[ -n $word ]]; do
        [[ -n $word && $word != '#'* ]] || continue
        {
            git grep -nIF -e "$word" -- . ':!tools/secrets-deny.txt' || true
            git log -p "${REFS[@]}" -- . ':!tools/secrets-deny.txt' | grep -E '^\+' | grep -F -e "$word" || true
        } | report "a denied string from $f"
    done <"$f"
done

[[ ! -s $found ]] || fail=1
((fail == 0)) && echo "secrets OK"
exit "$fail"
