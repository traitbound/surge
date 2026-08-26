//! The plugin skeleton against a live Surge API: the zero-dep Node MCP server
//! is driven over real stdio JSON-RPC, and the fallback hook scripts run as
//! Claude Code would run them. This is item 6's "proves run → spans-back"
//! without needing claude itself.

use std::process::Stdio;
use std::time::Duration;
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn plugin_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/claude-plugin")
}

struct Live {
    state: AppState,
    api: String,
    rt_token: String,
}

/// Project + fixture issue with a claimed lease + running run, live server.
async fn live() -> Live {
    let pool = surge_store::open_in_memory().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(pool.clone());
    let router = app(state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    surge_store::projects::insert(
        &pool,
        &surge_domain::project::Project {
            id: "prj_p".into(),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            assigned_pipeline: None,
            pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
            surge_yaml_written: false,
            tracker: surge_domain::project::TrackerKind::None,
            branch_format: "task/{issue}".into(),
            created_at: 1,
        },
    )
    .await
    .unwrap();
    // A fresh materialization the run executes under — what the work-order
    // fetch reports back to the worker (INV-ID-1).
    surge_store::pipelines::insert_graph(
        &pool,
        &surge_domain::pipeline::Pipeline {
            id: "pl_p".into(),
            name: "p".into(),
            version: 1,
            content_hash: "sha256:pipe".into(),
            blessed: false,
            forked_from: None,
            created_at: 1,
        },
        &[],
        &[],
    )
    .await
    .unwrap();
    surge_store::materializations::insert_fresh_committed(
        &pool,
        &surge_domain::materialization::Materialization {
            id: "mat_p".into(),
            content_hash: "sha256:mat".into(),
            cache_key: "mk_mat_p".into(),
            pipeline_id: "pl_p".into(),
            project_id: "prj_p".into(),
            signed_by: "h".into(),
            fresh: true,
            created_at: 1,
        },
    )
    .await
    .unwrap();
    let mut issue = surge_domain::board::Issue {
        id: "iss_p".into(),
        project_id: "prj_p".into(),
        title: "Plugin fixture".into(),
        wave: 1,
        phase: "phase-0".into(),
        status: surge_domain::board::OrchestrationStatus::Eligible,
        work_order_hash: String::new(),
        gate2: surge_domain::board::Gate2State::Reviewed {
            by: "h".into(),
            at: 1,
        },
        lease: None,
        retry_count: 0,
        disposition: None,
        priority: 0,
        is_wave_integration: false,
        created_at: 1,
    };
    issue.work_order_hash = surge_compiler::work_order::work_order_hash(
        &surge_compiler::work_order::render_work_order(&issue),
    );
    surge_store::issues::insert(&pool, &issue).await.unwrap();
    surge_store::observatory::insert_run(
        &pool,
        &surge_domain::observatory::Run {
            id: "run_p".into(),
            project_id: "prj_p".into(),
            issue_id: Some("iss_p".into()),
            kind: surge_domain::observatory::RunKind::WorkOrder,
            materialization_hash: "sha256:mat".into(),
            work_order_hash: Some(issue.work_order_hash.clone()),
            status: surge_domain::observatory::RunStatus::Running,
            started_at: 1,
            ended_at: None,
            cost: 0.0,
        },
    )
    .await
    .unwrap();
    assert!(
        surge_store::issues::claim_lease(&pool, "iss_p", "worker-1", "run_p", 1_000, 600_000)
            .await
            .unwrap()
    );
    // The credential a worker actually gets: minted for this run and injected
    // at spawn (INV-AUTH-4), so the plugin exercises the run-bound scope the
    // heartbeat-hijack fix installed — its own run, its own issue's lease.
    let rt_token = surge_store::tokens::mint_for_run(
        &pool,
        TokenKind::Runtime,
        Some("prj_p"),
        Some("run_p"),
        1,
    )
    .await
    .unwrap();
    Live {
        state,
        api,
        rt_token,
    }
}

#[tokio::test]
async fn mcp_server_speaks_the_protocol_and_reaches_surge() {
    let live = live().await;
    let mut child = tokio::process::Command::new("node")
        .arg(plugin_dir().join("mcp/server.mjs"))
        .env("SURGE_API", &live.api)
        .env("SURGE_RUN_ID", "run_p")
        .env("SURGE_ISSUE_ID", "iss_p")
        .env("SURGE_RUNTIME_TOKEN", &live.rt_token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    async fn rpc_call(
        stdin: &mut tokio::process::ChildStdin,
        lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
        req: serde_json::Value,
    ) -> serde_json::Value {
        stdin
            .write_all((req.to_string() + "\n").as_bytes())
            .await
            .unwrap();
        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    macro_rules! rpc {
        ($req:expr) => {
            rpc_call(&mut stdin, &mut lines, $req).await
        };
    }

    // initialize
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}})
    );
    assert_eq!(r["result"]["serverInfo"]["name"], "surge");

    // tools/list — the four phase-0 tools, in INV-AUTH-1 capability order
    // (claim-lease is deliberately absent: a spawned worker never claims).
    let r = rpc!(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let names: Vec<&str> = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "surge_fetch_work_order",
            "surge_append_span",
            "surge_heartbeat",
            "surge_poll_run"
        ]
    );

    // capability 1 through the tool: the worker fetches its OWN work order —
    // rendered body, pinned hash, live lease, materialization hash (INV-ID-1).
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"surge_fetch_work_order","arguments":{}}})
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    let issue = surge_store::issues::get(&live.state.pool, "iss_p")
        .await
        .unwrap()
        .unwrap();
    assert!(text.contains("issue: iss_p"), "{text}");
    assert!(
        text.contains(&format!("work_order_hash: {}", issue.work_order_hash)),
        "{text}"
    );
    assert!(text.contains("materialization_hash: sha256:mat"), "{text}");
    assert!(text.contains("lease: worker-1"), "{text}");
    assert!(
        text.contains(&surge_compiler::work_order::render_work_order(&issue)),
        "the rendered work-order body reaches the worker verbatim: {text}"
    );

    // span-append lands in the store
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"surge_append_span","arguments":{"body":"hello from mcp","status":"ok"}}})
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let spans = surge_store::observatory::span_tree(&live.state.pool, "run_p")
        .await
        .unwrap();
    assert!(spans
        .iter()
        .any(|s| s.body.as_deref() == Some("hello from mcp")));

    // heartbeat moves the lease clock
    let before = surge_store::issues::get(&live.state.pool, "iss_p")
        .await
        .unwrap()
        .unwrap()
        .lease
        .unwrap()
        .expires_at;
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
        "params":{"name":"surge_heartbeat","arguments":{}}})
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let after = surge_store::issues::get(&live.state.pool, "iss_p")
        .await
        .unwrap()
        .unwrap()
        .lease
        .unwrap()
        .expires_at;
    assert!(after > before);

    // status poll: running → then aborted reads as a stop order
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
        "params":{"name":"surge_poll_run","arguments":{}}})
    );
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("running"));
    surge_store::observatory::abort_run(&live.state.pool, "run_p", 999)
        .await
        .unwrap();
    let r = rpc!(
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
        "params":{"name":"surge_poll_run","arguments":{}}})
    );
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ABORTED"),
        "{r}"
    );
}

