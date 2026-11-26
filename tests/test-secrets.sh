# tests/test-secrets.sh — tools/secrets.sh flags host addresses (bare or in
# CIDR form, IPv6, or hexadecimal in a fixture's /proc/net table), MAC
# addresses, fine coordinates and denied strings (as whole words), and a
# value in the tracked deny list, and allows network ranges
# and the documentation ranges. gitleaks is stubbed: its own
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

# IPv6 addresses as text (audit 2026-11-21, L60). Built here like the
# samples above; the global one is in a real provider's range, the
# link-local one carries a MAC-derived identifier.
c=:
global6="2a01${c}4f8${c}c0c${c}1234${c}${c}1"
eui64="fe80${c}${c}1c2b${c}3cff${c}fe4d${c}5e6f"
scan "server $global6"
status_is "a global IPv6 address is flagged" 1 "$STATUS"
like "and named" "$OUTPUT" "IPv6 address outside the documentation range"
scan "link $eui64"
status_is "a link-local address with a hardware identifier is flagged" 1 "$STATUS"
scan "mapped ${c}${c}ffff${c}${private_host}"
status_is "an IPv4-mapped private address is flagged" 1 "$STATUS"
scan 'documentation 2001:db8::1 and 2001:0DB8:0:0:0:0:0:42, loopback ::1, any ::,
mapped ::ffff:192.0.2.1, link-local fe80::1, febf::1 and fec0::1, Rust std::fs::read,
Self::new, Scope::dead, Kind::abc and Foo::1:2, a time 12:30:45 and at:12:30:45,
a socket 0100007F:0BB8, a key "a:" and "dead:beef:", a path a::b::c, x.0.5:1:2,
an address ::1: refused, 2001:db8::1:'
status_is "documentation, loopback and non-address text pass" 0 "$STATUS"
# fe80::/9 is allowed only with words 2 to 7 zero (review of L60): an
# identifier in the middle words, or a prefix past feff, names a machine.
scan "link fe80${c}1234${c}5678${c}9abc${c}${c}1"
status_is "a link-local address with non-zero middle words is flagged" 1 "$STATUS"
scan "site fec0${c}1${c}2${c}3${c}${c}5"
status_is "a site-local address with a subnet is flagged" 1 "$STATUS"
scan "top feff${c}dead${c}beef${c}cafe${c}${c}1"
status_is "an feff address with non-zero words is flagged" 1 "$STATUS"
# Multicast: the repository uses none, so every multicast group is flagged; a
# solicited-node group carries the low bits of a host's own address.
scan "group ff02${c}${c}1${c}ff4d${c}5e6f"
status_is "a solicited-node multicast address is flagged" 1 "$STATUS"
scan "all nodes ff02${c}${c}1"
status_is "a well-known multicast address is flagged" 1 "$STATUS"
# An address joined to a word, or followed by a colon, is still read.
scan "addr${c}$global6"
status_is "an address after a word and a colon is flagged" 1 "$STATUS"
scan "host_$global6"
status_is "an address after a word and an underscore is flagged" 1 "$STATUS"
scan "host.$global6"
status_is "an address after a word and a dot is flagged" 1 "$STATUS"
# A word that ends in hex digits: its hex tail is not part of the address.
for word in inet6 ipv6 v6 dns6 id src; do
    scan "$word${c}$global6"
    status_is "an address after $word and a colon is flagged" 1 "$STATUS"
done
scan "$global6${c} refused"
status_is "an address followed by a colon is flagged" 1 "$STATUS"

