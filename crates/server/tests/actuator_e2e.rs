//! The phase-0 riskiest assumption, proven end to end with real processes:
//! dispatch → worktree on the task branch → compiled materialization + work
//! order inside it → spawned worker with env-injected token (INV-AUTH-4) →
//! exit-code-derived state (INV-EXEC-3) → worktree reaped (INV-EXEC-2).
//! Plus: refusal runs (INV-ERR-1), lease reclaim on silence, and an abort
//! landing at the worker's next status poll over live HTTP (§06).

use std::time::Duration;
use surge_domain::observatory::RunStatus;
use surge_server::supervisor::SupervisorConfig;
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;

struct Env {
    state: AppState,
    #[allow(dead_code)]
    session: String,
    repo: tempfile::TempDir,
    work: tempfile::TempDir,
    api_base: String,
}

/// Bound git repo + fixture graph + trusted library + fresh materialization +
/// one eligible issue, with a live server on an ephemeral loopback port.
async fn setup(worker_script: &str) -> Env {
    let pool = surge_store::open_in_memory().await.unwrap();
    let repo = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();

    // A real git repo with an initial commit (worktrees need one).
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        assert!(std::process::Command::new("git")
            .arg("-C").arg(repo.path()).args(&args).status().unwrap().success());
    }
    std::fs::write(repo.path().join("README.md"), "fixture\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "init"]] {
        assert!(std::process::Command::new("git")
            .arg("-C").arg(repo.path()).args(&args).status().unwrap().success());
    }

    // Worker: a script standing in for `claude -p` (the supervisor cares
    // about spawn/exit/lease mechanics, not what the worker thinks).
    let script = work.path().join("worker.sh");
    std::fs::write(&script, worker_script).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_base = format!("http://{}", listener.local_addr().unwrap());
    let cfg = SupervisorConfig {
        worker_cmd: vec!["/bin/sh".into(), script.to_string_lossy().into_owned()],
        lease_ttl_ms: 120_000,
        work_dir: work.path().join("worktrees"),
        api_base: api_base.clone(),
        poll_ms: 50,
    };
    let state = AppState::with_supervisor(pool.clone(), cfg);
    let router = app(state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();
    let project = surge_domain::project::Project {
        id: "prj_fix".into(),
        name: "fixture".into(),
        repo_path: repo.path().to_string_lossy().into_owned(),
        assigned_pipeline: None,
        pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: surge_domain::project::TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: 1,
    };
    surge_store::projects::insert(&pool, &project).await.unwrap();
    let (p, n, e) = surge_domain::fixtures::two_node_pipeline();
    surge_store::pipelines::insert_graph(&pool, &p, &n, &e).await.unwrap();
    for (kind, name) in [
        (surge_domain::library::LibraryItemKind::Subagent, "doc-writer"),
        (surge_domain::library::LibraryItemKind::Skill, "write-summary"),
        (surge_domain::library::LibraryItemKind::Subagent, "implementer"),
    ] {
        surge_store::library::insert(&pool, &surge_domain::library::LibraryItem {
            id: format!("li_{name}"),
            kind,
            name: name.into(),
            version: 1,
            body: format!("# {name}"),
            trust: surge_domain::library::TrustState::Local,
            created_at: 1,
        }).await.unwrap();
    }
    Env { state, session, repo, work, api_base }
}

async fn compile(env: &Env) {
    let (p, n, e) = surge_domain::fixtures::two_node_pipeline();
    let mut lib = surge_compiler::LibraryIndex::new();
    for (kind, r) in surge_compiler::referenced_items(&n) {
        let item = surge_store::library::get(&env.state.pool, kind, &r.name, r.version)
            .await.unwrap().unwrap();
        lib.insert((kind, r.name.clone(), r.version), item);
    }
    let project = surge_store::projects::get(&env.state.pool, "prj_fix").await.unwrap().unwrap();
    let compiled = surge_compiler::compile(&p, &n, &e, &lib, &project).unwrap();
    surge_store::materializations::insert_fresh(&env.state.pool, &surge_domain::materialization::Materialization {
        id: compiled.cache_key.clone(),
        content_hash: compiled.materialization_hash.clone(),
        cache_key: compiled.cache_key.clone(),
        pipeline_id: p.id.clone(),
        project_id: "prj_fix".into(),
        signed_by: "st".into(),
        fresh: true,
        created_at: 1,
    }).await.unwrap();
}

