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

capture "$BIN/mirqab" mcp-config codex --write -y
status_is "--write -y runs the client's command" 0 "$STATUS"
is "exactly that command" "codex mcp add mirqab -- mirqab mcp" "$(cat "$STUB_LOG")"
like "and shows what the client said" "$OUTPUT" "added mirqab"

capture "$BIN/mirqab" mcp-config cursor --write -y
status_is "a client configured by file is print only" 1 "$STATUS"
