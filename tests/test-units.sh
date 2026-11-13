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
# The sandbox's runtime dir is short (socket paths in units may not pass 108
# bytes) and is removed with the sandbox.
run="$XDG_RUNTIME_DIR"
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

like "the socket is private" "$(<"$REPO/dist/mirqab.socket")" "SocketMode=0600"
like "the daemon tells systemd when it is ready" "$(<"$REPO/dist/mirqab.service")" "Type=notify"
like "the window never restarts itself" "$(<"$REPO/dist/mirqab-ui.service")" "Restart=no"

# The launcher entry (plan 001 step 30): valid, opens a window (a launcher has
# no anchor for a popover), and names the application id.
if command -v desktop-file-validate >/dev/null 2>&1; then
    capture desktop-file-validate "$REPO/dist/mirqab.desktop"
    status_is "mirqab.desktop validates" 0 "$STATUS"
    is "and reports nothing" "" "$OUTPUT"
else
    not_ok "desktop-file-validate is installed (desktop-file-utils)"
fi
desktop="$(<"$REPO/dist/mirqab.desktop")"
like "the launcher opens a window" "$desktop" "Exec=mirqab open --window"
like "its window class is the application id" "$desktop" "StartupWMClass=io.github.t1nk333r.mirqab"
like "its icon is the application id" "$desktop" "Icon=io.github.t1nk333r.mirqab"
like "the unit starts the window hidden" "$(<"$REPO/dist/mirqab-ui.service")" "mirqab-ui --background"
