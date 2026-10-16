//! Headless Claude Code over `--output-format stream-json`: the same transport
//! the official Agent SDKs use, driven from Rust.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::proc;

/// One headless run. Every field maps to a `claude` flag.
pub struct Run<'a> {
    pub agent: &'a str,
    pub model: &'a str,
    pub prompt: String,
    pub json_schema: Option<Value>,
    pub max_budget_usd: f64,
    pub tools: Toolset<'a>,
    pub deadline: Duration,
}

pub enum Toolset<'a> {
    /// File reads plus the connectors, limited to this allowlist.
    Brain(&'a [&'a str]),
    /// No tools and no MCP servers: routing and merging only.
    None,
}

/// Built-in tools a brain may use. `cos` runs `cn` and `gh` itself, so no Bash.
const TOOLS: &str = "Read,Glob,Grep,ToolSearch";

/// Connector servers the brains never need. Denying them keeps their tool
/// definitions out of the context the run pays to cache.
const DENIED: &str = "mcp__claude_ai_Shutterstock,mcp__claude_ai_Looker,\
mcp__claude_ai_Microsoft_Learn,mcp__claude_ai_Google_Sheets,mcp__claude_ai_Google_Slides,\
mcp__claude_ai_Claude_Docs,mcp__prime";

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    System {
        subtype: String,
    },
    Assistant {
        message: Message,
        parent_tool_use_id: Option<String>,
    },
    User {
        message: Message,
    },
    Result(Outcome),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub content: Vec<Block>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        content: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct Outcome {
    pub subtype: String,
    pub is_error: bool,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub structured_output: Option<Value>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub num_turns: u32,
    #[serde(default)]
    pub permission_denials: Vec<Value>,
}

/// Runs `run` with `home` as cwd so the repo's CLAUDE.md and `.claude/agents`
/// load, and appends every tool call to `audit`.
pub fn execute(home: &Path, audit: &Path, run: &Run) -> Result<Outcome> {
    let mut cmd = Command::new("claude");
    cmd.current_dir(home)
        .args(["-p", &run.prompt])
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--agent", run.agent, "--model", run.model])
        // User settings carry ~80k tokens of rules and skills a brief never reads.
        .args(["--setting-sources", "project", "--disable-slash-commands"])
        .args(["--permission-mode", "dontAsk"])
        .args(["--max-budget-usd", &run.max_budget_usd.to_string()])
        .arg("--no-session-persistence");
    match run.tools {
        Toolset::Brain(allowed) => {
            cmd.args(["--tools", TOOLS, "--disallowedTools", DENIED])
                .arg("--allowedTools")
                .arg(allowed.join(","));
        }
        // An empty --tools also drops ToolSearch, which makes every MCP tool
        // load eagerly; --strict-mcp-config with no config removes them instead.
        Toolset::None => {
            cmd.args(["--tools", "", "--strict-mcp-config"]);
        }
    }
    if let Some(schema) = &run.json_schema {
        cmd.args(["--json-schema", &schema.to_string()]);
    }

    let mut log = AuditLog::open(audit, run.agent)?;
    let mut outcome = None;
    let (status, stderr) = proc::stream_lines(&mut cmd, run.deadline, |line| {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            return Ok(true);
        };
        match event {
            Event::Assistant {
                message,
                parent_tool_use_id,
            } => {
                for block in message.content {
                    if let Block::ToolUse { id, name, input } = block {
                        log.tool_use(&id, &name, &input, parent_tool_use_id.as_deref())?;
                    }
                }
                Ok(true)
            }
            Event::User { message } => {
                for block in message.content {
                    if let Block::ToolResult {
                        tool_use_id,
                        is_error: true,
                        content,
                    } = block
                    {
                        log.tool_error(&tool_use_id, &content)?;
                    }
                }
                Ok(true)
            }
            Event::Result(o) => {
                outcome = Some(o);
                Ok(false)
            }
            Event::System { .. } | Event::Other => Ok(true),
        }
    })?;

    let Some(outcome) = outcome else {
        bail!(
            "claude exited ({status}) without a result event: {}",
            stderr.trim()
        );
    };
    log.outcome(&outcome)?;
    if outcome.is_error {
        bail!("{}: {}", outcome.subtype, outcome.errors.join("; "));
    }
    Ok(outcome)
}

struct AuditLog {
    file: std::fs::File,
    agent: String,
}

impl AuditLog {
    fn open(path: &Path, agent: &str) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        Ok(Self {
            file,
            agent: agent.to_owned(),
        })
    }

    fn tool_use(
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        parent: Option<&str>,
    ) -> Result<()> {
        self.write(serde_json::json!({
            "at": jiff::Timestamp::now().to_string(),
            "agent": self.agent,
            "id": id,
            "tool": name,
            "input": input,
            "subagent_of": parent,
        }))
    }

    fn tool_error(&mut self, id: &str, content: &Value) -> Result<()> {
        self.write(serde_json::json!({
            "at": jiff::Timestamp::now().to_string(),
            "agent": self.agent,
            "id": id,
            "error": content,
        }))
    }

    fn outcome(&mut self, o: &Outcome) -> Result<()> {
        self.write(serde_json::json!({
            "at": jiff::Timestamp::now().to_string(),
            "agent": self.agent,
            "result": o.subtype,
            "cost_usd": o.total_cost_usd,
            "turns": o.num_turns,
            "denied": o.permission_denials,
        }))
    }

    fn write(&mut self, v: Value) -> Result<()> {
        writeln!(self.file, "{v}")?;
        Ok(())
    }
}

pub fn audit_path(home: &Path) -> PathBuf {
    let day = jiff::Zoned::now().date();
    home.join("out/audit").join(format!("{day}.jsonl"))
}
