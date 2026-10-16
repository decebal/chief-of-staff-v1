# Using `cos`

The full guide; the [README](../README.md) is the overview.

| Brain | Horizon | You get |
|---|---|---|
| **tactician** | today, this week | at most 5 things to do, each sourced; calendar prep; who is blocked on you; what was *not* read |
| **strategist** | this week against the quarter | each goal rated rock / sand / water, drift, decisions only you can take, what to stop |
| **chief-of-staff** | routes | ask a question; it picks one or both brains and merges their answers |

It reads Google Calendar, Gmail, Jira/Confluence and Notion through your
claude.ai connectors, plus GitHub PRs and chronis tasks on your machine. It is
**read-only**: it recommends, you send.

Measured cost per run: a daily brief about $0.30, a two-brain question about
$1.50. Every run carries a spending cap and a 300 s deadline.

## Requirements

- macOS (the scheduler uses launchd; everything else runs anywhere Rust does)
- [Claude Code](https://claude.com/claude-code) CLI, logged in (`claude` on PATH)
- Connectors enabled on your claude.ai account: Google Calendar, Gmail,
  Atlassian, Notion — whichever you use. Check with `/mcp` in Claude Code.
- `gh` logged in; optional `cn` (chronis) for tasks
- Rust (stable), [Task](https://taskfile.dev), `jq`

## Quickstart

```bash
git clone <this repo> ~/Projects/chief-of-staff
cd ~/Projects/chief-of-staff
task setup            # seeds context/ from templates, builds, installs cos + agents
$EDITOR context/profile.md context/goals.md
cos brief             # first brief → out/briefs/YYYY-MM-DD.{md,json}
```

`context/` is gitignored: your profile, goals and decisions stay on your
machine. Templates live in `templates/context/`.

## Use it

```bash
cos brief                                  # tactician
cos review                                 # strategist (weekly)
cos ask "release review or the bug fix today?"
cos --dry-run brief                        # print the prompt, spend nothing
task schedule                              # weekdays 08:30 brief, Fridays 16:00 review
```

### The report

Every `cos brief` and `cos review` writes `out/report.html` and opens it:

- **Wins** and **Coming up** across the top: what landed and why it mattered,
  dated milestones with countdowns, and a merged-PRs-per-day chart.
- **Left brain · Today** (tactician): a day timeline, the must-do list with a
  **copyable agent prompt per action** (paste into Claude Code or Codex; each
  stops before anything outward-facing), review-queue age, ready tasks by repo
  and priority, blockers.
- **Right brain · The quarter** (strategist): bets as rock / sand / water,
  a prompt for each bet's next step, where merged work went, decisions, drift.

- **History** builds up every run: a year of merged PRs as a calendar
  heatmap, trend lines for review queue, ready tasks, must-dos and meeting
  time, a diagram of how it works, and links to every archived daily report.
- **Diagrams** — the roadmap timeline, the goals flywheel and the
  architecture — are [Markdy](https://markdy.com) 1.5.7, vendored as one
  inline bundle (`vendor/markdy/`). Their source is generated from data, not
  written by the model.

Chart numbers come from the facts `cos` collects itself (`out/facts/`,
`out/history/`), not from the model. `cos report` rebuilds and opens it for
free; `--no-open` skips opening.

The archive seeds itself with a year of merged PRs, two months per run, so
GitHub's search rate limit is never hit; months not yet archived show as
outlined cells, not as zero.

### Live

```bash
cos dash            # or: task dash — http://127.0.0.1:8790
```

Serves the same page on localhost and reloads an open tab whenever a run
lands (scheduled ones included). It answers only to `127.0.0.1` / `localhost`
Host headers and serves nothing outside `out/reports/<date>.html`.

### In Claude Desktop and Codex

```bash
task install-mcp      # registers `cos mcp` with both (backs up the Desktop config)
```

Restart Claude Desktop. Both apps then have these tools:

| Tool | Cost | Notes |
|---|---|---|
| `latest_brief`, `latest_review` | free, instant | read the last saved run |
| `brief`, `review` | paid, minutes | fresh run |
| `ask` | paid, minutes | routes and merges |

In Claude Desktop the brief and review tools render a **dashboard** (an
[MCP App](https://modelcontextprotocol.io/docs/extensions/apps)): Today / Week
tabs, reload, and a two-click "run new". For Codex, the paid tools can outlast
the default 60 s tool timeout; add `tool_timeout_sec = 320` under
`[mcp_servers.cos]` in `~/.codex/config.toml`.

In Claude Code, `task setup` also installs the three agents into
`~/.claude/agents/`, so any session can delegate to `tactician` or `strategist`.

### Spin off agents

Every must-do and every bet carries an agent prompt. `cos agents` runs them:

```bash
cos agents spawn --from brief                 # list the items that have a prompt
cos agents spawn --from brief --item 2        # Claude Code on must-do #2
cos agents spawn --from review --item 1 --harness codex
cos agents spawn --from brief --all           # one agent per must-do
cos agents spawn --prompt "…" --repo ~/Projects/my-repo
cos agents watch                              # the table, redrawn until all finish
cos agents tail -f <id>                       # one line per tool call or message
cos agents show <id>                          # summary, cost, how to take it over
cos agents kill <id>
cos agents clean                              # remove finished worktrees (keeps any holding work)
```

- **Where it works**: a detached worktree under `out/agents/<id>/tree`, cut
  from the HEAD of the repo the prompt names (`~/Projects/<repo>`), or
  `--repo`. `--in-place` uses the checkout itself. Without a repo it gets a
  scratch directory.
- **What it may do**: edit and run commands inside that worktree. Shell
  commands have no network (Claude Code's sandbox with a strict allowlist;
  Codex's `workspace-write`), so it cannot push, comment or send; it stops
  and leaves the draft for you, as every agent prompt asks.
- **Limits**: `--deadline` seconds (default 1800, ceiling 7200) and
  `--budget` dollars for Claude (default $5). Codex has no spend flag.
- **Take over**: `cos agents show <id>` prints `claude --resume <session>` or
  `codex resume <session>` for the worktree.
- **From Claude Desktop or Codex**: the `spin_off`, `agents` and `stop_agent`
  tools (local server only; the remote one never offers them).
- **In the browser**: `cos dash`, then `/agents`.

`cos agents clean` refuses a worktree with uncommitted changes or new commits;
copy or commit the work first, or pass `--force`.

### Remote, for claude.ai (Claude Dashboards)

```bash
brew install cloudflared
cos tunnel            # or: task tunnel — prints the connector URL; Ctrl-C stops it
```

`cos tunnel` starts the read-only server and a Cloudflare quick tunnel, and
prints `https://<name>.trycloudflare.com/mcp/<token>`. Add that in claude.ai →
Settings → Connectors → Add custom connector.

- The remote server exposes **only** `latest_brief` and `latest_review`: a
  leaked URL can read your last brief, never trigger a paid run.
- The URL path is the credential (`out/.mcp-token`, mode 600). Treat the URL
  like a password; `task rotate-token` invalidates it.
- Quick-tunnel hostnames change on every restart, so the connector has to be
  re-added after one. A stable hostname needs a named Cloudflare tunnel on
  your own domain.
- `cos` owns the `cloudflared` process: Ctrl-C, SIGTERM or SIGHUP stop both;
  after a SIGKILL the next `cos tunnel` stops the leftover (`out/tunnel.pid`).

Briefs quote your mail and calendar; decide deliberately before exposing them.
`cos serve --public-host <host>` runs the server alone, behind a proxy of your
choosing.

## Customise

- **What the brains may touch**: `READ_ONLY` in `src/lib.rs`. Anything not
  listed is denied. Add individual tools, never a whole server, and never a
  file tool: `context/` reaches the brains inlined in the prompt.
- **Cheaper runs**: list connectors the brains never use in
  `context/deny-mcp.txt`; their tool definitions then stay out of every run's
  cached context.
- **Prompts**: `.claude/agents/*.md`. Re-run `task install` after editing.
- **Models and budgets**: `src/pipeline.rs` (`brain(...)` calls, defaults
  `--budget`).
- **Where your repos live**: `COS_PROJECTS` (default `~/Projects`).

## Troubleshooting

- *"needs to be connected in claude.ai"* in a brief's gaps: reconnect that
  connector with `/mcp` in Claude Code. The brief lists it as not read rather
  than pretending the inbox is empty.
- Every tool call and failure is logged to `out/audit/YYYY-MM-DD.jsonl`.
- Scheduled runs log to `out/logs/`.

Design and measurements: [docs/design.md](docs/design.md).
