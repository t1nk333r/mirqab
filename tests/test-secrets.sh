# tests/test-secrets.sh — tools/secrets.sh flags host addresses (bare or in
# CIDR form), MAC addresses, fine coordinates and denied strings, and allows
# network ranges and the documentation ranges. gitleaks is stubbed: its own
# rules are its business; this file tests the patterns around it.
# mirqab:covers tools/secrets.sh tools/secrets-deny.txt

sandbox

repo="$SANDBOX/repo"
mkdir -p "$repo/tools"
cp "$REPO/tools/secrets.sh" "$REPO/tools/secrets-deny.txt" "$repo/tools/"
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

scan 'nothing private here'
status_is "a clean tree passes" 0 "$STATUS"

scan 'inet 10.20.30.40/24 brd 10.20.30.255'
status_is "a host in CIDR form is flagged" 1 "$STATUS"
like "and named" "$OUTPUT" "10.20.30.40/24"

scan 'peer 100.101.102.103/32'
status_is "a /32 tailnet peer is flagged" 1 "$STATUS"

scan 'host 192.168.1.50'
status_is "a bare private address is flagged" 1 "$STATUS"

scan 'tailnet range 100.64.0.0/10, documentation 192.0.2.10/24 and 203.0.113.7, loopback 127.0.0.1'
status_is "network ranges and documentation addresses pass" 0 "$STATUS"

scan 'mac aa:bb:cc:dd:ee:ff'
status_is "a MAC address is flagged" 1 "$STATUS"

scan '"latitude": 24.71361'
status_is "a fine coordinate is flagged" 1 "$STATUS"

scan '"latitude": 24.71'
status_is "a city-level coordinate passes" 0 "$STATUS"

# A local deny list whose last line has no newline still counts.
printf 'lunabox' >"$SANDBOX/deny"
export MIRQAB_SECRETS_DENY="$SANDBOX/deny"
scan 'connect to lunabox'
status_is "a denied string without a trailing newline is caught" 1 "$STATUS"
like "and named" "$OUTPUT" "denied string"
unset MIRQAB_SECRETS_DENY