async fn run_hook(script: &str, live: &Live, stdin_payload: Option<&str>) -> (i32, String) {
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.arg(plugin_dir().join(script))
        .env("SURGE_API", &live.api)
        .env("SURGE_RUN_ID", "run_p")
        .env("SURGE_ISSUE_ID", "iss_p")
        .env("SURGE_RUNTIME_TOKEN", &live.rt_token)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(p) = stdin_payload {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(p.as_bytes())
            .await
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().await.unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[tokio::test]
async fn fallback_hooks_guard_and_emit() {
    let live = live().await;

    // PostToolUse fallback: emits a span naming the tool.
    let (code, _) = run_hook("hooks/emit-span.sh", &live, Some(r#"{"tool_name":"Bash"}"#)).await;
    assert_eq!(code, 0);
    let spans = surge_store::observatory::span_tree(&live.state.pool, "run_p")
        .await
        .unwrap();
    assert!(spans
        .iter()
        .any(|s| s.body.as_deref() == Some("tool: Bash")));

    // PreToolUse guard: passes while running, heartbeats on the way through…
    let before = surge_store::issues::get(&live.state.pool, "iss_p")
        .await
        .unwrap()
        .unwrap()
        .lease
        .unwrap()
        .expires_at;
    tokio::time::sleep(Duration::from_millis(10)).await;
    let (code, _) = run_hook("hooks/poll-abort.sh", &live, None).await;
    assert_eq!(code, 0);
    let after = surge_store::issues::get(&live.state.pool, "iss_p")
        .await
        .unwrap()
        .unwrap()
        .lease
        .unwrap()
        .expires_at;
    assert!(after > before, "the guard heartbeats while it's here");

    // …and blocks with exit 2 once the abort ledger is written (§06).
    surge_store::observatory::abort_run(&live.state.pool, "run_p", 999)
        .await
        .unwrap();
    let (code, stderr) = run_hook("hooks/poll-abort.sh", &live, None).await;
    assert_eq!(code, 2, "exit 2 blocks the tool call");
    assert!(stderr.contains("aborted"), "{stderr}");
}
