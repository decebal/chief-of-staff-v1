//! The three runs `cos` offers — brief, review, ask — shared by the CLI and the
//! MCP server. Blocking: each spawns `claude` children under one deadline.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;

use crate::READ_ONLY;
use crate::claude::{self, Outcome, Run, Toolset};
use crate::gather::{self, Facts};
use crate::reports::{self, Brief, Review, Route};
use crate::store::{self, Kind};

/// Route, then brains in parallel, then merge: all three inside this ceiling.
const ASK_DEADLINE: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct Config {
    pub home: PathBuf,
    pub projects: PathBuf,
    /// Overrides each run's default spending cap, in USD.
    pub budget: Option<f64>,
}

/// What a run produced, plus what it cost across every `claude` child.
pub struct Done<T> {
    pub value: T,
    pub markdown: String,
    pub cost_usd: f64,
    pub saved: Option<PathBuf>,
}

struct Dates {
    day: String,
    now: String,
    week_ago: String,
}

fn dates() -> Dates {
    let now = jiff::Zoned::now();
    let today = now.date();
    Dates {
        day: today.to_string(),
        now: now.strftime("%H:%M %Z").to_string(),
        week_ago: today.saturating_sub(jiff::Span::new().days(7)).to_string(),
    }
}

fn facts_block(facts: &Facts) -> String {
    format!(
        "Local facts gathered by cos (already read, do not re-fetch):\n\n{}",
        facts.render()
    )
}

/// Everything a brain reads that is not a connector: `context/`, then facts.
fn grounding(home: &std::path::Path, facts: &Facts) -> String {
    format!("{}\n\n{}", crate::context::block(home), facts_block(facts))
}

impl Config {
    /// Collects local facts once and keeps them, so the report charts the same
    /// numbers the brain was shown.
    pub fn gather(&self) -> Result<Facts> {
        let d = dates();
        let facts = gather::collect(&self.projects, &d.week_ago);
        store::save_json(&self.home, Kind::Facts, &d.day, &facts)?;
        Ok(facts)
    }

