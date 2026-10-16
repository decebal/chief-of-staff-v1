//! `cos` as an MCP server, so Claude Desktop, Codex and claude.ai can call it.
//!
//! Two modes. `Local` (stdio) offers every tool, including the paid runs.
//! `Remote` (HTTP behind a secret path) offers only the free read tools: a
//! leaked URL can read the last brief, never spend money or trigger a run.

use std::convert::Infallible;
use std::sync::Arc;

use anyhow::Result;
use rmcp::handler::server::tool::schema_for_output;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::pipeline::Config;
use crate::reports::{Brief, Review};
use crate::store::{self, Kind};

const DASHBOARD_URI: &str = "ui://cos/dashboard";
const DASHBOARD_HTML: &str = include_str!("dashboard.html");
const APP_MIME: &str = "text/html;profile=mcp-app";

/// Tools a remote client may call. Everything else is local-only.
const REMOTE_TOOLS: &[&str] = &["latest_brief", "latest_review"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Local,
    Remote,
}

#[derive(Clone)]
pub struct Cos {
    cfg: Config,
    mode: Mode,
}

/// What every tool returns as structured content; the dashboard reads this.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Report {
    /// "brief" or "review"; null when nothing is saved yet.
    kind: Option<String>,
    day: Option<String>,
    report: Option<Value>,
    markdown: Option<String>,
    /// Set only for a fresh run.
    cost_usd: Option<f64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AskArgs {
    /// The question for the chief of staff.
    question: String,
}

fn ui_meta() -> MetaObject {
    let Value::Object(map) = json!({ "ui": { "resourceUri": DASHBOARD_URI } }) else {
        unreachable!()
    };
    MetaObject::from(map)
}

fn structured(r: Report) -> CallToolResult {
    CallToolResult::structured(serde_json::to_value(r).unwrap_or(Value::Null))
}

fn failed(e: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))])
}

#[tool_router]
impl Cos {
    pub fn new(cfg: Config, mode: Mode) -> Self {
        Self { cfg, mode }
    }

    /// The most recent saved daily brief: calendar, must-do, blockers, what was
    /// not read. Free and instant; does not call Claude.
    #[tool(meta = ui_meta(), output_schema = schema_for_output::<Report>(),
           annotations(read_only_hint = true, open_world_hint = false))]
    async fn latest_brief(&self) -> CallToolResult {
        self.latest::<Brief>(Kind::Briefs, "brief")
    }

    /// The most recent saved weekly review: bets with rock/sand/water
    /// confidence, drift, decisions. Free and instant; does not call Claude.
    #[tool(meta = ui_meta(), output_schema = schema_for_output::<Report>(),
           annotations(read_only_hint = true, open_world_hint = false))]
    async fn latest_review(&self) -> CallToolResult {
        self.latest::<Review>(Kind::Reviews, "review")
    }

    /// Run a fresh daily brief now. Calls Claude: costs money (about $0.30) and
    /// can take a few minutes. Prefer latest_brief unless asked to refresh.
    #[tool(meta = ui_meta(), output_schema = schema_for_output::<Report>(),
           annotations(read_only_hint = true, open_world_hint = true))]
    async fn brief(&self) -> CallToolResult {
        let cfg = self.cfg.clone();
        match tokio::task::spawn_blocking(move || cfg.brief()).await {
            Ok(Ok(done)) => structured(Report {
                kind: Some("brief".into()),
                day: Some(jiff::Zoned::now().date().to_string()),
                report: serde_json::to_value(&done.value).ok(),
                markdown: Some(done.markdown),
                cost_usd: Some(done.cost_usd),
            }),
            Ok(Err(e)) => failed(e),
            Err(e) => failed(e),
        }
    }

    /// Run a fresh weekly review now. Calls Claude: costs money (about $1–2)
    /// and can take a few minutes. Prefer latest_review unless asked to refresh.
    #[tool(meta = ui_meta(), output_schema = schema_for_output::<Report>(),
           annotations(read_only_hint = true, open_world_hint = true))]
    async fn review(&self) -> CallToolResult {
        let cfg = self.cfg.clone();
        match tokio::task::spawn_blocking(move || cfg.review()).await {
            Ok(Ok(done)) => structured(Report {
                kind: Some("review".into()),
                day: Some(jiff::Zoned::now().date().to_string()),
                report: serde_json::to_value(&done.value).ok(),
                markdown: Some(done.markdown),
                cost_usd: Some(done.cost_usd),
            }),
            Ok(Err(e)) => failed(e),
            Err(e) => failed(e),
        }
    }

    /// Ask the chief of staff a question about priorities or today's work. It
    /// routes to the tactical and/or strategic brain and merges their answers.
    /// Calls Claude: costs money (about $1–1.50) and can take a few minutes.
    #[tool(annotations(read_only_hint = true, open_world_hint = true))]
    async fn ask(&self, Parameters(args): Parameters<AskArgs>) -> CallToolResult {
        let cfg = self.cfg.clone();
        match tokio::task::spawn_blocking(move || cfg.ask(&args.question)).await {
            Ok(Ok(done)) => CallToolResult::success(vec![ContentBlock::text(format!(
                "{}\n\n(cost ${:.2})",
                done.markdown, done.cost_usd
            ))]),
            Ok(Err(e)) => failed(e),
            Err(e) => failed(e),
        }
    }
}