async fn create_issue(env: &Env) -> surge_domain::board::Issue {
    let mut issue = surge_domain::board::Issue {
        id: "iss_1".into(),
        project_id: "prj_fix".into(),
        title: "Fixture task".into(),
        wave: 1,
        phase: "phase-0".into(),
        status: surge_domain::board::OrchestrationStatus::Eligible,
        work_order_hash: String::new(),
        gate2: surge_domain::board::Gate2State::Reviewed { by: "h".into(), at: 1 },
        lease: None,
        retry_count: 0,
        disposition: None,
        priority: 0,
        is_wave_integration: false,
        created_at: 1,
    };
    issue.work_order_hash = surge_compiler::work_order::work_order_hash(
        &surge_compiler::work_order::render_work_order(&issue));
    surge_store::issues::insert(&env.state.pool, &issue).await.unwrap();
    issue
}

async fn wait_terminal(env: &Env, run_id: &str, timeout_ms: u64) -> RunStatus {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let run = surge_store::observatory::get_run(&env.state.pool, run_id).await.unwrap();
        if run.status != RunStatus::Running {
            return run.status;
        }
        assert!(std::time::Instant::now() < deadline, "run never left running");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn worktree_dir(env: &Env) -> std::path::PathBuf {
    env.work.path().join("worktrees/prj_fix/iss_1")
}

#[tokio::test]
async fn dispatch_runs_a_worker_in_a_reaped_worktree() {
    // The worker proves it ran inside a compiled worktree with the token in
    // its env by writing what it sees into a file OUTSIDE the worktree.
    let env = setup(
        "#!/bin/sh\n\
         echo \"wo=$1 run=$SURGE_RUN_ID tok=${SURGE_RUNTIME_TOKEN%%${SURGE_RUNTIME_TOKEN#rt_}} pwd=$PWD\" > \"$OUT\"\n\
         test -f .claude/settings.json && echo compiled >> \"$OUT\"\n\
         test -f \"$1\" && echo workorder >> \"$OUT\"\n\
         git rev-parse --abbrev-ref HEAD >> \"$OUT\"\n\
         exit 0\n",
    ).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // Pass the observation file path through the environment the script inherits.
    std::env::set_var("OUT", env.work.path().join("observed.txt"));

    let out = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        other => panic!("expected spawn, got refusal: {:?}", matches!(other, _)),
    };
    assert_eq!(wait_terminal(&env, &out, 5_000).await, RunStatus::Succeeded);

    let observed = std::fs::read_to_string(env.work.path().join("observed.txt")).unwrap();
    assert!(observed.contains("wo=work_orders/iss_1.md"), "{observed}");
    assert!(observed.contains(&format!("run={out}")), "{observed}");
    assert!(observed.contains("tok=rt_"), "runtime token injected (INV-AUTH-4): {observed}");
    assert!(observed.contains("compiled"), "materialization compiled into worktree: {observed}");
    assert!(observed.contains("workorder"), "rendered work order present: {observed}");
    assert!(observed.contains("task/iss_1"), "on the task branch (INV-EXEC-2): {observed}");

    // Lease released, issue verified from the exit code (INV-EXEC-3),
    // worktree reaped (INV-EXEC-2), bound repo untouched beyond git metadata.
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Verified);
    assert!(issue.lease.is_none());
    assert!(!worktree_dir(&env).exists(), "worktree reaped at lease end");
    assert!(!env.repo.path().join(".claude").exists(), "bound repo untouched (INV-DATA-1)");
}

#[tokio::test]
async fn stale_materialization_refuses_with_a_visible_run() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    let issue = create_issue(&env).await; // no compile → no fresh materialization
    let (run_id, reason) = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Refused { run_id, reason } => (run_id, reason),
        _ => panic!("expected refusal"),
    };
    assert!(reason.contains("compile first"));
    let run = surge_store::observatory::get_run(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(run.status, RunStatus::Refused);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(spans.len(), 1, "the refusal run carries one span with the reason (INV-ERR-1)");
    assert!(spans[0].body.as_deref().unwrap().contains("compile first"));
}

