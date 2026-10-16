//! `context/` inlined into a brain's prompt. Under `cos` the brains have no
//! file tools at all: they read mail and tickets written by other people, and
//! a brain that can open any file can be talked into quoting `~/.ssh` into a
//! brief that is then served over MCP.

use std::fmt::Write as _;
use std::path::Path;

/// Inlined context stops growing past this; the rest is named, not read.
pub const LIMIT: usize = 64 * 1024;

const FILES: &[&str] = &["profile.md", "goals.md", "principles.md"];

/// Every context file as a markdown section. Decisions come newest first, so
/// a truncated block drops the oldest.
pub fn block(home: &Path) -> String {
    let dir = home.join("context");
    let mut s = String::from(
        "Context files, inlined by cos (you have no file tools here; this is all of context/):\n",
    );
    for name in FILES {
        let body = std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|_| "(missing — say so in gaps)".into());
        let _ = write!(s, "\n### context/{name}\n\n{}\n", body.trim());
    }

    let mut decisions: Vec<_> = std::fs::read_dir(dir.join("decisions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    decisions.sort_unstable_by(|a, b| b.cmp(a));

    let mut skipped = Vec::new();
    for name in decisions {
        let Ok(body) = std::fs::read_to_string(dir.join("decisions").join(&name)) else {
            continue;
        };
        let section = format!("\n### context/decisions/{name}\n\n{}\n", body.trim());
        if s.len() + section.len() > LIMIT {
            skipped.push(name);
        } else {
            s.push_str(&section);
        }
    }
    if !skipped.is_empty() {
        let _ = write!(
            s,
            "\n(Not inlined, over the {} KiB context limit: {}. Name them in gaps.)\n",
            LIMIT / 1024,
            skipped.join(", ")
        );
    }
    s
}

/// MCP servers denied to every brain, from `context/deny-mcp.txt`: one tool
/// prefix per line, `#` comments. Denying a server keeps its tool definitions
/// out of the context each run pays to cache.
pub fn denied_servers(home: &Path) -> Vec<String> {
    std::fs::read_to_string(home.join("context/deny-mcp.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}