impl Cos {
    fn latest<T: serde::de::DeserializeOwned + Serialize>(
        &self,
        kind: Kind,
        name: &str,
    ) -> CallToolResult {
        match store::latest::<T>(&self.cfg.home, kind) {
            Ok(Some(saved)) => structured(Report {
                kind: Some(name.into()),
                day: Some(saved.day),
                report: serde_json::to_value(&saved.value).ok(),
                markdown: Some(saved.markdown),
                cost_usd: None,
            }),
            Ok(None) => structured(Report {
                kind: None,
                day: None,
                report: None,
                markdown: None,
                cost_usd: None,
            }),
            Err(e) => failed(e),
        }
    }

    fn allowed(&self, tool: &str) -> bool {
        self.mode == Mode::Local || REMOTE_TOOLS.contains(&tool)
    }
}

impl ServerHandler for Cos {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "chief-of-staff",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Chief of staff. latest_brief / latest_review are free and instant: use them \
                 first. brief, review and ask call Claude and cost money; use them only when \
                 the user asks for a fresh run or a question the saved brief cannot answer.",
        )
    }

    async fn list_tools(
        &self,
        _req: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = Self::tool_router()
            .list_all()
            .into_iter()
            .filter(|t| self.allowed(&t.name))
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.allowed(&req.name) {
            return Err(ErrorData::invalid_params(
                format!("tool {} is local-only", req.name),
                None,
            ));
        }
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, req, ctx);
        Self::tool_router().call(tcc).await
    }

    async fn list_resources(
        &self,
        _req: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(DASHBOARD_URI, "dashboard")
                .with_title("Chief of staff dashboard")
                .with_mime_type(APP_MIME),
        ]))
    }

    async fn read_resource(
        &self,
        req: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if req.uri != DASHBOARD_URI {
            return Err(ErrorData::invalid_params(
                format!("unknown resource {}", req.uri),
                None,
            ));
        }
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(DASHBOARD_HTML, req.uri).with_mime_type(APP_MIME),
        ])
        .into())
    }
}

pub async fn stdio(cfg: Config, mode: Mode) -> Result<()> {
    Cos::new(cfg, mode)
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

/// Streamable HTTP at `/mcp/<token>`, remote mode. Any other path is a 404.
/// `public_host` is the tunnel hostname, so its Host header is accepted.
pub async fn http(cfg: Config, addr: &str, token: &str, public_host: Option<String>) -> Result<()> {
    anyhow::ensure!(token.len() >= 32, "token must be at least 32 characters");
    let mut hosts = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
    hosts.extend(public_host);
    let svc = StreamableHttpService::new(
        move || Ok(Cos::new(cfg.clone(), Mode::Remote)),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_allowed_hosts(hosts),
    );
    let secret: Arc<str> = format!("/mcp/{token}").into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("cos: serving read-only MCP on http://{addr}/mcp/<token>");
    loop {
        let (stream, _) = listener.accept().await?;
        let (svc, secret) = (svc.clone(), secret.clone());
        tokio::spawn(async move {
            let handler =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (svc, secret) = (svc.clone(), secret.clone());
                    async move {
                        if req.uri().path() != &*secret {
                            use http_body_util::{BodyExt, Full};
                            let not_found = hyper::Response::builder()
                                .status(404)
                                .body(Full::new(bytes::Bytes::new()).boxed())
                                .expect("static response");
                            return Ok::<_, Infallible>(not_found);
                        }
                        Ok(svc.handle(req).await)
                    }
                });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), handler)
                .await;
        });
    }
}