    pub fn brief_run(&self, facts: &Facts) -> Run<'static> {
        let d = dates();
        let prompt = format!(
            "Today is {}, now {}. Write today's brief. Meetings that have already \
             ended go nowhere but a one-line note; prep only for what is still ahead.\n\n{}",
            d.day,
            d.now,
            grounding(&self.home, facts)
        );
        Run {
            json_schema: Some(reports::brief_schema()),
            ..self.brain("tactician", "sonnet", prompt, self.budget.unwrap_or(2.0))
        }
    }

    pub fn review_run(&self, facts: &Facts) -> Run<'static> {
        let d = dates();
        let prompt = format!(
            "Today is {}. Run the weekly review against context/goals.md.\n\n{}",
            d.day,
            grounding(&self.home, facts)
        );
        Run {
            json_schema: Some(reports::review_schema()),
            ..self.brain("strategist", "opus", prompt, self.budget.unwrap_or(5.0))
        }
    }

    pub fn brief(&self) -> Result<Done<Brief>> {
        let day = dates().day;
        let facts = self.gather()?;
        let (brief, cost): (Brief, f64) = self.structured(&self.brief_run(&facts))?;
        let markdown = brief.to_markdown(&day);
        let saved = store::save(&self.home, Kind::Briefs, &day, &brief, &markdown)?;
        crate::history::record(&self.home, &day, &facts, Some(&brief))?;
        crate::report::build(&self.home)?;
        Ok(Done {
            value: brief,
            markdown,
            cost_usd: cost,
            saved: Some(saved),
        })
    }

    pub fn review(&self) -> Result<Done<Review>> {
        let day = dates().day;
        let facts = self.gather()?;
        let (review, cost): (Review, f64) = self.structured(&self.review_run(&facts))?;
        let markdown = review.to_markdown(&day);
        let saved = store::save(&self.home, Kind::Reviews, &day, &review, &markdown)?;
        crate::history::record(&self.home, &day, &facts, None)?;
        crate::report::build(&self.home)?;
        Ok(Done {
            value: review,
            markdown,
            cost_usd: cost,
            saved: Some(saved),
        })
    }

    pub fn route_run(question: &str) -> Run<'static> {
        Run {
            deadline: Duration::from_secs(60),
            json_schema: Some(reports::route_schema()),
            ..bare(format!(
                "Today is {}. Routing step only — do not answer. Pick the brain(s) for \
                 this question and restate it so each brain can answer it alone.\n\n\
                 Question: {question}",
                dates().day
            ))
        }
    }

    /// Route, run the chosen brains in parallel, merge. `value` is the routing.
    pub fn ask(&self, question: &str) -> Result<Done<Route>> {
        let started = Instant::now();
        let left = || ASK_DEADLINE.saturating_sub(started.elapsed());
        let audit = claude::audit_path(&self.home);
        let d = dates();

        let (route, mut cost): (Route, f64) = self.structured(&Self::route_run(question))?;
        let prompt = format!(
            "Today is {}. {}\n\nOriginal question: {question}\n\n{}",
            d.day,
            route.framing,
            grounding(&self.home, &gather::collect(&self.projects, &d.week_ago))
        );
        let budget = self.budget.unwrap_or(2.0);
        let answers: Vec<(&str, Result<Outcome>)> = thread::scope(|s| {
            let handles: Vec<_> = route
                .brains
                .iter()
                .map(|b| {
                    let run = Run {
                        deadline: left(),
                        ..self.brain(b.agent(), b.model(), prompt.clone(), budget)
                    };
                    let (home, audit) = (&self.home, &audit);
                    (
                        b.agent(),
                        s.spawn(move || claude::execute(home, audit, &run)),
                    )
                })
                .collect();
            handles
                .into_iter()
                .map(|(name, h)| {
                    let r = h
                        .join()
                        .unwrap_or_else(|_| Err(anyhow!("{name} thread panicked")));
                    (name, r)
                })
                .collect()
        });

        let mut merged_input = String::new();
        for (name, r) in &answers {
            let body = match r {
                Ok(o) => {
                    cost += o.total_cost_usd;
                    o.result.clone().unwrap_or_default()
                }
                Err(e) => format!("FAILED: {e:#}"),
            };
            merged_input.push_str(&format!("## {name}\n\n{body}\n\n"));
        }

        let merged = claude::execute(
            &self.home,
            &audit,
            &Run {
                deadline: left(),
                ..bare(format!(
                    "Merge step. Question: {question}\n\n\
                     Answers from the brains (a FAILED entry is a gap, not an empty answer):\n\n\
                     {merged_input}Write the final answer under your Answering rules."
                ))
            },
        )?;
        cost += merged.total_cost_usd;
        Ok(Done {
            value: route,
            markdown: merged.result.unwrap_or_default(),
            cost_usd: cost,
            saved: None,
        })
    }

    fn structured<T: DeserializeOwned>(&self, run: &Run) -> Result<(T, f64)> {
        let out = claude::execute(&self.home, &claude::audit_path(&self.home), run)?;
        let value = out
            .structured_output
            .context("run returned no structured_output")?;
        let parsed =
            serde_json::from_value(value).context("structured_output did not match the schema")?;
        Ok((parsed, out.total_cost_usd))
    }

    fn brain(
        &self,
        agent: &'static str,
        model: &'static str,
        prompt: String,
        budget: f64,
    ) -> Run<'static> {
        Run {
            agent,
            model,
            prompt,
            json_schema: None,
            max_budget_usd: budget,
            tools: Toolset::Brain(READ_ONLY),
            deadline: Duration::from_secs(300),
            denied: crate::context::denied_servers(&self.home),
        }
    }
}

/// The chief with no tools: routing and merging read only the prompt.
fn bare(prompt: String) -> Run<'static> {
    Run {
        agent: "chief-of-staff",
        model: "sonnet",
        prompt,
        json_schema: None,
        max_budget_usd: 0.5,
        tools: Toolset::None,
        deadline: Duration::from_secs(120),
        denied: Vec::new(),
    }
}
