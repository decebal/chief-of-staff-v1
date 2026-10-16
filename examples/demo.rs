//! A fully synthetic `COS_HOME` for screenshots and trying `cos` without
//! connecting anything: a fictional person, company, repos and teammates.
//! Nothing here reads your accounts, and every month is pre-marked archived
//! so `cos report` and `cos dash` never call GitHub for it.
//!
//! ```sh
//! cargo run --example demo -- /tmp/cos-demo
//! cos --home /tmp/cos-demo dash
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use cos::gather::{Facts, Pr, Task};
use cos::history::{DayRow, Merged};
use cos::reports::{Brief, Review};
use cos::store::{self, Kind};
use jiff::{Span, civil::Date};
use serde_json::json;

const ORG: &str = "northwind";
const REPOS: &[&str] = &["api-gateway", "web-app", "mobile", "infra", "design-system"];

/// xorshift64*: deterministic, so screenshots are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn main() -> Result<()> {
    let home = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/cos-demo".into()),
    );
    anyhow::ensure!(
        !home.join(".git").exists(),
        "{} looks like a repo; point the demo at an empty directory",
        home.display()
    );
    let today = jiff::Zoned::now().date();
    let mut rng = Rng(0x00C0_FFEE_2026);

    write(
        &home.join("context/profile.md"),
        "# Profile\n\n- **Name**: Alex Rivera\n- **Role and team**: staff engineer, Northwind Labs platform team\n",
    )?;
    write(
        &home.join("context/goals.md"),
        "# Goals\n\n## Mobile 2.0 on the new gateway\n## Halve review wait time\n## Design system v3\n",
    )?;

    let facts = facts(today, &mut rng);
    store::save_json(&home, Kind::Facts, &today.to_string(), &facts)?;

    let brief: Brief = serde_json::from_value(brief_json(today))?;
    let day = today.to_string();
    store::save(&home, Kind::Briefs, &day, &brief, &brief.to_markdown(&day))?;
    let review: Review = serde_json::from_value(review_json(today))?;
    store::save(
        &home,
        Kind::Reviews,
        &day,
        &review,
        &review.to_markdown(&day),
    )?;

    history(&home, today, &facts, &mut rng)?;

    let report = cos::report::build(&home)?;
    for back in 1..=12 {
        let d = today.checked_sub(Span::new().days(back))?;
        std::fs::copy(&report, home.join(format!("out/reports/{d}.html")))?;
    }
    println!("demo home ready: {}", home.display());
    println!("  cos --home {} dash", home.display());
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, text)?;
    Ok(())
}

fn ts(d: Date, h: u8) -> String {
    format!("{d}T{h:02}:00:00Z")
}

