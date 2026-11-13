# tests/test-contrib.sh — the files under contrib/ (plan 004, Tests): the
# haseen widget's manifest parses and names its entry, Widget.qml passes
# qmllint where that tool is installed, and the Waybar snippets are valid
# JSON naming only items `mirqab bar` knows.
# mirqab:covers contrib/**

sandbox
build_bins

M="$REPO/contrib/haseen/mirqab.bar/manifest.json"
is "the manifest parses" true "$(jq -r '.schemaVersion == 1 and .id == "mirqab.bar"' "$M" 2>/dev/null)"
is "its bar-widget entry exists" true "$(test -f "$REPO/contrib/haseen/mirqab.bar/$(jq -r '.entry["bar-widget"]' "$M")" && echo true)"
known="$("$BIN/mirqab" bar --list | awk '{print $1}' | xargs)"
like "its default item is one mirqab knows" " $known " " $(jq -r '.settings.item.default' "$M") "
QMLLINT="$(command -v qmllint || command -v /usr/lib/qt6/bin/qmllint || true)"
if [[ -n $QMLLINT ]]; then
    # haseen's qs.Haseen modules are not here, so import warnings are
    # expected; a syntax error is not.
    capture "$QMLLINT" "$REPO/contrib/haseen/mirqab.bar/Widget.qml"
    unlike "Widget.qml has no syntax error" "$OUTPUT" "[syntax]"
else
    echo "# qmllint is not installed: Widget.qml not linted" >&2
    ok
fi

# Every ```json block of the Waybar README parses, and each exec names a
# known item.
W="$REPO/contrib/waybar/README.md"
json="$(awk '/^```json$/{on=1; next} /^```$/{on=0} on' "$W")"
is "the Waybar snippet is valid JSON" true "$(jq -r 'type == "object"' <<<"$json" 2>/dev/null)"
# The item is the word after `bar`, wherever flags stand.
for item in $(jq -r '.[].exec | split(" ") | .[(index("bar") + 1)]' <<<"$json"); do
    like "the Waybar example's item $item is known" " $known " " $item "
done
# Waybar restarts a continuous exec only with restart-interval, and reads
# text as markup unless escape is set.
is "every --watch module restarts" 0 "$(jq '[.[] | select((.exec | test("--watch")) and (."restart-interval" | type) != "number")] | length' <<<"$json")"
is "every module escapes its text" 0 "$(jq '[.[] | select(.escape != true)] | length' <<<"$json")"
