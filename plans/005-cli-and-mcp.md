# Plan 005: CLI and MCP server

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MEDIUM. The MCP server lets an agent act on the machine. The
  refusals and the confirmation rule must be right before any destructive
  tool exists.
- **Depends on**: 001, 002
- **Category**: integration
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Stat Space ships a CLI and an MCP server so that a terminal or a coding
agent can ask what is running and clean up
(https://statspace.app/features/cli-mcp, read 2026-11-13): commands for
ports, load, apps, sensors, battery, agents, cleanup, orphans, keep-awake
and blockers; `--json` everywhere; exit code 2 when the app is not running;
and twelve MCP tools. Plan 001 built `status`, `open` and `history`. This
plan builds the frame the rest hang on: output conventions, the confirm
flow, the MCP server, and the tools whose data already exists. Each tab's
plan then adds its own commands and tools in its checklist.

## Linux data sources

None of its own: both are clients of the daemon's socket. No root.

| Fact | Source |
|------|--------|
| MCP transport | JSON-RPC 2.0 over stdio, newline-delimited (https://modelcontextprotocol.io/specification, "Transports: stdio") |
| MCP methods used | `initialize`, `notifications/initialized`, `tools/list`, `tools/call`, `ping` (same specification, "Tools") |
| Where clients keep MCP config | Claude Code: `claude mcp add` or `.mcp.json`; Codex: `~/.codex/config.toml` `[mcp_servers.<name>]`; others: printed JSON. **[unverified]**, step 1 |

## Not possible on Linux

Nothing. The command set matches Stat Space's in behaviour; names are ours.

## Design

### CLI conventions

- Exit codes: 0 success; 1 failure; 2 daemon not running; 3 refused or
  needs confirmation in a non-interactive call; 64 usage error.
- `--json` prints exactly the daemon's `result` object, one line. Without
  it, an aligned table. No colour unless stdout is a terminal and
  `NO_COLOR` is unset.
- A mutating command: prints what it will do; asks `[y/N]` when stdin is a
  terminal; with no terminal and no `-y` it exits 3 after printing the
  list; `--dry-run` prints the list and exits 0.
- `--help` on every command comes from one table in `cli/help.rs`, which
  also generates `docs/cli.md` (checked by `tools/check-docs.sh`).
- Shell completion: `mirqab completions bash|zsh|fish` prints a script
  generated from the same table.

Commands in this plan: `status`, `load`, `sensors`, `history`, `open`,
`toggle`, `close`, `bar` (plan 004), `completions`, `--mcp`, `mcp-config`.
Added by later plans: `apps`, `what` (006); `ports`, `port`, `kill` (007);
`agents`, `cleanup`, `orphans` (008); `awake`, `blockers` (009); `battery`
(013); `helper` (015); `why` (018); `report` (019).

### MCP server

`mirqab --mcp` reads requests on stdin, writes responses on stdout, logs to
stderr, and exits when stdin closes. It holds one connection to the daemon.
If the daemon is not running, `initialize` succeeds and every tool returns
an error result saying how to start it, so the agent can tell the user.

Server info: name `mirqab`, the crate version. Capabilities: `tools` only.
No resources, no prompts, no sampling.

Tools (the names match Stat Space's, since an agent may already know them;
the descriptions and schemas are written here from scratch):

| Tool | Reads or acts | Plan |
|------|---------------|------|
| `system_load` | reads `system` | here |
| `explain_slowness` | reads; rules of plan 018; here it returns the load summary and says the rules are not installed yet | here, 018 |
| `top_apps` | reads `apps` | 006 |
| `what_is_process` | reads the inspector | 006 |
| `list_ports`, `whats_on_port` | reads `ports` | 007 |
| `kill_port` | **acts** | 007 |
| `agent_sessions`, `orphans` | reads `agents` | 008 |
| `cleanup_agent_leftovers` | **acts** | 008 |
| `keep_awake` | **acts** (reversible) | 009 |
| `sleep_blockers` | reads `awake` | 009 |

Rules for tools that act:

1. The first call, without `confirm: true`, never acts. It returns the
   `wouldDo` list as text and as structured content, with a sentence
   telling the agent to show it to the user and call again with
   `confirm: true`.
2. With `confirm: true` the daemon re-computes the list and acts only on
   targets that were in the list it returned before (the tool passes back
   an opaque `plan` id; a plan expires after 120 s). A process that
   appeared since is not touched.
3. The refusals of architecture §5.4 apply and cannot be overridden by any
   argument: other users' processes, pid 1, the daemon, the compositor.
4. **An agent cannot kill itself or its ancestors by accident.** The MCP
   server's own process ancestry (walked from its parent pid) is added to
   the refused set for that connection, and a `wouldDo` list that would
   have contained them says so.
5. Every act through MCP is written to the `event` table (`kind:
   "mcp-action"`, the tool, the targets, no arguments beyond those) and
   shown in Support's diagnostics (plan 019).
6. Tool descriptions state the side effect in the first sentence. Read
   tools set `annotations.readOnlyHint: true`; acting tools set
   `destructiveHint: true`.

Tool results are short: a table as text, plus `structuredContent` with the
same data. Process command lines are included (they are what the agent
asked about); process environments never are.

### `mirqab mcp-config <client>`

Prints, and with `--write` applies after showing a diff and asking, the
configuration for `claude`, `codex`, `cursor`, `gemini`, `opencode`, or
`json` (generic). `--write` is the only place this app edits another
program's config; it goes through `act.rs`, has `--dry-run`, and backs the
file up beside itself first. Default is print only.

## Rejected

- **An HTTP or SSE MCP transport.** A listening port in an app whose Ports
  tab flags listening ports; stdio needs none.
- **An MCP SDK crate.** The server needs five methods; an SDK brings an
  async runtime. Hand-written JSON-RPC over `serde_json` is about 300
  lines.
- **Letting `confirm: true` on the first call act.** One hallucinated
  argument would kill processes.
- **Tools for fans, disk cleanup or volume.** Not in the reference set, and
  each widens what an agent can do. Add later only if the owner asks.
- **`clap`** (plan 001).

## Tests and fixtures

- `tests/mcp/*.jsonl`: request scripts and expected responses (golden).

| Test | Checks |
|------|--------|
| `crates/mirqab/tests/mcp.rs` | `initialize` handshake; `tools/list` matches the table for the plans merged so far; unknown tool; malformed JSON gives a JSON-RPC error and the server stays up; daemon absent gives tool errors, not a crash |
| same | Confirm flow against a fixture root and a recording `act`: first call acts on nothing; second call with the plan id acts on exactly the listed targets; an expired or foreign plan id is refused; the caller's ancestry is never in the list |
| `tests/test-cli.sh` | Exit codes 0, 2, 3, 64; `--json` is one line of valid JSON; no ANSI codes when piped; `--dry-run` changes nothing (the stub log is empty) |
| `tests/test-mcp-config.sh` | Printed config for each client parses; `--write --dry-run` touches nothing; `--write` in the sandbox `HOME` makes a backup and a valid file |
| `tools/check-docs.sh` | `docs/cli.md` equals `mirqab help --markdown` |

## Acceptance

1. `mirqab load` and `mirqab load --json` work; with the daemon stopped
   both exit 2 with the hint.
2. An MCP client configured with `mirqab mcp-config claude` lists the
   tools and gets an answer from `system_load`.
3. The confirm-flow tests pass with a recording `act` and no real signal
   is ever sent by a test.
4. `docs/cli.md` is generated and in step.

## Checklist

1. Resolve Unverified; record each client's config location with the
   command or document that proves it.
2. `crates/mirqab/src/cli/{mod,help,table,confirm}.rs`; move plan 001's
   commands onto it; `completions`.
3. `crates/mirqab-core/src/plan.rs`: plan ids, expiry, the "only listed
   targets" check; `act.rs` records to `event`.
4. `crates/mirqab/src/mcp/{mod,rpc,tools}.rs`: tools as a table of name,
   description, input schema, handler; `system_load` and the
   `explain_slowness` stub.
5. `crates/mirqab/src/mcp/guard.rs`: caller ancestry.
6. `crates/mirqab/src/cli/mcp_config.rs`.
7. `docs/cli.md` generation and the docs check.
8. Tests above.

## Unverified

- **MCP protocol revision to declare.** Read the current specification at
  https://modelcontextprotocol.io/specification and declare the newest
  revision the server implements; record it here.
- **Client config locations and formats** for Claude Code, Codex, Cursor,
  Gemini CLI and opencode: check each tool's own documentation or
  `--help`. `claude`, `codex`, `cursor-agent`, `gemini` and `opencode` are
  installed on the development machine **[machine]**; read their config
  shape without copying any value from the owner's files.
- **Tool annotations** (`readOnlyHint`, `destructiveHint`): confirm the
  field names in the specification revision chosen.