fn facts(today: Date, rng: &mut Rng) -> Facts {
    let titles = [
        "Rate-limit the public token endpoint",
        "Cache layout tokens at build time",
        "Retry uploads on flaky networks",
        "Move preview envs to the new cluster",
        "Dark-mode tokens for charts",
        "Paginate the audit log API",
        "Offline queue for mobile sync",
        "Trace IDs through the gateway",
        "Remove the legacy session cookie",
        "Typed client for the billing API",
        "Speed up cold starts on Android",
        "Canary deploys for the gateway",
    ];
    let mut tasks = Vec::new();
    for (i, t) in titles.iter().enumerate() {
        let repo = REPOS[i % REPOS.len()];
        let pri = ["p0", "p1", "p1", "p2", "p2", "p3"][rng.below(6) as usize];
        tasks.push(Task {
            repo: repo.into(),
            id: format!("t-{:04x}", 0x3a00 + i * 37),
            kind: if i % 5 == 0 {
                "epic".into()
            } else {
                "task".into()
            },
            title: (*t).into(),
            priority: pri.into(),
            blocked_by: if i % 4 == 3 {
                format!("t-{:04x}", 0x3a00 + (i - 1) * 37)
            } else {
                String::new()
            },
            parent: String::new(),
        });
    }
    for extra in 0..30 {
        let repo = REPOS[rng.below(REPOS.len() as u64) as usize];
        tasks.push(Task {
            repo: repo.into(),
            id: format!("t-{:04x}", 0x5000 + extra),
            kind: "task".into(),
            title: format!("Follow-up {extra} for {repo}"),
            priority: ["p1", "p2", "p2", "p3"][rng.below(4) as usize].into(),
            blocked_by: String::new(),
            parent: String::new(),
        });
    }
    tasks.sort_by(|a, b| a.repo.cmp(&b.repo));

    let pr = |repo: &str, n: u64, title: &str, updated: Date, closed: Option<Date>| Pr {
        repo: format!("{ORG}/{repo}"),
        number: n,
        title: title.into(),
        draft: false,
        updated: ts(updated, 10),
        closed: closed.map(|c| ts(c, 15)),
    };
    let ago = |d: i64| today.checked_sub(Span::new().days(d)).unwrap();
    let review_requested = vec![
        pr(
            "api-gateway",
            412,
            "Release 3.4: gateway auth rewrite",
            ago(0),
            None,
        ),
        pr("mobile", 288, "Offline sync queue", ago(1), None),
        pr("web-app", 1931, "Chart dark-mode tokens", ago(2), None),
        pr("design-system", 77, "Button v3 variants", ago(4), None),
        pr("infra", 640, "Preview env autoscaling", ago(6), None),
        pr("web-app", 1902, "Billing page a11y fixes", ago(12), None),
        pr("api-gateway", 398, "Trace propagation", ago(21), None),
        pr("mobile", 251, "Android cold start", ago(45), None),
    ];
    let mine_open = vec![
        pr("api-gateway", 415, "Canary deploy pipeline", ago(0), None),
        pr("web-app", 1936, "Audit log pagination", ago(1), None),
        pr("infra", 644, "Cluster migration runbook", ago(3), None),
    ];
    let mut merged = Vec::new();
    for back in 0..7i64 {
        let n = if back % 7 >= 5 {
            rng.below(2)
        } else {
            2 + rng.below(5)
        };
        for k in 0..n {
            let repo = REPOS[rng.below(REPOS.len() as u64) as usize];
            merged.push(pr(
                repo,
                2000 + (back as u64) * 10 + k,
                "Shipped change",
                ago(back),
                Some(ago(back)),
            ));
        }
    }
    Facts {
        collected_at: jiff::Zoned::now().to_string(),
        merged_since: ago(7).to_string(),
        tasks,
        mine_open,
        review_requested,
        merged,
        gaps: vec![],
    }
}

