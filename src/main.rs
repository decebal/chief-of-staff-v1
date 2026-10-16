use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use cos::agents::{self, Harness};
use cos::claude::Run;
use cos::mcp::{self, Mode};
use cos::pipeline::Config;

/// Chief of staff: a tactical and a strategic brain over headless Claude Code.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Repo holding CLAUDE.md, .claude/agents and context/.
    #[arg(long, env = "COS_HOME", default_value_os_t = default_home())]
    home: PathBuf,
    /// Directory scanned for chronis repos.
    #[arg(long, env = "COS_PROJECTS", default_value_os_t = home_dir().join("Projects"))]
    projects: PathBuf,
    /// Spending cap per Claude run, in USD.
    #[arg(long)]
    budget: Option<f64>,
    /// Print the prompt instead of calling Claude.
    #[arg(long)]
    dry_run: bool,
    /// Write the HTML report but do not open it.
    #[arg(long)]
    no_open: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Tactician: today's calendar, inbox, Jira, chronis and PRs → one brief.
    Brief,
    /// Strategist: this week against your goals and bets.
    Review,
    /// Ask the chief of staff; it routes to one or both brains and merges.
    Ask { question: Vec<String> },
    /// Rebuild the HTML report from saved runs and open it. Free; calls nothing.
    Report,
    /// Serve the dashboard live on localhost; an open tab reloads after every run.
    Dash {
        #[arg(long, default_value_t = 8790)]
        port: u16,
    },
    /// MCP server on stdio, for Claude Desktop and Codex. All tools.
    Mcp,
    /// MCP server over streamable HTTP at /mcp/<token>. Read-only tools only.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
        /// Path secret; the URL is the credential, so keep it long and private.
        #[arg(long, env = "COS_MCP_TOKEN", hide_env_values = true)]
        token: String,
        /// Public hostname in front of the server (a tunnel); its Host header is accepted.
        #[arg(long, env = "COS_PUBLIC_HOST")]
        public_host: Option<String>,
    },
    /// Read-only MCP server behind a Cloudflare quick tunnel, for claude.ai.
    /// Prints the connector URL; Ctrl-C stops both.
    Tunnel {
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
    /// Spin off coding agents on a brief's or review's prompts, and watch them.
    Agents {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// Start an agent in the background; prints its id. `--budget` caps a
    /// Claude agent's spend (default $5; Codex has no budget flag).
    Spawn {
        /// Take the prompt from the latest saved brief (must-dos) or review (bets).
        #[arg(long, value_enum, conflicts_with = "prompt")]
        from: Option<agents::From>,
        /// Which item, numbered as the report numbers it.
        #[arg(long, requires = "from")]
        item: Option<usize>,
        /// Every item with a prompt, one agent each.
        #[arg(long, requires = "from", conflicts_with = "item")]
        all: bool,
        /// A prompt of your own instead.
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long, value_enum, default_value_t = Harness::Claude)]
        harness: Harness,
        /// Repo to work in; default: the first ~/Projects/<repo> the prompt names.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Work in the repo's own checkout instead of a detached worktree.
        #[arg(long)]
        in_place: bool,
        #[arg(long)]
        model: Option<String>,
        /// Seconds before the agent is stopped; capped at 7200.
        #[arg(long, default_value_t = 1800)]
        deadline: u64,
    },
    /// Every agent: status, tool calls, cost, latest activity.
    Ls,
    /// The table, redrawn every 2 s until no agent is running.
    Watch,
    /// One agent in full, with the command to take it over interactively.
    Show { id: String },
    /// Its activity, one line per tool call or message.
    Tail {
        id: String,
        /// Keep following until it ends.
        #[arg(short, long)]
        follow: bool,
    },
    /// Stop an agent and its whole process group.
    Kill { id: String },
    /// Remove finished agents' directories and worktrees. Refuses one whose
    /// worktree holds work, unless --force.
    Clean {
        id: Option<String>,
        #[arg(long)]
        force: bool,
    },
    #[command(hide = true)]
    Supervise { id: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config {
        home: cli.home.clone(),
        projects: cli.projects.clone(),
        budget: cli.budget,
    };
    match &cli.cmd {
        Cmd::Brief if cli.dry_run => print_run(&cfg.brief_run(&cfg.gather()?)),
        Cmd::Review if cli.dry_run => print_run(&cfg.review_run(&cfg.gather()?)),
        Cmd::Ask { question } if cli.dry_run => print_run(&Config::route_run(&question.join(" "))),
        Cmd::Brief => {
            let done = cfg.brief()?;
            finish(&done.markdown, done.cost_usd, done.saved.as_deref())?;
            show_report(&cli)
        }
        Cmd::Review => {
            let done = cfg.review()?;
            finish(&done.markdown, done.cost_usd, done.saved.as_deref())?;
            show_report(&cli)
        }
        Cmd::Report => {
            fold_latest_run(&cli.home)?;
            cos::report::build(&cli.home)?;
            show_report(&cli)
        }
        Cmd::Dash { port } => {
            if !cli.no_open {
                let url = format!("http://127.0.0.1:{port}/");
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    let _ = cos::report::open(std::path::Path::new(&url));
                });
            }
            runtime()?.block_on(cos::dash::serve(cli.home.clone(), *port))
        }
        Cmd::Ask { question } => {
            let done = cfg.ask(&question.join(" "))?;
            finish(&done.markdown, done.cost_usd, None)
        }
        Cmd::Mcp => runtime()?.block_on(mcp::stdio(cfg, Mode::Local)),
        Cmd::Serve {
            addr,
            token,
            public_host,
        } => runtime()?.block_on(mcp::http(cfg, addr, token, public_host.clone())),
        Cmd::Tunnel { port } => runtime()?.block_on(cos::tunnel::run(cfg, *port)),
        Cmd::Agents { cmd } => agents_cmd(&cli, cmd),
    }
}

