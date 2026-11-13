# Plan 008: Agents tab

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH for privacy, MEDIUM otherwise. This tab reads other
  tools' private files (transcripts, session state, optionally a
  credential). It must read as little as possible, write nothing, and
  never show or store a secret. File formats belong to other vendors and
  change without notice.
- **Depends on**: 001, 005, 006, 007
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Coding agents start dev servers, watchers and test runners, and leave them
running when the session ends. Stat Space's Agents tab lists the sessions
of each agent, what each is running, which one is waiting for the user,
what was left behind, and the usage limits
(https://statspace.app/features/agents, read 2026-11-13). The owner named
the agents: Claude Code, Codex, Cursor, Aider, Gemini CLI and opencode.

## Linux data sources

No root anywhere. Everything is the user's own files and processes.

| Data | Source |
|------|--------|
| Running agent processes | Plan 006's process cache: executable or command name `claude`, `codex`, `cursor-agent`, `gemini`, `opencode`, `aider`. The first five are installed on the development machine; `aider` and the Cursor editor are not **[machine]** |
| Children of a session | The process tree, plus marker variable **names** in `/proc/<pid>/environ`. Claude Code sets `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_ENTRYPOINT`, `AI_AGENT` in its children **[machine]**. Other agents' markers **[unverified]** |
| Claude Code session state | `~/.claude/sessions/<pid>.json`: keys `pid`, `procStart`, `sessionId`, `cwd`, `name`, `kind`, `entrypoint`, `status`, `statusUpdatedAt`, `startedAt`, `updatedAt`, `version` **[machine]** (18 files; a sample had `"status":"idle"`, `"kind":"interactive"`). The file also has a socket path field, which this app never reads or uses |
| Claude Code transcripts | `~/.claude/projects/<slug>/<session>.jsonl` **[machine]** |
| Codex sessions | `~/.codex/sessions/**/rollout-*.jsonl`, `~/.codex/session_index.jsonl` **[machine]** |
| Codex rate limits | A `"rate_limits"` object inside rollout lines, with keys `limit_id`, `primary`, `secondary`, `credits`, `plan_type` **[machine]** (all null in the sample read) |
| Gemini CLI | `~/.gemini/` (`projects.json`) **[machine]**; session layout **[unverified]** |
| opencode | `~/.local/share/opencode/opencode.db` (SQLite), `~/.config/opencode/` **[machine]**; schema **[unverified]** |
| Aider | `.aider.chat.history.md` and `.aider.input.history` in the project directory (from Aider's documentation, https://aider.chat/docs/config/options.html) **[unverified]** |
| Cursor | `cursor-agent` processes; the editor's agent state is in its own storage and is not read. Process detection only |
| Claude usage limits (opt-in) | `GET https://api.anthropic.com/api/oauth/usage` with `Authorization: Bearer <token>` and `anthropic-beta: oauth-2025-04-20`; token at `~/.claude/.credentials.json` → `claudeAiOauth.accessToken`; reply has `five_hour` and `seven_day`. Evidence: haseen `share/haseen/omarchy/bin/omarchy-agent-usage-claude` lines 79, 637, 769, 807–808. The endpoint is not a documented public API |
| Idle | A process's CPU time delta over 10 minutes |

## Not possible on Linux

Nothing is impossible; three things are limited:

| Item | Limit |
|------|-------|
| "Needs you" for every agent | Only where the agent writes its state. Claude Code: the `status` field. Others: inferred from the transcript's last record when the format allows, otherwise absent |
| Usage limits for every agent | Claude (endpoint, opt-in) and Codex (local logs). Others: absent |
| Cursor sessions | Processes only |

## Design

### Privacy rules (these outrank everything else in this plan)

1. **Read-only.** Nothing under an agent's directory is ever created,
   modified, locked or deleted. SQLite files are opened with
   `mode=ro&immutable=0` and a busy timeout; if that fails, the adapter
   reports "unavailable".
2. **Transcripts are read only while the Agents page is visible**, only
   for sessions on screen, incrementally from the last offset, and only to
   extract: the session title or first user line (truncated to 80
   characters), the last activity time, the model name, token counts when
   present, and whether the last record is a question to the user. Message
   bodies are not kept in memory beyond that, never written to history,
   never sent anywhere and never returned by the CLI or MCP.
3. **Credentials.** Usage limits are built, opt-in and read-only (owner,
   2026-11-13). They are off by default
   (`config.agents.usage = false`). When on, they use the token the agent
   itself stored: it is read into memory for one request, passed to
   `curl` on stdin (`--config -`), and dropped. It is never copied to
   another file, never refreshed, rewritten, logged, stored or shown. If the token
   has expired the app shows "sign in with the agent to see limits" and
   does nothing else. This app never changes how any agent authenticates.
4. **Environments** are scanned for variable names from a fixed list. No
   value is kept except a session id used as a join key, held in memory.
5. Fixtures are synthetic. No file from a real `~/.claude`, `~/.codex` or
   similar is ever committed.

### Adapters

One module per agent behind a trait:

```rust
trait AgentAdapter {
    fn id(&self) -> &'static str;                       // "claude", "codex", …
    fn is_agent_process(&self, p: &Proc) -> bool;
    fn env_markers(&self) -> &'static [&'static str];   // names only
    fn sessions(&mut self, root: &Root, home: &Path, procs: &ProcTable) -> Vec<Session>;
    fn details(&mut self, root: &Root, home: &Path, s: &Session) -> Option<Details>; // transcript-derived
    fn usage(&mut self, …) -> Option<Usage>;            // opt-in
}
```

An adapter whose files are missing or unparseable returns what it can from
processes alone. A parse failure is counted in `status`, never fatal.

### Sessions, leftovers, orphans

- A **session** is a running agent process (and its state file when there
  is one), or a recently ended one (state or transcript modified in the
  last 24 h).
- A session's **processes** are its descendants, plus any process carrying
  its session marker.
- **State**: `working` (CPU in the last 10 s, or the agent says so),
  `needs-you` (the agent's status says it is waiting, or the transcript
  ends on a question or a permission request), `idle`, `ended`.
- **Leftovers**: processes that belonged to a session that has ended, or
  that belong to a live session but have used no CPU for 10 minutes and
  hold a listening port or a file watcher. Shown with the port (plan 007),
  memory and age.
- **Orphans**: developer tools (a fixed list: `node`, `bun`, `deno`,
  `vite`, `esbuild`, `tsc`, `webpack`, `next-server`, `cargo`, `rustc`,
  `rust-analyzer`, `gopls`, `pytest`, `jest`, `vitest`, `playwright`,
  `chromium --headless`, `tsserver`) owned by the user, re-parented to the
  user manager, whose session leader is gone and which carry no agent
  marker. This is a guess, and the page says so: "probably left behind".

### Stream `agents`

Process-level facts every 5 s while subscribed; state files are watched
with inotify (no polling); transcripts are tailed every 5 s only while the
page is visible (`params.details: true`, which only the page sets).

```json
{"sessions":[{"id":"claude:…","agent":"claude","name":"fix login","state":"needs-you",
              "cwd":"~/code/acme-web","project":"acme-web","startedAt":0,"lastActiveAt":0,
              "model":"…","tokens":{"in":0,"out":0},
              "procs":[{"pid":4242,"name":"next dev","rssBytes":0,"cpu":0.0,"port":3000}],
              "rssBytes":0}],
 "leftovers":[{"pid":4300,"name":"vite","session":"claude:…","endedAgoSec":360,"port":5173,"rssBytes":0}],
 "orphans":[{"pid":5100,"name":"tsserver","ageSec":86400,"rssBytes":0}],
 "needsYou":1,
 "usage":{"claude":{"fiveHour":{"percent":42,"resetsAt":0},"sevenDay":{"percent":10,"resetsAt":0},"at":0}}}
```

With no subscriber and a bar item or badge enabled, a light sweep
(processes and state files only) runs every 30 s. With neither, nothing
runs.

### Actions

- `agents.cleanup {session}`: SIGTERM to that session's leftovers.
- `agents.killOrphans {pids?}`: SIGTERM to listed orphans.
- `agents.stop {pid}`: one process.

All need `confirm`, list every pid with its name, port and age, and obey
architecture §5.4. A live agent process itself is never in a cleanup list.

### Page

- A summary line: sessions, how many need the user, leftovers' memory.
- One card per session: agent glyph and name, title, project, state pill,
  age, model and tokens when known; a list of its processes; "Clean up"
  when it has leftovers.
- "Left behind" and "Probably orphaned" groups, each with a button that
  opens the confirm dialog.
- A usage card when enabled: one bar per limit window with its reset time,
  refreshed at most every 5 minutes while the page is visible, with the
  time of the last reading.
- Sidebar badge: the `needsYou` count. A notification on a session entering
  `needs-you` is off by default (`config.agents.notify`).
- Pinnable card `agents.sessions`.

### CLI and MCP

`mirqab agents [--json]`, `mirqab cleanup <session> [-y] [--dry-run]`,
`mirqab orphans [--kill] [-y] [--dry-run]`. MCP: `agent_sessions`,
`orphans` (read), `cleanup_agent_leftovers` (acts, plan 005's flow; the
calling agent's own session and ancestry are excluded). Bar item `agents`.

Session events (`agent-start`, `agent-end`, with agent and project name
only) go to the `event` table for Trends.

## Rejected

- **Parsing transcripts in the background.** Private data read while
  nobody is looking, and CPU for nothing.
- **Refreshing an expired token.** That would change the agent's
  credentials on disk.
- **Talking to an agent's own control socket.** Not this app's interface
  to use.
- **Killing leftovers automatically after a timeout.** A long build looks
  like a leftover.
- **One generic "AI process" heuristic instead of adapters.** The formats
  differ too much, and a wrong guess kills the wrong thing.
- **Usage through a vendor's billing API with a pasted key.** A stored
  secret in this app's config.

## Tests and fixtures

`tests/fixtures/home-agents/` (a fake home, all synthetic):
`.claude/sessions/<pid>.json` in each state, `.claude/projects/…/*.jsonl`
with five short invented records, `.codex/sessions/…/rollout-*.jsonl` with
a filled `rate_limits`, a tiny `opencode.db` built by a script from SQL in
the repository, a `.gemini/` tree, an `.aider.chat.history.md`. Processes
added to `procs.tsv`: two Claude sessions (one ended, with a surviving
`vite`), a Codex session, orphan `tsserver`, and a long-running `cargo`
that must **not** be classed as a leftover. A stub `curl` returning a
usage reply.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/agents.rs` | Each adapter's sessions from fixtures; states; a missing or corrupt file degrades to process-only |
| same | Leftover and orphan classification, including the negative cases; a live agent process is never a cleanup target |
| same | **Privacy**: with `details` false no transcript file is opened (the `Root` test double records opens); the serialised stream contains no transcript text beyond the 80-character title; no environ value appears; the `curl` argv has no token (it is on stdin) and the log has none |
| same | Read-only: after a full run the fixture tree's checksums are unchanged |
| same | Usage off by default makes no request; on, at most one per 5 minutes; an expired token makes no second request |
| `tests/test-ui-agents.sh` (`mirqab-ui --selftest agents`, plan 001) | Cards, the badge count, the cleanup dialog |
| `tests/test-cli.sh` | `agents`, `cleanup --dry-run`, `orphans` |

## Acceptance

1. A running Claude Code session and a Codex session each show with
   project, state and their child processes.
2. Ending a session that started a dev server moves that server to "Left
   behind" within a minute, and Clean up ends it after confirmation.
3. With usage off, `strace -f -e trace=openat` on the daemon (an owner
   check, by hand) shows no open of any credentials file.
4. The privacy and read-only tests pass.
5. With the tab closed and no badge, no agent file is read
   (`mirqab status` shows no `agents` subscriber).

## Checklist

1. Resolve every Unverified item by reading each agent's own
   documentation and a **scrubbed description** of its files (keys and
   types only). Write the findings into this plan. Drop an adapter to
   "processes only" where the format cannot be confirmed.
2. `crates/mirqab-core/src/samplers/agents/{mod,adapter,classify}.rs`.
3. `…/agents/adapters/{claude,codex,gemini,opencode,aider,cursor}.rs`.
4. `…/agents/{usage,actions}.rs`; `exec.rs` gains the stdin-config `curl`
   call if plan 017 has not added it yet.
5. `tests/golden/agents.json`; the fixtures and tests, privacy tests first.
6. CLI and MCP; bar item; sidebar badge; tray attention.
7. `crates/mirqab-view/src/tabs/agents.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/agents/{mod,page,session_card,leftover_list,usage_card,sessions_card}.rs`.
8. Settings: the usage switch with the sentence explaining exactly what it
   reads and sends; the notification switch.
9. Append `agents.sessions` to the Home seed list.

## Unverified

- Marker variables for Codex, Gemini CLI, opencode, Aider and
  `cursor-agent`: start each, then read the variable **names** of a child
  process. Record names only.
- The values Claude Code writes in `status` besides `idle`, and which one
  means "waiting for the user".
- Codex rollout record types and the `rate_limits` fields when filled
  (`primary.used_percent`, `resets_at` or similar).
- Gemini CLI and opencode session storage; Aider's history file names.
- Whether the Claude usage endpoint and header are still accepted; it is
  undocumented and may change. If it fails, the usage card says so and the
  feature stays off.