# Hexadecimal addresses in a fixture's /proc/net tables, as the kernel
# writes them: each 32-bit word in host (little-endian) byte order.
# plant PATH ROW... — the table PATH (a header and ROWs) as the only file
# besides sample.txt; run the scan; remove it again.
plant() {
    local path=$1
    shift
    mkdir -p "$repo/$(dirname "$path")"
    {
        echo '  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode'
        printf '%s\n' "$@"
    } >"$repo/$path"
    scan 'clean'
    rm -rf "$repo/tests"
}
tail6=' 0A 00000000:00000000 00:00000000 00000000  1000        0 70020 1 0000000000000000 100 0 0 10 0'
tail4=' 0A 00000000:00000000 00:00000000 00000000  1000        0 70010 1 0000000000000000 100 0 0 10 0'
z8=00000000
z32=$z8$z8$z8$z8
lan4=0101A8C0 # a private address (192, 168, 1, 1), byte-reversed
plant tests/fixtures/x/proc/net/tcp \
    "   0: 0100007F:0BB8 $z8:0000$tail4" \
    "   1: 0A0200C0:2328 $z8:0000$tail4" \
    "   2: $z8:1F40 $z8:0000$tail4"
status_is "loopback, documentation and any in a tcp table pass" 0 "$STATUS"
plant tests/fixtures/x/proc/net/tcp "   0: $lan4:0016 $z8:0000$tail4"
status_is "a private address in a tcp table is flagged" 1 "$STATUS"
like "and named" "$OUTPUT" "address in a /proc/net table"
like "and decoded" "$OUTPUT" "192${d}168${d}1${d}1"
plant tests/fixtures/x/proc/net/udp "   0: 0100007F:0035 $lan4:0035$tail4"
status_is "a private remote address in a udp table is flagged" 1 "$STATUS"
plant tests/fixtures/x/proc/net/tcp6 \
    "   0: $z32:1435 $z32:0000$tail6" \
    "   1: $z8$z8${z8}01000000:0277 $z32:0000$tail6" \
    "   2: B80D0120$z8${z8}01000000:0016 $z32:0000$tail6" \
    "   3: $z8${z8}FFFF00000100007F:0016 $z32:0000$tail6"
status_is "any, loopback, documentation and mapped loopback in tcp6 pass" 0 "$STATUS"
plant tests/fixtures/x/proc/net/tcp6 "   0: F804012A$z8${z8}01000000:0016 $z32:0000$tail6"
status_is "a global address in a tcp6 table is flagged" 1 "$STATUS"
plant tests/fixtures/x/proc/net/udp6 "   0: $z8${z8}FFFF0000$lan4:0035 $z32:0000$tail6"
status_is "a mapped private address in a udp6 table is flagged" 1 "$STATUS"
# A table row in history, removed since, is still found.
mkdir -p "$repo/tests/fixtures/x/proc/net"
printf '   0: %s:0016 %s:0000%s\n' "$lan4" "$z8" "$tail4" >"$repo/tests/fixtures/x/proc/net/tcp"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm table
rm -rf "$repo/tests"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm gone
capture "$repo/tools/secrets.sh"
status_is "a private address in a removed table is found in history" 1 "$STATUS"
rm -rf "$repo/.git"
git -C "$repo" init -q -b main

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
# A removed "-- " line directly before an added "++ " line: the diff shows
# "--- old" then "+++ <host>", which must not be taken for a file header.
printf -- '-- old\n' >"$repo/sample.txt"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm dashes
printf '++ %s\n' "$private_host" >"$repo/sample.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam swap
printf 'gone\n' >"$repo/sample.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam gone
capture "$repo/tools/secrets.sh"
status_is "an added ++ line after a removed -- line is scanned" 1 "$STATUS"
rm -rf "$repo/.git"
git -C "$repo" init -q -b main

# The allow list is scanned like any other file.
printf 'host %s\n' "$private_host" >>"$repo/tools/secrets-allow.txt"
scan 'clean'
status_is "an address hidden in the allow list is found" 1 "$STATUS"
printf '# none\n' >"$repo/tools/secrets-allow.txt"