fn agents_cmd(cli: &Cli, cmd: &AgentCmd) -> Result<()> {
    let home = &cli.home;
    match cmd {
        AgentCmd::Spawn {
            from,
            item,
            all,
            prompt,
            harness,
            repo,
            in_place,
            model,
            deadline,
        } => {
            let picked: Vec<agents::Task> = match (from, prompt) {
                (Some(from), _) => {
                    let tasks = agents::tasks(home, *from)?;
                    match item {
                        _ if *all => tasks,
                        Some(n) => vec![
                            tasks
                                .into_iter()
                                .find(|t| t.n == *n)
                                .with_context(|| format!("item {n} has no agent prompt"))?,
                        ],
                        None => {
                            for t in &tasks {
                                println!("{:>2}  {}", t.n, t.title);
                            }
                            anyhow::bail!("pick one with --item <n>, or --all");
                        }
                    }
                }
                (None, Some(p)) => vec![agents::Task {
                    n: 0,
                    title: p.lines().next().unwrap_or("").chars().take(80).collect(),
                    prompt: p.clone(),
                    source: "prompt".into(),
                }],
                (None, None) => anyhow::bail!("give --from brief|review or --prompt"),
            };
            let cos = std::env::current_exe().context("locate the cos binary")?;
            for t in picked {
                let repo = repo
                    .clone()
                    .or_else(|| agents::infer_repo(&t.prompt, &cli.projects));
                let spec = agents::spawn(
                    home,
                    agents::Request {
                        harness: *harness,
                        program: None,
                        model: model.clone(),
                        worktree: !in_place,
                        repo,
                        title: t.title,
                        source: t.source,
                        prompt: t.prompt,
                        deadline: Duration::from_secs(*deadline),
                        budget_usd: cli.budget.unwrap_or(5.0),
                    },
                    &cos,
                )?;
                println!("{}  {}  {}", spec.id, spec.workdir.display(), spec.title);
            }
            eprintln!("watch: cos agents watch   ·   live page: cos dash → /agents");
            Ok(())
        }
        AgentCmd::Ls => {
            print!("{}", agents::view::table(&agents::list(home)?));
            Ok(())
        }
        AgentCmd::Watch => loop {
            let list = agents::list(home)?;
            print!("\x1b[2J\x1b[H{}", agents::view::table(&list));
            if list.iter().all(|(_, s)| s.status.terminal()) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(2));
        },
        AgentCmd::Show { id } => {
            let (spec, state) = agents::read(home, id)?;
            print!("{}", agents::view::show(&spec, &state));
            Ok(())
        }
        AgentCmd::Tail { id, follow } => {
            let (spec, _) = agents::read(home, id)?;
            let events = agents::events_path(home, id);
            let mut at = 0;
            loop {
                let (lines, next) = agents::view::activity(&events, spec.harness, at);
                for l in lines {
                    println!("{l}");
                }
                at = next;
                let (_, state) = agents::read(home, id)?;
                if !follow || state.status.terminal() {
                    if *follow {
                        println!("— {}", state.status);
                    }
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        AgentCmd::Kill { id } => {
            let state = agents::stop(home, id, Duration::from_secs(10))?;
            println!("{id} {}", state.status);
            Ok(())
        }
        AgentCmd::Clean { id, force } => {
            let ids: Vec<String> = match id {
                Some(id) => vec![id.clone()],
                None => agents::list(home)?
                    .into_iter()
                    .filter(|(_, s)| s.status.terminal())
                    .map(|(spec, _)| spec.id)
                    .collect(),
            };
            let mut refused = 0;
            for id in ids {
                match agents::clean(home, &id, *force) {
                    Ok(()) => println!("removed {id}"),
                    Err(e) => {
                        refused += 1;
                        eprintln!("kept {id}: {e:#}");
                    }
                }
            }
            anyhow::ensure!(refused == 0, "{refused} agent(s) kept");
            Ok(())
        }
        AgentCmd::Supervise { id } => agents::supervise(home, id),
    }
}

/// Folds the newest saved facts (and that day's brief) into the archive;
/// re-recording a day replaces its row, so this is safe to repeat.
fn fold_latest_run(home: &std::path::Path) -> Result<()> {
    use cos::store::{Kind, latest};
    let Some(facts) = latest::<cos::gather::Facts>(home, Kind::Facts)? else {
        return Ok(());
    };
    let brief = latest::<cos::reports::Brief>(home, Kind::Briefs)?.filter(|b| b.day == facts.day);
    cos::history::record(
        home,
        &facts.day,
        &facts.value,
        brief.as_ref().map(|b| &b.value),
    )?;
    Ok(())
}

fn show_report(cli: &Cli) -> Result<()> {
    let path = cli.home.join("out/report.html");
    eprintln!("report {}", path.display());
    if cli.no_open {
        return Ok(());
    }
    cos::report::open(&path)
}

fn finish(markdown: &str, cost: f64, saved: Option<&std::path::Path>) -> Result<()> {
    println!("{markdown}");
    eprintln!("cost ${cost:.2}");
    if let Some(p) = saved {
        eprintln!("wrote {}", p.display());
    }
    Ok(())
}

fn print_run(run: &Run) -> Result<()> {
    println!(
        "--agent {} --model {}\n\n{}",
        run.agent, run.model, run.prompt
    );
    Ok(())
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("start tokio runtime")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The clone `cos` was built from: `task install` builds in place, so an
/// installed binary finds its agents and context with no configuration.
fn default_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
