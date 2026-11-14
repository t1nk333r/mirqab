# tests/test-mcp-config.sh — `mirqab mcp-config` (plan 005, Tests): each
# client's registration prints, cursor's and the generic JSON parse;
# --dry-run runs nothing; --write with no terminal and no -y does nothing
# and exits 3; --write -y runs the client's own command (a recording stub
# here, never the real client); a client configured by file is print only.
# mirqab:covers crates/mirqab/src/{mcp_config,confirm}.rs

sandbox
build_bins
export STUB_LOG="$SANDBOX/stub.log"
for c in claude codex gemini opencode; do
    stub "$c" 'printf "%s %s\n" "$(basename "$0")" "$*" >>"$STUB_LOG"; echo "added mirqab"'
done

for c in claude codex gemini opencode; do
    capture "$BIN/mirqab" mcp-config "$c"
    status_is "mcp-config $c prints" 0 "$STATUS"
    like "it names $c's own command" "$OUTPUT" "$c mcp add"
    like "and the server command" "$OUTPUT" "mirqab mcp"
done
capture "$BIN/mirqab" mcp-config cursor
like "cursor's file" "$OUTPUT" ".cursor/mcp.json"
is "cursor's JSON parses" '["mcp"]' "$(sed -n '/^{/,/^}/p' <<<"$OUTPUT" | jq -c '.mcpServers.mirqab.args')"
capture "$BIN/mirqab" mcp-config json
is "the generic JSON parses" mirqab "$(sed -n '/^{/,/^}/p' <<<"$OUTPUT" | jq -r '.mcpServers.mirqab.command')"
capture "$BIN/mirqab" mcp-config vim
status_is "an unknown client is a usage error" 64 "$STATUS"
capture "$BIN/mirqab" mcp-config
status_is "no client is a usage error" 64 "$STATUS"

capture "$BIN/mirqab" mcp-config claude --write --dry-run
status_is "--dry-run exits 0" 0 "$STATUS"
like "and shows the command" "$OUTPUT" "claude mcp add --scope user mirqab -- mirqab mcp"
is "and runs nothing" "" "$(cat "$STUB_LOG" 2>/dev/null)"

capture "$BIN/mirqab" mcp-config codex --write </dev/null
status_is "--write with no terminal and no -y needs confirmation" 3 "$STATUS"
like "and says why" "$OUTPUT" "no terminal"
is "it ran nothing" "" "$(cat "$STUB_LOG" 2>/dev/null)"

# A terminal to answer in but stdout piped: the question could not be
# seen, so nothing is asked and nothing runs.
code=0
# script runs the command under $SHELL, so bash for pipefail. Its
# input stays open and silent, so without the check the prompt
# would wait on the terminal until the timeout (exit 124, not 3).
SHELL=/bin/bash timeout 10 script -qec "set -o pipefail; '$BIN/mirqab' mcp-config codex --write | cat" /dev/null \
    < <(sleep 12) >/dev/null || code=$?
is "--write with stdout piped needs confirmation" 3 "$code"
is "and ran nothing" "" "$(cat "$STUB_LOG" 2>/dev/null)"

# --write -y runs exactly the client's command, registered for the user.
expect_cmd() {
    : >"$STUB_LOG"
    capture "$BIN/mirqab" mcp-config "$1" --write -y
    status_is "--write -y runs $1's command" 0 "$STATUS"
    is "exactly $1's command" "$2" "$(cat "$STUB_LOG")"
}
expect_cmd codex "codex mcp add mirqab -- mirqab mcp"
like "and shows what the client said" "$OUTPUT" "added mirqab"
expect_cmd claude "claude mcp add --scope user mirqab -- mirqab mcp"
expect_cmd gemini "gemini mcp add -s user mirqab mirqab mcp"
expect_cmd opencode "opencode mcp add --global mirqab -- mirqab mcp"

capture "$BIN/mirqab" mcp-config cursor --write -y
status_is "a client configured by file is print only" 1 "$STATUS"
