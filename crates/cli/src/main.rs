//! `surge` CLI — thin shim over the loopback API (code-map: cli).
//! No retries, no local state beyond the token file (INV-AUTH-4): every rule
//! is enforced server-side; this binary only presents tokens and prints what
//! the server said.

use clap::{Parser, Subcommand};
use std::io::Write as _;
use std::path::PathBuf;

const DEFAULT_API: &str = "http://127.0.0.1:7420";

#[derive(Parser)]
#[command(name = "surge", version, about = "Surge — local pipeline orchestrator")]
struct Args {
    /// Print the raw server JSON instead of formatted output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Store the human session token in machine-local config (INV-AUTH-4).
    /// With no token argument, report where the token would come from.
    Auth {
        /// Session token to store. Obtain one by visiting the one-time claim
        /// URL the server prints at boot (INV-AUTH-5).
        token: Option<String>,
    },
    /// Server health plus the most recent audit entries.
    Status,
    /// Bind a project to its repo: write `surge.yaml` and nothing else (INV-DATA-1).
    Bind {
        project_id: String,
        /// Create the project first (requires --name and --repo).
        #[arg(long)]
        create: bool,
        /// Project display name (with --create).
        #[arg(long, required_if_eq("create", "true"))]
        name: Option<String>,
        /// Absolute path to the workplace repo (with --create).
        #[arg(long, required_if_eq("create", "true"))]
        repo: Option<String>,
    },
    /// Compile a pipeline into a project's repo and print the §04 capability report.
    Compile { project_id: String, pipeline_id: String },
    /// Dispatch one issue to the runtime supervisor; prints the run id.
    Dispatch { issue_id: String },
    /// Abort a run — effective at the worker's next status poll (§06-06).
    Abort { run_id: String },
}

// ---------------------------------------------------------------- token file

fn config_dir() -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var("SURGE_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("cannot locate a home directory; set SURGE_CONFIG_DIR"))?;
    Ok(PathBuf::from(home).join(".config").join("surge"))
}

fn token_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("token"))
}

