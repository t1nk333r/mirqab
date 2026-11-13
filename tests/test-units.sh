# tests/test-units.sh — the systemd user units under dist/ pass
# `systemd-analyze verify --user`, with ExecStart pointed at the built
# binaries; and a unit with a broken ExecStart fails it.
# mirqab:covers dist/**

sandbox
build_bins

units="$SANDBOX/units"
mkdir -p "$units"
for u in "$REPO"/dist/*.socket "$REPO"/dist/*.service; do
    sed -e "s#/usr/bin/mirqab-ui#$BIN/mirqab-ui#" -e "s#/usr/bin/mirqab #$BIN/mirqab #" "$u" >"$units/${u##*/}"
done
# A short runtime dir: socket paths in units may not pass 108 bytes.
run="$(mktemp -d /tmp/mqu.XXXXXX)"
verify() {
    env -i HOME="$HOME" PATH=/usr/bin:/bin XDG_RUNTIME_DIR="$run" \
        systemd-analyze verify --user --man=no --recursive-errors=no "$@" 2>&1
}
capture verify "$units"/*
status_is "the units verify" 0 "$STATUS"
is "verify reports nothing" "" "$OUTPUT"

sed -i 's#^ExecStart=.*#ExecStart=/nonexistent/mirqab daemon#' "$units/mirqab.service"
capture verify "$units/mirqab.service" "$units/mirqab.socket"
status_is "a broken ExecStart fails verify" 1 "$STATUS"
rm -rf "$run"

like "the socket is private" "$(<"$REPO/dist/mirqab.socket")" "SocketMode=0600"
like "the daemon tells systemd when it is ready" "$(<"$REPO/dist/mirqab.service")" "Type=notify"
like "the window never restarts itself" "$(<"$REPO/dist/mirqab-ui.service")" "Restart=no"
