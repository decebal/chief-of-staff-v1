# Design

Adapted from the Agent SDK cookbook
[The chief of staff agent](https://platform.claude.com/cookbook/claude-agent-sdk-01-the-chief-of-staff-agent),
in Rust.

## Transport

There is no Rust Agent SDK. The Python and TypeScript SDKs spawn the `claude`
CLI and speak `stream-json` over its stdio; `cos` does the same
(`src/claude.rs`). That keeps everything the CLI already has: the claude.ai
connectors (Gmail, Calendar, Atlassian, Notion), `.claude/agents`, and
`--json-schema` structured output, which arrives on the `result` event as
`structured_output`.

A direct-API loop (`rig` + `rmcp`) was rejected: it would mean re-wiring OAuth
for four MCP servers and rebuilding subagents by hand.

| Cookbook piece | Here |
|---|---|
| `CLAUDE.md` company context | `CLAUDE.md` + `context/goals.md`, `principles.md`, `decisions/` |
| `.claude/agents/financial-analyst.md` | `.claude/agents/{tactician,strategist}.md` |
| main agent with "delegate, don't answer" | `.claude/agents/chief-of-staff.md` |
| `scripts/*.py` called through Bash | `src/gather.rs` — `cn` and `gh`, run by `cos`, not by the model |
| `/budget-impact` slash command | `cos brief`, `cos review`, `cos ask` |
| PostToolUse `report-tracker.py` hook | `out/audit/YYYY-MM-DD.jsonl`, written from the event stream |
| `executive` output style | the `Answering` rules in the chief's prompt + typed reports |
| `send_query()` | `claude::execute` |

## `cos ask`: orchestration in Rust

The Agent tool runs subagents in the background, and `claude -p` emits its
result before they report: the first live run returned "both brains are still
running" for $0.85. So `cos ask` orchestrates outside the model:

1. **Route** — the chief, no tools, returns `{brains, framing}` (`--json-schema`).
2. **Brains** — `cos` runs each chosen brain as its own `claude -p --agent …`,
   in parallel threads, sharing one deadline.
3. **Merge** — the chief, no tools, merges under its Answering rules. A failed
   brain arrives as `FAILED: …`, which the chief reports as a gap.

Interactive sessions (agents installed in `~/.claude/agents`) still delegate
in-session; there background agents report back.

## Cost

Every headless run pays to cache its context. Measured on 2026-10-15 for a
one-word reply (Sonnet):

| Config | Cache-write tokens | Cost |
|---|---|---|
| default (user rules, skills, all connectors) | 125,695 | $0.50 |
| `--tools ""` (drops ToolSearch, so MCP tools load eagerly) | 209,597 | $0.84 |
| `--setting-sources project`, unused servers denied, ToolSearch kept | 42,465 | $0.17 |

`cos` uses the last for brains and `--tools "" --strict-mcp-config` for route
and merge. Live runs: `cos brief` $0.32 / 9 turns; `cos ask` (both brains)
$1.45 including the route step. Every run carries `--max-budget-usd`.

## Safety

- Read-only: `--permission-mode dontAsk` plus an explicit allowlist
  (`READ_ONLY` in `src/lib.rs`). Send, draft, trash, Jira edits and file
  writes are denied, not merely discouraged.
- Every child runs in its own process group under one deadline (≤ 300 s);
  on expiry the group is killed and reaped (`src/proc.rs`).
- `out/` and `context/` are gitignored: briefs quote mail and calendar, and
  context holds personal goals.

## MCP: Claude Desktop, Codex, claude.ai

`cos mcp` (stdio) and `cos serve` (streamable HTTP) expose the same server,
built on `rmcp` 3.5 (`src/mcp.rs`).

| Mode | Tools | Why |
|---|---|---|
| stdio (`Local`) | all five | runs as you, on your machine |
| HTTP (`Remote`) | `latest_brief`, `latest_review` only | a leaked URL must not spend money or start runs |

The remote filter sits in both `list_tools` and `call_tool`, so a client that
calls an unlisted tool by name gets `-32602 tool … is local-only`. HTTP is
served at `/mcp/<token>` (≥ 32 chars); every other path is a 404. Verified
2026-10-16 with curl: wrong path 404, two tools listed, `brief` refused.

Saved runs are stored as JSON next to the markdown (`src/store.rs`), so the
free tools return typed `structuredContent`. Brief and review tools carry
`_meta.ui.resourceUri = ui://cos/dashboard`; that resource is
`src/dashboard.html` with mime `text/html;profile=mcp-app`, speaking the MCP
Apps postMessage protocol (2026-01-26) without the JS SDK.

`cos tunnel` (`src/tunnel.rs`) adds a Cloudflare quick tunnel. Verified
2026-10-16 through the public URL: wrong path 404, two tools listed,
`latest_brief` returned the day's brief. `cos` owns `cloudflared` in its own
process group: SIGINT/SIGTERM/SIGHUP kill and reap it (tested: no process
left, pidfile removed); a SIGKILLed run leaves it orphaned to PPID 1, and the
next start stops it via `out/tunnel.pid` (tested).

Claude Desktop, 2026-10-16: `latest_brief` returned text in a task-view chat,
and the server log shows Desktop fetched `ui://cos/dashboard`
(`resources/read`), but no dashboard rendered. Not yet checked in a plain chat.

Not verified: Claude Dashboards over the remote server. Codex Spaces hold
pages, sites and images; MCP servers do not appear there, and no documented
way to populate a Space from a server was found.

## Not built

- An `--act` mode (drafting replies, moving tickets). Add per tool to
  `READ_ONLY`'s sibling list when wanted, never by widening to a whole server.
- Multi-turn sessions (`--resume`). Every run is stateless; memory is the
  `context/` files.