#[tokio::test]
async fn silent_worker_is_reclaimed_at_ttl() {
    let env = setup("#!/bin/sh\nsleep 30\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // Tight TTL for the test: no heartbeats arrive, so the clock runs out.
    let mut cfg = (*env.state.supervisor).clone();
    cfg.lease_ttl_ms = 300;
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    let run_id = match surge_server::supervisor::dispatch_issue(&state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Failed);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("lease reclaimed")),
        "the reclaim reason is a visible record (§06-03)");
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    assert!(!worktree_dir(&env).exists());
}

#[tokio::test]
async fn abort_lands_at_the_workers_next_status_poll() {
    // The worker polls its own run status over live HTTP (capability 5) and
    // exits when it sees aborted — the §06 abort semantics, for real.
    let env = setup(
        "#!/bin/sh\n\
         for i in $(seq 1 100); do\n\
           s=$(curl -s -H \"Authorization: Bearer $SURGE_RUNTIME_TOKEN\" \"$SURGE_API/runtime/runs/$SURGE_RUN_ID\")\n\
           echo \"$s\" | grep -q aborted && exit 7\n\
           sleep 0.1\n\
         done\n\
         exit 0\n",
    ).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };

    // Give the worker a beat to start polling, then write the abort ledger.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(surge_store::observatory::abort_run(&env.state.pool, &run_id, 999).await.unwrap());

    assert_eq!(wait_terminal(&env, &run_id, 8_000).await, RunStatus::Aborted,
        "the abort stands even after the process exits (guarded transition)");
    // The ledger write is immediate; the lease releases when the monitor
    // observes the worker's exit — wait for that separately.
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let issue = loop {
        let i = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
        if i.lease.is_none() {
            break i;
        }
        assert!(std::time::Instant::now() < deadline, "lease never released after abort");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Aborted);
    assert!(!worktree_dir(&env).exists());
    let _ = env.api_base; // live server held for the worker's polls
}

#[tokio::test]
async fn runtime_capabilities_fetch_claim_heartbeat() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let rt = surge_store::tokens::mint(&env.state.pool, TokenKind::Runtime, Some("prj_fix"), 1)
        .await.unwrap();
    let client = |path: &str, post: bool| {
        let url = format!("{}{}", env.api_base, path);
        let tok = rt.clone();
        async move {
            let c = reqwest_lite(&url, &tok, post).await;
            c
        }
    };
    // Capability 1: fetch work order + lease + materialization hash.
    let (code, body) = client(&format!("/runtime/issues/{}/work-order", issue.id), false).await;
    assert_eq!(code, 200);
    assert!(body.contains("Work order — Fixture task") && body.contains("sha256:"), "{body}");
    // Capability 2: claim. Second claim loses (one claimant wins).
    let (code, body) = client(&format!("/runtime/issues/{}/claim", issue.id), true).await;
    assert_eq!(code, 200, "{body}");
    let (code, _) = client(&format!("/runtime/issues/{}/claim", issue.id), true).await;
    assert_eq!(code, 409);
    // Capability 3: heartbeat extends the lease.
    let before = surge_store::issues::get(&env.state.pool, &issue.id).await.unwrap().unwrap()
        .lease.unwrap().expires_at;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let (code, _) = client(&format!("/runtime/issues/{}/heartbeat", issue.id), true).await;
    assert_eq!(code, 200);
    let after = surge_store::issues::get(&env.state.pool, &issue.id).await.unwrap().unwrap()
        .lease.unwrap().expires_at;
    assert!(after > before, "heartbeat moved the lease clock");
}

/// Minimal HTTP client (no reqwest dep): shell out to curl like the workers do.
async fn reqwest_lite(url: &str, token: &str, post: bool) -> (u16, String) {
    let mut cmd = tokio::process::Command::new("curl");
    cmd.arg("-s").arg("-w").arg("\n%{http_code}")
        .arg("-H").arg(format!("Authorization: Bearer {token}"));
    if post {
        cmd.arg("-X").arg("POST");
    }
    let out = cmd.arg(url).output().await.unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap();
    (code.trim().parse().unwrap(), body.to_string())
}
