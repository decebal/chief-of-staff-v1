# chief-of-staff

A personal chief of staff: a Rust binary, `cos`, that drives headless Claude
Code (`claude -p --output-format stream-json`) with two brains, and serves
them over MCP to Claude Desktop, Codex and claude.ai. User guide: `README.md`.

| Agent | Horizon | Invoked by |
|---|---|---|
| `chief-of-staff` | routes, never answers domain questions | `cos ask "…"` |
| `tactician` | today, this week | `cos brief` |
| `strategist` | this week against the quarter | `cos review` |

Design and measurements: `docs/design.md`. Slack copy for sharing the idea:
`docs/slack-chief-of-staff.html`.

## Context the brains read

All under `context/`, which is gitignored and seeded from `templates/context/`:

- `profile.md` — who the user is, where their work lives.
- `goals.md` — the quarter's bets. The strategist's yardstick.
- `principles.md` — how to choose between bets.
- `decisions/` — decisions already taken, one file each, dated.

The brains are read-only. They recommend; the user edits these files. Never
commit anything from `context/` or `out/`.

## Layout

| Path | What |
|---|---|
| `src/claude.rs` | stream-json driver, audit log |
| `src/proc.rs` | bounded child processes: one deadline, kill the group, reap |
| `src/gather.rs` | `cn` / `gh` facts collected without the model |
| `src/pipeline.rs` | brief, review, ask — shared by CLI and MCP |
| `src/reports.rs` | typed outputs + their JSON Schemas |
| `src/store.rs` | `out/<kind>/<day>.{md,json}` |
| `src/mcp.rs` + `src/dashboard.html` | MCP server (stdio = all tools, HTTP = read-only) and the MCP App view |
| `src/report.rs` + `src/report.html` | the HTML report; `vendor/markdy/` is inlined into it |
| `src/history.rs` | the archive under `out/history/`, backfilled two months per run |
| `src/dash.rs` | `cos dash`, the live localhost page |
| `examples/demo.rs` | the synthetic home behind every screenshot (`task demo`) |

Screenshots in `docs/assets/` come only from `task screenshots` on the demo
home. Never capture a real `COS_HOME`: briefs quote mail, calendar and tickets.

## Commands

```bash
task test               # fmt, clippy -D warnings, tests
task setup              # first run: seed context/, build, install
task install-mcp        # register with Claude Desktop + Codex
task schedule           # launchd: weekday brief, Friday review
cos --dry-run brief     # print the prompt, call nothing
```

Agent files carry `@COS_HOME@`; `task install` substitutes the clone path.