/// Write the token file with owner-only permissions (INV-AUTH-4).
fn store_token(token: &str) -> anyhow::Result<PathBuf> {
    let path = token_path()?;
    let dir = path.parent().expect("token path has a parent");
    std::fs::create_dir_all(dir)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    #[cfg(unix)]
    {
        // The file may pre-exist with looser permissions; tighten regardless.
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    writeln!(f, "{token}")?;
    Ok(path)
}

/// Token resolution order: $SURGE_TOKEN, then the config file.
fn resolve_token() -> anyhow::Result<Option<(String, &'static str)>> {
    if let Ok(t) = std::env::var("SURGE_TOKEN") {
        if !t.trim().is_empty() {
            return Ok(Some((t.trim().to_string(), "$SURGE_TOKEN")));
        }
    }
    let path = token_path()?;
    match std::fs::read_to_string(&path) {
        Ok(s) if !s.trim().is_empty() => Ok(Some((s.trim().to_string(), "config file"))),
        _ => Ok(None),
    }
}

// -------------------------------------------------------------- HTTP client

enum ApiError {
    /// No token was presented or the server rejected it (401).
    Unauthorized,
    /// Any other non-2xx: status plus the server's reason string.
    Refused { status: u16, reason: String, body: serde_json::Value },
    Unreachable(String),
    Other(anyhow::Error),
}

impl ApiError {
    fn into_anyhow(self, base: &str) -> anyhow::Error {
        match self {
            ApiError::Unauthorized => anyhow::anyhow!(
                "not authenticated (401).\n\
                 The server prints a one-time claim URL at boot (INV-AUTH-5); \
                 claim it, then store the session token with `surge auth <token>` \
                 or export $SURGE_TOKEN."
            ),
            ApiError::Refused { status: 403, reason, .. } => {
                anyhow::anyhow!("refused by server (403): {reason}")
            }
            ApiError::Refused { status, reason, .. } => {
                anyhow::anyhow!("server refused ({status}): {reason}")
            }
            ApiError::Unreachable(e) => anyhow::anyhow!(
                "no surge instance reachable at {base} ({e}) — is `surge-server` running?"
            ),
            ApiError::Other(e) => e,
        }
    }
}

struct Client {
    base: String,
    token: Option<String>,
}

impl Client {
    fn new() -> anyhow::Result<Self> {
        let base =
            std::env::var("SURGE_API").unwrap_or_else(|_| DEFAULT_API.to_string());
        let token = resolve_token()?.map(|(t, _)| t);
        Ok(Self { base, token })
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, ApiError> {
        let mut req = ureq::request(method, &format!("{}{path}", self.base));
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let result = match body {
            Some(v) => req.send_json(v.clone()),
            None => req.call(),
        };
        match result {
            Ok(resp) => resp
                .into_json::<serde_json::Value>()
                .map_err(|e| ApiError::Other(e.into())),
            Err(ureq::Error::Status(401, _)) => Err(ApiError::Unauthorized),
            Err(ureq::Error::Status(status, resp)) => {
                let body: serde_json::Value =
                    resp.into_json().unwrap_or(serde_json::Value::Null);
                let reason = body
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("no reason given")
                    .to_string();
                Err(ApiError::Refused { status, reason, body })
            }
            Err(ureq::Error::Transport(t)) => Err(ApiError::Unreachable(t.to_string())),
        }
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, ApiError> {
        self.call("GET", path, None)
    }

    fn post(
        &self,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, ApiError> {
        self.call("POST", path, body)
    }
}

// ------------------------------------------------------------------ commands

/// `surge auth` with no argument: say where a token would come from — never
/// what it is.
fn auth_report() -> anyhow::Result<()> {
    let path = token_path()?;
    let env_set = std::env::var("SURGE_TOKEN").map(|t| !t.trim().is_empty()).unwrap_or(false);
    let file_set = std::fs::read_to_string(&path)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    println!("token sources, in order:");
    println!("  1. $SURGE_TOKEN         {}", if env_set { "set" } else { "not set" });
    println!(
        "  2. {}  {}",
        path.display(),
        if file_set { "configured" } else { "not configured" }
    );
    if !env_set && !file_set {
        println!(
            "\nno token configured. The server prints a one-time claim URL at boot \
             (INV-AUTH-5); claim it, then run `surge auth <token>`."
        );
    }
    Ok(())
}

fn cmd_status(client: &Client, json: bool) -> anyhow::Result<()> {
    let health = client.get("/healthz").map_err(|e| e.into_anyhow(&client.base))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&health)?);
    } else {
        println!(
            "surge {} at {} — schema v{}",
            health.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
            client.base,
            health.get("schema_version").and_then(|v| v.as_i64()).unwrap_or(0),
        );
    }
    if client.token.is_none() {
        println!("not authenticated — audit skipped (run `surge auth`)");
        return Ok(());
    }
    let audit = client
        .get("/api/audit?limit=5")
        .map_err(|e| e.into_anyhow(&client.base))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&audit)?);
        return Ok(());
    }
    println!("recent audit:");
    for entry in audit.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        println!(
            "  [{}] {} — {} ({})",
            entry.get("at").and_then(|v| v.as_i64()).unwrap_or(0),
            entry.get("action").and_then(|v| v.as_str()).unwrap_or("?"),
            entry.get("subject").and_then(|v| v.as_str()).unwrap_or("?"),
            entry.get("actor").and_then(|v| v.as_str()).unwrap_or("?"),
        );
    }
    Ok(())
}

fn cmd_bind(
    client: &Client,
    project_id: &str,
    create: bool,
    name: Option<String>,
    repo: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if create {
        let body = serde_json::json!({
            "id": project_id,
            "name": name.expect("clap enforces --name with --create"),
            "repo_path": repo.expect("clap enforces --repo with --create"),
        });
        client
            .post("/api/projects", Some(&body))
            .map_err(|e| e.into_anyhow(&client.base))?;
        if !json {
            println!("project {project_id} created");
        }
    }
    let project = client
        .post(&format!("/api/projects/{project_id}/bind"), None)
        .map_err(|e| e.into_anyhow(&client.base))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&project)?);
    } else {
        println!(
            "project {project_id} bound — surge.yaml written to {}",
            project.get("repo_path").and_then(|v| v.as_str()).unwrap_or("?"),
        );
    }
    Ok(())
}

