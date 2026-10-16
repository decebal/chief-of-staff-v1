//! The two structured outputs. Each struct has its JSON Schema beside it; the
//! schema is what `--json-schema` enforces, the struct is what Rust reads back.
//! `tests/reports.rs` keeps the pair in step.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// New fields carry `#[serde(default)]` so runs saved before they existed
// still load; the schemas require them on every new run.

#[derive(Debug, Serialize, Deserialize)]
pub struct Brief {
    pub headline: String,
    pub calendar: Vec<Slot>,
    pub must_do: Vec<Action>,
    pub reply_to: Vec<Mail>,
    pub blockers: Vec<Blocker>,
    pub can_drop: Vec<String>,
    pub gaps: Vec<String>,
    #[serde(default)]
    pub celebrations: Vec<Win>,
    #[serde(default)]
    pub upcoming: Vec<Milestone>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Slot {
    /// `HH:MM`, 24h, local; empty for all-day or another day.
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub end: String,
    pub what: String,
    pub prep: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Action {
    pub item: String,
    pub why: String,
    pub source: String,
    /// A self-contained prompt an agent can execute for this action.
    #[serde(default)]
    pub agent_prompt: String,
}

/// Something done worth marking, tied to why it mattered.
#[derive(Debug, Serialize, Deserialize)]
pub struct Win {
    pub win: String,
    pub why_it_matters: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Milestone {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub what: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Mail {
    pub from: String,
    pub subject: String,
    pub ask: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Blocker {
    pub item: String,
    pub waiting_on: String,
    pub unblock: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Review {
    pub summary: String,
    pub bets: Vec<Bet>,
    pub drift: Vec<String>,
    pub decisions: Vec<Decision>,
    pub stop_doing: Vec<String>,
    pub gaps: Vec<String>,
    #[serde(default)]
    pub celebrations: Vec<Win>,
    #[serde(default)]
    pub upcoming: Vec<Milestone>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bet {
    pub bet: String,
    pub confidence: Confidence,
    pub evidence: String,
    pub next: String,
    #[serde(default)]
    pub agent_prompt: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Rock,
    Sand,
    Water,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Decision {
    pub decision: String,
    pub options: Vec<String>,
    pub recommendation: String,
}

fn obj(props: Value) -> Value {
    let required: Vec<_> = props
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn list(item: Value) -> Value {
    json!({ "type": "array", "items": item })
}

fn str_() -> Value {
    json!({ "type": "string" })
}

fn wins() -> Value {
    list(obj(
        json!({ "win": str_(), "why_it_matters": str_(), "source": str_() }),
    ))
}

fn milestones() -> Value {
    list(obj(
        json!({ "date": str_(), "what": str_(), "source": str_() }),
    ))
}

pub fn brief_schema() -> Value {
    obj(json!({
        "headline": str_(),
        "calendar": list(obj(json!({ "start": str_(), "end": str_(), "what": str_(), "prep": str_() }))),
        "must_do": list(obj(json!({ "item": str_(), "why": str_(), "source": str_(), "agent_prompt": str_() }))),
        "reply_to": list(obj(json!({ "from": str_(), "subject": str_(), "ask": str_() }))),
        "blockers": list(obj(json!({ "item": str_(), "waiting_on": str_(), "unblock": str_() }))),
        "can_drop": list(str_()),
        "gaps": list(str_()),
        "celebrations": wins(),
        "upcoming": milestones(),
    }))
}

pub fn review_schema() -> Value {
    obj(json!({
        "summary": str_(),
        "bets": list(obj(json!({
            "bet": str_(),
            "confidence": { "type": "string", "enum": ["rock", "sand", "water"] },
            "evidence": str_(),
            "next": str_(),
            "agent_prompt": str_(),
        }))),
        "drift": list(str_()),
        "decisions": list(obj(json!({ "decision": str_(), "options": list(str_()), "recommendation": str_() }))),
        "stop_doing": list(str_()),
        "gaps": list(str_()),
        "celebrations": wins(),
        "upcoming": milestones(),
    }))
}

/// The chief's routing decision for `cos ask`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Route {
    pub brains: Vec<BrainName>,
    /// What each brain should answer, restated so it stands alone.
    pub framing: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrainName {
    Tactician,
    Strategist,
}

impl BrainName {
    pub fn agent(self) -> &'static str {
        match self {
            Self::Tactician => "tactician",
            Self::Strategist => "strategist",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Self::Tactician => "sonnet",
            Self::Strategist => "opus",
        }
    }
}

pub fn route_schema() -> Value {
    obj(json!({
        "brains": {
            "type": "array",
            "items": { "type": "string", "enum": ["tactician", "strategist"] },
            "minItems": 1,
            "maxItems": 2,
            "uniqueItems": true,
        },
        "framing": str_(),
    }))
}

impl Brief {
    pub fn to_markdown(&self, day: &str) -> String {
        let mut s = format!("# Brief — {day}\n\n{}\n", self.headline);
        section(&mut s, "Calendar", &self.calendar, |x| {
            format!("**{}–{}** {} — prep: {}", x.start, x.end, x.what, x.prep)
        });
        section(&mut s, "Must do", &self.must_do, |x| {
            format!("{} — {} _({})_", x.item, x.why, x.source)
        });
        section(&mut s, "Reply to", &self.reply_to, |x| {
            format!("{}: {} — {}", x.from, x.subject, x.ask)
        });
        section(&mut s, "Blockers", &self.blockers, |x| {
            format!(
                "{} — waiting on {}; unblock: {}",
                x.item, x.waiting_on, x.unblock
            )
        });
        section(&mut s, "Can drop", &self.can_drop, String::clone);
        section(&mut s, "Wins", &self.celebrations, |w| {
            format!("{} — {}", w.win, w.why_it_matters)
        });
        section(&mut s, "Coming up", &self.upcoming, |m| {
            format!("{} {}", m.date, m.what)
        });
        section(&mut s, "Gaps (not read)", &self.gaps, String::clone);
        s
    }
}

impl Review {
    pub fn to_markdown(&self, day: &str) -> String {
        let mut s = format!("# Weekly review — {day}\n\n{}\n", self.summary);
        section(&mut s, "Bets", &self.bets, |b| {
            format!(
                "`{}` **{}** — {} → next: {}",
                b.confidence, b.bet, b.evidence, b.next
            )
        });
        section(&mut s, "Drift", &self.drift, String::clone);
        section(&mut s, "Decisions needed", &self.decisions, |d| {
            format!(
                "{} — options: {} → recommend: {}",
                d.decision,
                d.options.join(" / "),
                d.recommendation
            )
        });
        section(&mut s, "Stop doing", &self.stop_doing, String::clone);
        section(&mut s, "Wins", &self.celebrations, |w| {
            format!("{} — {}", w.win, w.why_it_matters)
        });
        section(&mut s, "Coming up", &self.upcoming, |m| {
            format!("{} {}", m.date, m.what)
        });
        section(&mut s, "Gaps (not read)", &self.gaps, String::clone);
        s
    }
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rock => "rock",
            Self::Sand => "sand",
            Self::Water => "water",
        })
    }
}

fn section<T>(s: &mut String, title: &str, items: &[T], line: impl Fn(&T) -> String) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n## {title}\n");
    for it in items {
        let _ = writeln!(s, "- {}", line(it));
    }
}
