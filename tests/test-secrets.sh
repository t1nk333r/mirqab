# tests/test-secrets.sh — tools/secrets.sh flags host addresses (bare or in
# CIDR form), MAC addresses, fine coordinates and denied strings, and allows
# network ranges and the documentation ranges. gitleaks is stubbed: its own
# rules are its business; this file tests the patterns around it.
# mirqab:covers tools/secrets.sh tools/secrets-deny.txt tools/secrets-allow.txt

sandbox

repo="$SANDBOX/repo"
mkdir -p "$repo/tools"
cp "$REPO/tools/secrets.sh" "$REPO/tools/secrets-deny.txt" "$repo/tools/"
printf '# none\n' >"$repo/tools/secrets-allow.txt"
stub gitleaks 'exit 0'
export GITLEAKS="$SANDBOX/stubs/gitleaks"
git -C "$repo" init -q -b main

# scan CONTENT — commit CONTENT as the only file besides the tool; run it.
n=0
scan() {
    n=$((n + 1))
    printf '%s\n' "$1" >"$repo/sample.txt"
    git -C "$repo" add -A
    git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm "case $n" --allow-empty
    capture "$repo/tools/secrets.sh"
    # The history scan sees every earlier case too: start each from a fresh repo.
    rm -rf "$repo/.git"
    git -C "$repo" init -q -b main
}

# The samples are built here, not written out, so this file itself passes
# the scan it tests.
d=.
host_cidr="10${d}20${d}30${d}40/24"
peer_cidr="100${d}101${d}102${d}103/32"
private_host="192${d}168${d}1${d}50"
mac="aa:bb:cc:dd:ee:$(printf ff)"
fine="\"latitude\": 24${d}71361"

scan 'nothing private here'
status_is "a clean tree passes" 0 "$STATUS"

scan "inet $host_cidr"
status_is "a host in CIDR form is flagged" 1 "$STATUS"
like "and named" "$OUTPUT" "$host_cidr"

scan "peer $peer_cidr"
status_is "a /32 tailnet peer is flagged" 1 "$STATUS"

scan "host $private_host"
status_is "a bare private address is flagged" 1 "$STATUS"

scan 'tailnet range 100.64.0.0/10, documentation 192.0.2.10/24 and 203.0.113.7, loopback 127.0.0.1'
status_is "network ranges and documentation addresses pass" 0 "$STATUS"

scan "mac $mac"
status_is "a MAC address is flagged" 1 "$STATUS"

scan "$fine"
status_is "a fine coordinate is flagged" 1 "$STATUS"

scan '"latitude": 24.71'
status_is "a city-level coordinate passes" 0 "$STATUS"

# An added line whose text starts with "++ " is scanned, not taken for a
# diff header, even after the next commit removes it.
printf '++ %s\n' "$private_host" >"$repo/sample.txt"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm plusplus
printf 'gone\n' >"$repo/sample.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam gone
capture "$repo/tools/secrets.sh"
status_is "a line starting with ++ in history is scanned" 1 "$STATUS"
rm -rf "$repo/.git"
git -C "$repo" init -q -b main

# The allow list is scanned like any other file.
printf 'host %s\n' "$private_host" >>"$repo/tools/secrets-allow.txt"
scan 'clean'
status_is "an address hidden in the allow list is found" 1 "$STATUS"
printf '# none\n' >"$repo/tools/secrets-allow.txt"

# A local deny list whose last line has no newline still counts.
printf 'lunabox' >"$SANDBOX/deny"
export MIRQAB_SECRETS_DENY="$SANDBOX/deny"
scan 'connect to lunabox'
status_is "a denied string without a trailing newline is caught" 1 "$STATUS"
like "and named" "$OUTPUT" "denied string"
unset MIRQAB_SECRETS_DENY

# A commit on the allow list is skipped in history; the same text in the
# working tree, or in a commit not on the list, is still found.
printf 'host %s\n' "$private_host" >"$repo/sample.txt"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm leak
leak="$(git -C "$repo" rev-parse HEAD)"
printf 'clean\n' >"$repo/sample.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam clean
capture "$repo/tools/secrets.sh"
status_is "a leak in history is found" 1 "$STATUS"
printf '%s  # synthetic, reviewed\n' "$leak" >"$repo/tools/secrets-allow.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam allow
capture "$repo/tools/secrets.sh"
status_is "an allowed commit is skipped" 0 "$STATUS"
printf 'host %s\n' "$private_host" >"$repo/sample.txt"
capture "$repo/tools/secrets.sh"
status_is "the same text in the working tree is still found" 1 "$STATUS"
