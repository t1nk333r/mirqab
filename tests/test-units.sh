# tests/test-units.sh — the systemd user units under dist/ pass
# `systemd-analyze verify --user`, with ExecStart pointed at the built
# binaries; and a unit with a broken ExecStart fails it; and the tray icon.
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
like "its icon is the two-tone mirqab icon" "$desktop" "Icon=mirqab"
like "the unit starts the window hidden" "$(<"$REPO/dist/mirqab-ui.service")" "mirqab-ui --background"

# The symbolic icon (plan 004) stays a 16-pixel SVG for GTK.
like "the symbolic icon is a 16-pixel SVG" "$(<"$REPO/dist/icons/mirqab-symbolic.svg")" 'viewBox="0 0 16 16"'

# The tray's icon (plan 021): two tones in plain SVG, which every renderer
# draws alike, named as the tray item asks for it.
icon="$(<"$REPO/dist/icons/mirqab.svg")"
like "the tray icon is a 16-pixel SVG" "$icon" 'viewBox="0 0 16 16"'
is "it has exactly two fill colours" 2 "$(grep -o 'fill="#[0-9a-fA-F]*"' <<<"$icon" | sort -u | wc -l)"
is "it uses no feature renderers draw differently" 0 \
    "$(grep -c -E 'paint-order|filter|mask|clipPath|Gradient|<style|currentColor|stroke' <<<"$icon")"
is "the tray asks for that name" 1 "$(grep -c 'TRAY_ICON: &str = "mirqab";' "$REPO/crates/mirqab-proto/src/strings.rs")"