fn brief_json(today: Date) -> serde_json::Value {
    let in_days = |d: i64| today.checked_add(Span::new().days(d)).unwrap().to_string();
    let stop = "Stop before anything outward-facing — merging, commenting, sending, approving — and show me the draft.";
    json!({
        "headline": "Release 3.4 ships Monday and its PR waits on your review; the mobile sync PR blocks Priya.",
        "calendar": [
            { "start": "09:30", "end": "09:45", "what": "Platform stand-up", "prep": "Raise the gateway canary blocker." },
            { "start": "11:00", "end": "12:00", "what": "Design review: Button v3", "prep": "Read design-system#77 first." },
            { "start": "14:00", "end": "14:30", "what": "1:1 with Priya", "prep": "Unblock the offline sync PR." },
            { "start": "16:00", "end": "17:00", "what": "Release 3.4 go/no-go", "prep": "Have api-gateway#412 reviewed." }
        ],
        "must_do": [
            { "item": "Review northwind/api-gateway#412 (Release 3.4)", "why": "Monday's release cannot ship without a reviewer.", "source": "GitHub: awaiting your review; calendar: go/no-go 16:00",
              "agent_prompt": format!("Work in ~/Projects/api-gateway. Run `gh pr checkout 412`, `gh pr diff 412` and `gh pr checks 412`. Review the auth rewrite for correctness and rollout risk; list findings with file and line, and a verdict. {stop}") },
            { "item": "Review northwind/mobile#288 (offline sync queue)", "why": "Priya is blocked on it; you meet her at 14:00.", "source": "GitHub: awaiting your review",
              "agent_prompt": format!("Work in ~/Projects/mobile. Run `gh pr checkout 288`. Check the queue's retry and ordering logic and its tests. Draft review comments. {stop}") },
            { "item": "Prep the Button v3 design review", "why": "The review is at 11:00 and design-system#77 is the agenda.", "source": "Calendar; GitHub design-system#77",
              "agent_prompt": format!("Work in ~/Projects/design-system. Read PR #77 and the v3 spec. Write five talking points and two open questions. {stop}") },
            { "item": "Unblock t-3a6f: trace IDs through the gateway", "why": "It is the p1 under the observability epic and blocks two tasks.", "source": "chronis api-gateway",
              "agent_prompt": format!("Work in ~/Projects/api-gateway. Run `cn show t-3a6f --toon`, find what blocks it, and propose the smallest change that unblocks it. {stop}") },
            { "item": "Reply to Sam about the cluster migration window", "why": "The infra freeze starts Wednesday.", "source": "Gmail: Sam, 'Migration window?'",
              "agent_prompt": format!("Draft a reply to Sam proposing Tuesday 18:00-20:00 for the cluster migration, citing infra#644 runbook. {stop}") }
        ],
        "reply_to": [ { "from": "Sam Okafor", "subject": "Migration window?", "ask": "Pick a window before Wednesday's freeze." } ],
        "blockers": [
            { "item": "api-gateway#415 canary pipeline", "waiting_on": "infra#640 autoscaling", "unblock": "Ask Jordan for an ETA at stand-up." },
            { "item": "web-app#1902 a11y fixes", "waiting_on": "design tokens from v3", "unblock": "Confirm token names in the 11:00 review." }
        ],
        "can_drop": [ "Newsletter and vendor threads", "mobile#251 (idle 45 days; ask the author if it is still wanted)" ],
        "gaps": [ "Notion was not searched for this brief." ],
        "celebrations": [
            { "win": "Gateway auth rewrite feature-complete (api-gateway#405-#411)", "why_it_matters": "Release 3.4 is now review-only.", "source": "merged this week" },
            { "win": "Preview environments 40% cheaper (infra#633)", "why_it_matters": "Moves the cost goal for the quarter.", "source": "merged this week" },
            { "win": "Design tokens v3 landed (design-system#71)", "why_it_matters": "Unblocks dark mode across web and mobile.", "source": "merged this week" }
        ],
        "upcoming": [
            { "date": in_days(3), "what": "Release 3.4", "source": "calendar" },
            { "date": in_days(8), "what": "Mobile 2.0 beta", "source": "goals.md" },
            { "date": in_days(15), "what": "Q1 planning", "source": "calendar" },
            { "date": in_days(29), "what": "Design system v3 GA", "source": "goals.md" }
        ]
    })
}

