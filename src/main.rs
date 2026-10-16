use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