# A local deny list whose last line has no newline still counts.
printf 'examplebox' >"$SANDBOX/deny"
export MIRQAB_SECRETS_DENY="$SANDBOX/deny"
scan 'connect to examplebox'
status_is "a denied string without a trailing newline is caught" 1 "$STATUS"
like "and named" "$OUTPUT" "denied string"
# A denied name matches as a whole word: inside a longer word it is not that
# host, beside a dot or a dash it still is (plan 024 L20).
scan 'the exampleboxes and myexamplebox'
status_is "a denied name inside a longer word passes" 0 "$STATUS"
scan 'ssh examplebox.local'
status_is "a denied name before a dot is caught" 1 "$STATUS"
scan 'host examplebox-2'
status_is "a denied name before a dash is caught" 1 "$STATUS"
unset MIRQAB_SECRETS_DENY

# The tracked deny list holds only comments: every scan skips it, so a name
# written there would be published unseen (plan 024 L19).
cp "$repo/tools/secrets-deny.txt" "$SANDBOX/deny.tracked"
printf 'examplebox\n' >>"$repo/tools/secrets-deny.txt"
scan 'clean'
status_is "a value in the tracked deny list is found" 1 "$STATUS"
like "and named" "$OUTPUT" "tracked tools/secrets-deny.txt"
# Removed again, it is still in history.
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qm deny
cp "$SANDBOX/deny.tracked" "$repo/tools/secrets-deny.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam undeny
capture "$repo/tools/secrets.sh"
status_is "a value once in the tracked deny list is found in history" 1 "$STATUS"
like "and named" "$OUTPUT" "tracked tools/secrets-deny.txt"
rm -rf "$repo/.git"
git -C "$repo" init -q -b main

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

# Commit messages are scanned too: a squash merge copies a branch's messages
# into main's (audit of the gates). The diff of these commits is clean.
rm -rf "$repo/.git"
git -C "$repo" init -q -b main
printf '%s\n' 'checked on a machine' '' "host $private_host" "mac $mac" >"$SANDBOX/msg"
printf '# none\n' >"$repo/tools/secrets-allow.txt"
printf 'clean\n' >"$repo/sample.txt"
git -C "$repo" add -A
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -q -F "$SANDBOX/msg"
capture "$repo/tools/secrets.sh"
status_is "an address and a MAC in a commit message are found" 1 "$STATUS"
like "and the address is named" "$OUTPUT" "IPv4 address outside the documentation ranges"
like "and the MAC" "$OUTPUT" "MAC address"
printf 'examplebox' >"$SANDBOX/deny"
export MIRQAB_SECRETS_DENY="$SANDBOX/deny"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m 'seen on examplebox'
capture "$repo/tools/secrets.sh"
like "a denied string in a commit message is found" "$OUTPUT" "denied string"
unset MIRQAB_SECRETS_DENY
# The commit is allowed: its message is skipped like its diff.
msg_leak="$(git -C "$repo" rev-parse HEAD~1)"
git -C "$repo" reset -q --hard "$msg_leak"
printf '%s  # synthetic, reviewed\n' "$msg_leak" >"$repo/tools/secrets-allow.txt"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -qam allow
capture "$repo/tools/secrets.sh"
status_is "an allowed commit's message is skipped" 0 "$STATUS"
# An identity trailer names a committer's own address, which the commit's
# metadata publishes anyway: it passes, and the rest of its message is read.
rm -rf "$repo/.git"
git -C "$repo" init -q -b main
printf '# none\n' >"$repo/tools/secrets-allow.txt"
git -C "$repo" add -A
gm=gmail
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -q -m 'squash' \
    -m "Co-authored-by: someone <someone@$gm.com>" -m "Signed-off-by: someone <someone@$gm.com>"
capture "$repo/tools/secrets.sh"
status_is "identity trailers in a message pass" 0 "$STATUS"
git -C "$repo" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m 'squash' \
    -m "write to someone@$gm.com" -m "Co-authored-by: someone <someone@$gm.com>"
capture "$repo/tools/secrets.sh"
status_is "an address in the body beside a trailer is found" 1 "$STATUS"
like "and named" "$OUTPUT" "personal e-mail address"