/// Print the §04 capability report: the four lines a human accepts when they
/// compile, then the signature material (hash + cache key).
fn cmd_compile(
    client: &Client,
    project_id: &str,
    pipeline_id: &str,
    json: bool,
) -> anyhow::Result<()> {
    let body = serde_json::json!({ "pipeline_id": pipeline_id });
    let resp = client
        .post(&format!("/api/projects/{project_id}/compile"), Some(&body))
        .map_err(|e| e.into_anyhow(&client.base))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    let report = resp.get("capability_report").cloned().unwrap_or_default();
    let strings = |key: &str| -> Vec<String> {
        report
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let writes = strings("writes");
    let network = strings("network");
    let shell_first = strings("shell_first");
    let shell_count = report.get("shell_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let mat = resp.get("materialization").cloned().unwrap_or_default();

    println!("compiled {pipeline_id} → {project_id}");
    println!(
        "  writes:  {}",
        if writes.is_empty() { "none".into() } else { writes.join(", ") }
    );
    if shell_count == 0 {
        println!("  shell:   0 commands");
    } else {
        println!("  shell:   {shell_count} commands (first: {})", shell_first.join(" · "));
    }
    println!(
        "  network: {}",
        if network.is_empty() { "none".into() } else { network.join(", ") }
    );
    println!(
        "  egress:  {}",
        report.get("egress").and_then(|v| v.as_str()).unwrap_or("?")
    );
    println!(
        "  hash:      {}",
        mat.get("content_hash").and_then(|v| v.as_str()).unwrap_or("?")
    );
    println!(
        "  cache key: {}",
        mat.get("cache_key").and_then(|v| v.as_str()).unwrap_or("?")
    );
    Ok(())
}

fn cmd_dispatch(client: &Client, issue_id: &str, json: bool) -> anyhow::Result<()> {
    match client.post(&format!("/api/issues/{issue_id}/dispatch"), None) {
        Ok(resp) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "run {} dispatched for issue {issue_id}",
                    resp.get("run_id").and_then(|v| v.as_str()).unwrap_or("?"),
                );
            }
            Ok(())
        }
        // A dispatch refusal is a visible record (INV-ERR-1): the server made
        // a refusal run whose id is worth printing alongside the reason.
        Err(ApiError::Refused { status: 409, reason, body }) => {
            let run_id = body.get("run_id").and_then(|v| v.as_str()).unwrap_or("?");
            anyhow::bail!("{reason} (refusal run {run_id})")
        }
        Err(e) => Err(e.into_anyhow(&client.base)),
    }
}

fn cmd_abort(client: &Client, run_id: &str, json: bool) -> anyhow::Result<()> {
    let resp = client
        .post(&format!("/api/runs/{run_id}/abort"), None)
        .map_err(|e| e.into_anyhow(&client.base))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!("run {run_id} aborted — lands at the worker's next status poll");
    }
    Ok(())
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Auth { token: Some(token) } => {
            let path = store_token(&token)?;
            println!("token stored at {}", path.display());
            Ok(())
        }
        Command::Auth { token: None } => auth_report(),
        Command::Status => cmd_status(&Client::new()?, args.json),
        Command::Bind { project_id, create, name, repo } => {
            cmd_bind(&Client::new()?, &project_id, create, name, repo, args.json)
        }
        Command::Compile { project_id, pipeline_id } => {
            cmd_compile(&Client::new()?, &project_id, &pipeline_id, args.json)
        }
        Command::Dispatch { issue_id } => cmd_dispatch(&Client::new()?, &issue_id, args.json),
        Command::Abort { run_id } => cmd_abort(&Client::new()?, &run_id, args.json),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("surge: {e}");
        std::process::exit(1);
    }
}