fn review_json(today: Date) -> serde_json::Value {
    let in_days = |d: i64| today.checked_add(Span::new().days(d)).unwrap().to_string();
    let stop = "Stop before anything outward-facing and show me the draft.";
    json!({
        "summary": "Two of three bets moved this week; review wait time is the one drifting.",
        "bets": [
            { "bet": "Mobile 2.0 on the new gateway", "confidence": "rock", "evidence": "Auth rewrite merged; 3.4 in review.", "next": "Ship 3.4, then cut the mobile beta branch.",
              "agent_prompt": format!("Work in ~/Projects/mobile. List what the 2.0 beta still needs from the gateway 3.4 API and draft the beta checklist. {stop}") },
            { "bet": "Halve review wait time", "confidence": "sand", "evidence": "Queue grew from 5 to 8; two PRs older than 3 weeks.", "next": "Shape it: pick a rotation or an SLA before more work.",
              "agent_prompt": format!("Using `gh search prs --review-requested=@me --state=open`, chart review age by repo and propose two options for a review rotation. {stop}") },
            { "bet": "Design system v3", "confidence": "water", "evidence": "Tokens landed; component scope still undefined.", "next": "Spike: inventory components used by web and mobile.",
              "agent_prompt": format!("Work in ~/Projects/design-system. Inventory components imported by web-app and mobile; output a table of usage counts. {stop}") }
        ],
        "drift": [ "Two days on preview-env cost work, which serves no stated bet." ],
        "decisions": [ { "decision": "Review rotation or review SLA?", "options": ["Weekly rotation", "48h SLA", "Both"], "recommendation": "Weekly rotation; measure for two weeks." } ],
        "stop_doing": [ "Ad-hoc reviews outside the rotation once it starts." ],
        "gaps": [],
        "celebrations": [ { "win": "Auth rewrite done", "why_it_matters": "Moves Mobile 2.0 from sand to rock.", "source": "api-gateway" } ],
        "upcoming": [ { "date": in_days(8), "what": "Mobile 2.0 beta", "source": "goals.md" } ]
    })
}

fn history(home: &Path, today: Date, facts: &Facts, rng: &mut Rng) -> Result<()> {
    let dir = home.join("out/history");
    std::fs::create_dir_all(&dir)?;
    let mut days = Vec::new();
    for back in (0..30i64).rev() {
        let d = today.checked_sub(Span::new().days(back))?;
        if d.weekday().to_monday_one_offset() > 5 {
            continue;
        }
        let ready = BTreeMap::from([
            ("p0".to_owned(), 2 + rng.below(3) as usize),
            ("p1".to_owned(), 10 + rng.below(6) as usize),
            ("p2".to_owned(), 14 + rng.below(8) as usize),
            ("p3".to_owned(), 3 + rng.below(4) as usize),
        ]);
        days.push(DayRow {
            date: d.to_string(),
            review_queue: (4 + (30 - back as usize) / 7 + rng.below(3) as usize).min(12),
            mine_open: 2 + rng.below(4) as usize,
            ready,
            must_do: Some(3 + rng.below(3) as usize),
            meeting_minutes: Some(60 + 15 * rng.below(10) as u32),
            wins: Some(rng.below(4) as usize),
        });
    }
    if let Some(last) = days.last_mut() {
        *last = cos::history::row(&today.to_string(), facts, None);
        last.must_do = Some(5);
        last.meeting_minutes = Some(165);
        last.wins = Some(3);
    }

    let mut merged = Vec::new();
    for back in 0..365i64 {
        let d = today.checked_sub(Span::new().days(back))?;
        let weekday = d.weekday().to_monday_one_offset() <= 5;
        let busy = (back / 30) % 4 != 1;
        let n = if !weekday {
            rng.below(2)
        } else if busy {
            rng.below(6)
        } else {
            rng.below(3)
        };
        for k in 0..n {
            merged.push(Merged {
                repo: format!("{ORG}/{}", REPOS[rng.below(REPOS.len() as u64) as usize]),
                number: 10_000 + (back as u64) * 10 + k,
                title: "Shipped change".into(),
                day: d.to_string(),
            });
        }
    }
    merged.sort_by(|a, b| a.day.cmp(&b.day));
    let backfilled: Vec<String> = cos::history::months_to(&today.to_string())
        .into_iter()
        .map(|(from, _)| from)
        .collect();

    std::fs::write(dir.join("daily.json"), serde_json::to_vec_pretty(&days)?)?;
    std::fs::write(dir.join("merged.json"), serde_json::to_vec_pretty(&merged)?)?;
    std::fs::write(
        dir.join("backfilled.json"),
        serde_json::to_vec_pretty(&backfilled)?,
    )?;
    Ok(())
}
