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
        // Hermetic: the developer's global commit.gpgsign must not reach the
        // fixture — parallel signed commits exhaust gpg-agent and fail here.
        vec!["config", "commit.gpgsign", "false"],
    ] {
        assert!(std::process::Command::new("git")
            .arg("-C").arg(repo.path()).args(&args).status().unwrap().success());
    }
    std::fs::write(repo.path().join("README.md"), "fixture\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "init"]] {
        let out = std::process::Command::new("git")
            .arg("-C").arg(repo.path()).args(&args).output().unwrap();
        assert!(out.status.success(), "git {args:?} failed: {} {}",
            String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }

    // Worker: a script standing in for `claude -p` (the supervisor cares
    // about spawn/exit/lease mechanics, not what the worker thinks).
    let script = work.path().join("worker.sh");
    std::fs::write(&script, worker_script).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_base = format!("http://{}", listener.local_addr().unwrap());
    // A real extracted plugin tree: spawning verifies it (NEW-1), and it is a
    // constructor argument because the field has no default (N14) — there is
    // no `Default` to spread from, only `new`.
    let plugin_dir =
        surge_server::plugin_assets::extract_beside_db(&work.path().join("x.db")).unwrap();
    let cfg = SupervisorConfig {
        worker_cmd: vec!["/bin/sh".into(), script.to_string_lossy().into_owned()],
        lease_ttl_ms: 120_000,
        work_dir: work.path().join("worktrees"),
        api_base: api_base.clone(),
        poll_ms: 50,
        // No sweeper runs in this fixture unless a test starts one; the grace
        // is parked far away so it can never race a monitor under test.
        sweep_ms: 600_000,
        ..SupervisorConfig::new(plugin_dir)
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
    surge_store::materializations::insert_fresh_committed(&env.state.pool, &surge_domain::materialization::Materialization {
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

/// How long a barrier waits for a write that is already in flight before it
/// calls the supervisor broken. Generous on purpose: these tests run under
/// `cargo test`'s full thread fan-out, and the failure this bounds ("the
/// monitor never got there") is a real defect, not a slow machine. It is the
/// loudness bound on an already-correct wait, never the wait itself — every
/// barrier below polls a specific condition, so a longer budget can only turn
/// a spurious red into a pass, never a genuine red into a green.
const BARRIER_MS: u64 = 10_000;

/// Barrier on the RUN ROW, and nothing else.
///
/// `supervisor::monitor` writes the terminal run status FIRST and then, in
/// order, on the same task: revokes the runtime token, appends the run's end
/// span, resolves the worker's dangling spans, releases the lease (which is
/// also where the issue's verdict is written — one UPDATE, see
/// `store::issues::release_lease`), reaps the worktree, and audits
/// `run.finished`. Every one of those lands strictly AFTER this returns.
///
/// So this is the right barrier for asserting a run's own status and for
/// anything the worker itself wrote before exiting, and the WRONG barrier for
/// issue status, lease state, spans, tokens or worktree existence. Use
/// [`wait_lease_released`] or [`wait_reaped`] for those — reading them off
/// this one is the harness race that made this file flake under CPU load
/// (measured: 2 failures in 55 runs with eight-to-sixteen spinning processes
/// alongside; 10-20% where it was first reported), because the window between
/// the run row and the writes after it widens with contention.
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

/// Barrier on the monitor's issue-side writes: the lease release, the issue's
/// verdict, and — because they are awaited earlier on the same task —
/// the end span, the resolved dangling spans and the revoked runtime token.
///
/// The verdict rides in the same UPDATE as the lease release, so a null lease
/// is proof the status has been written too: `assert_eq!` the exact status
/// after this rather than polling for the status you hoped for, which would
/// turn a wrong verdict into a timeout instead of a value mismatch.
async fn wait_lease_released(env: &Env, issue_id: &str) {
    let deadline = std::time::Instant::now() + Duration::from_millis(BARRIER_MS);
    loop {
        let issue = surge_store::issues::get(&env.state.pool, issue_id).await.unwrap().unwrap();
        if issue.lease.is_none() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "lease never released on {issue_id} — still held by run {:?}, status {:?}",
            issue.lease.as_ref().map(|l| &l.run_id),
            issue.status
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Barrier on the reap, which is the monitor's last act before its audit row
/// — after the lease release, so [`wait_lease_released`] is not a barrier for
/// it either. A test that dispatched a real worker polls for the directory to
/// go rather than reading it the instant the run goes terminal.
async fn wait_reaped(dir: &std::path::Path) {
    let deadline = std::time::Instant::now() + Duration::from_millis(BARRIER_MS);
    while dir.exists() {
        assert!(std::time::Instant::now() < deadline, "worktree never reaped: {dir:?}");
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
         stdin=$(cat)\n\
         echo \"run=$SURGE_RUN_ID tok=${SURGE_RUNTIME_TOKEN%%${SURGE_RUNTIME_TOKEN#rt_}} plugin=$SURGE_PLUGIN_DIR\" > \"$OUT\"\n\
         echo \"$stdin\" | grep -q 'Work order — Fixture task' && echo stdin-workorder >> \"$OUT\"\n\
         echo \"$stdin\" | grep -q 'surge_fetch_work_order' && echo stdin-fetchtool >> \"$OUT\"\n\
         test -f .claude/settings.json && echo compiled >> \"$OUT\"\n\
         test -f \"work_orders/$SURGE_ISSUE_ID.md\" && echo workorder >> \"$OUT\"\n\
         git rev-parse --abbrev-ref HEAD >> \"$OUT\"\n\
         echo note > note.md\n\
         git add -A && git commit -qm 'did the work'\n\
         curl -sf -X POST \"$SURGE_API/runtime/runs/$SURGE_RUN_ID/spans\" \\\n\
           -H \"Authorization: Bearer $SURGE_RUNTIME_TOKEN\" -H 'Content-Type: application/json' \\\n\
           -d \"{\\\"id\\\":\\\"sp_w_$SURGE_RUN_ID\\\",\\\"run_id\\\":\\\"$SURGE_RUN_ID\\\",\\\"parent_span_id\\\":null,\\\"node_id\\\":null,\\\"role\\\":\\\"worker\\\",\\\"started_at\\\":1,\\\"duration_ms\\\":1,\\\"status\\\":\\\"ok\\\",\\\"cost\\\":0.0,\\\"depth\\\":0,\\\"policy_decision\\\":null,\\\"body\\\":\\\"worked\\\"}\" >/dev/null\n\
         exit 0\n",
    ).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // Pass the observation file path by writing it INTO the script, not by
    // exporting it from the test process. `std::env::set_var` mutates the
    // environment of a binary running twenty-odd tests across ten threads,
    // every one of which spawns processes that read that same environment —
    // a data race by construction, and one more way this file fails under
    // load. The script is the only reader, so it is the right place to put it.
    let observed = env.work.path().join("observed.txt");
    let script = env.work.path().join("worker.sh");
    let body = std::fs::read_to_string(&script).unwrap();
    std::fs::write(
        &script,
        body.replace("#!/bin/sh\n", &format!("#!/bin/sh\nOUT={}\n", observed.display())),
    )
    .unwrap();

    let out = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        other => panic!("expected spawn, got refusal: {:?}", matches!(other, _)),
    };
    assert_eq!(wait_terminal(&env, &out, 5_000).await, RunStatus::Succeeded);

    let observed = std::fs::read_to_string(&observed).unwrap();
    assert!(observed.contains("stdin-workorder"), "work order delivered on stdin (F2): {observed}");
    assert!(observed.contains("stdin-fetchtool"),
        "the prompt names surge_fetch_work_order, like the seeded implementer body: {observed}");
    assert!(observed.contains(&format!("run={out}")), "{observed}");
    assert!(observed.contains("plugin=/"), "SURGE_PLUGIN_DIR is absolute (F3): {observed}");
    assert!(observed.contains("tok=rt_"), "runtime token injected (INV-AUTH-4): {observed}");
    assert!(observed.contains("compiled"), "materialization compiled into worktree: {observed}");
    assert!(observed.contains("workorder"), "rendered work order present: {observed}");
    assert!(observed.contains("task/iss_1"), "on the task branch (INV-EXEC-2): {observed}");

    // Lease released, issue verified from the exit code (INV-EXEC-3),
    // worktree reaped (INV-EXEC-2), bound repo untouched beyond git metadata.
    wait_lease_released(&env, "iss_1").await;
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Verified);
    assert!(issue.lease.is_none());
    wait_reaped(&worktree_dir(&env)).await; // reaped at lease end (INV-EXEC-2)
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

/// ESC-2 / INV-ERR-1: the doc-run dispatch path makes the SAME freshness
/// check `dispatch_issue` does, and used to answer it with a bare `anyhow!`
/// that `human_api::internal` flattened into `500 {"error":"doc run dispatch
/// failed"}` — no run, no span, no audit entry, the reason on the server's
/// stderr only. A doc run has no issue and no work-order hash (the `run.kind`
/// CHECKs in 0002_object_model.sql), so the refusal record must be a valid
/// `doc` run row. The allowed side of this constraint is
/// `the_doc_run_prompt_is_scoped_to_the_doc_node`, which dispatches after a
/// compile.
#[tokio::test]
async fn doc_run_without_fresh_materialization_refuses_with_a_visible_run() {
    let env = setup("#!/bin/sh\nexit 0\n").await; // no compile → nothing fresh
    let (run_id, reason) =
        match surge_server::supervisor::dispatch_doc_run(&env.state, "prj_fix").await.unwrap() {
            surge_server::supervisor::DispatchOutcome::Refused { run_id, reason } => (run_id, reason),
            other => panic!("expected refusal, got {other:?}"),
        };
    assert!(reason.contains("compile first"), "{reason}");

    let run = surge_store::observatory::get_run(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(run.status, RunStatus::Refused);
    // A valid `doc` run row, not a work-order one wearing a doc label.
    assert_eq!(run.kind, surge_domain::observatory::RunKind::Doc);
    assert!(run.issue_id.is_none() && run.work_order_hash.is_none(),
        "a doc run carries neither an issue nor a work-order hash: {run:?}");
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(spans.len(), 1, "the refusal run carries one span with the reason (INV-ERR-1)");
    assert!(spans[0].body.as_deref().unwrap().contains("compile first"), "{spans:?}");
    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "dispatch.refused"
        && a.project_id.as_deref() == Some("prj_fix")
        && a.subject.contains("compile first")),
        "audited with its reason and its project (INV-ERR-1, N13): {audit:?}");
    // Nothing was spawned: the bound repo is untouched (INV-DATA-1).
    assert!(!env.repo.path().join(".claude").exists());
}

/// ESC-2 / INV-ERR-1 + INV-OBS-1: the interactive claim path (INV-AUTH-1's
/// fifth capability) refuses an uncompiled project with a bare 409 body — no
/// run, no span, no audit entry — while its own sibling branch (lease already
/// held, walk 4 S3) writes all three for the same endpoint. A refused claim is
/// both a refusal and a declined privileged act, so it owes both records. The
/// allowed side of this constraint is
/// `runtime_capabilities_fetch_claim_heartbeat`, which claims after a compile.
#[tokio::test]
async fn interactive_claim_without_fresh_materialization_refuses_visibly() {
    let env = setup("#!/bin/sh\nexit 0\n").await; // no compile → nothing fresh
    let issue = create_issue(&env).await;
    let rt = surge_store::tokens::rotate_project_runtime(
        &env.state.pool, "prj_fix", surge_server::now_ms())
        .await.unwrap().plaintext;

    let (code, body) =
        reqwest_lite(&format!("{}/runtime/issues/{}/claim", env.api_base, issue.id), &rt, true).await;
    assert_eq!(code, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("compile first"), "{body}");
    let run_id = v["run_id"].as_str().expect("the refusal names the run recording it").to_string();

    let run = surge_store::observatory::get_run(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(run.status, RunStatus::Refused);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(spans.len(), 1, "the refusal run carries one span with the reason (INV-ERR-1)");
    assert!(spans[0].body.as_deref().unwrap().contains("compile first"), "{spans:?}");
    // INV-OBS-1: the declined privileged act names itself and its claimant,
    // not `dispatch.refused`/`human` — this was not the supervisor dispatching.
    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "lease.claim_refused"
        && a.actor == "rt:prj_fix"
        && a.project_id.as_deref() == Some("prj_fix")
        && a.subject.contains("compile first")),
        "audited as a refused claim, with claimant and reason (INV-OBS-1): {audit:?}");
    // The refusal takes nothing: no lease moved, the issue stays claimable.
    let issue = surge_store::issues::get(&env.state.pool, &issue.id).await.unwrap().unwrap();
    assert!(issue.lease.is_none(), "a refused claim holds no lease");
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Eligible);
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
    // The reclaim span and the issue's verdict are both written after the run
    // row; the lease release is the last of the three.
    wait_lease_released(&env, "iss_1").await;
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("lease reclaimed")),
        "the reclaim reason is a visible record (§06-03): {spans:?}");
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    wait_reaped(&worktree_dir(&env)).await;
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
    wait_lease_released(&env, "iss_1").await;
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Aborted);
    wait_reaped(&worktree_dir(&env)).await;
    let _ = env.api_base; // live server held for the worker's polls
}

#[tokio::test]
async fn runtime_capabilities_fetch_claim_heartbeat() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // The interactive/project credential (unbound): claiming is what it is
    // for, and it is the one shape that may (INV-EXEC-1, F1).
    let rt = surge_store::tokens::rotate_project_runtime(
        &env.state.pool, "prj_fix", surge_server::now_ms())
        .await.unwrap().plaintext;
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

/// What the real plugin's MCP tool does, in shell: one worker span.
/// A worker that produced something and committed it on the task branch —
/// what "the worker did its job" means once the commit floor is in force
/// (R1). Fixtures whose subject is anything other than the floor itself use
/// this, so they exercise the shape a real work order has.
const COMMIT_WORK: &str = "echo note > note.md\ngit add -A && git commit -qm 'did the work'\n";

const SPAN_CURL: &str = "curl -sf -X POST \"$SURGE_API/runtime/runs/$SURGE_RUN_ID/spans\" \
 -H \"Authorization: Bearer $SURGE_RUNTIME_TOKEN\" -H 'Content-Type: application/json' \
 -d \"{\\\"id\\\":\\\"sp_w_$SURGE_RUN_ID\\\",\\\"run_id\\\":\\\"$SURGE_RUN_ID\\\",\\\"parent_span_id\\\":null,\\\"node_id\\\":null,\\\"role\\\":\\\"worker\\\",\\\"started_at\\\":1,\\\"duration_ms\\\":1,\\\"status\\\":\\\"ok\\\",\\\"cost\\\":0.0,\\\"depth\\\":0,\\\"policy_decision\\\":null,\\\"body\\\":\\\"worked\\\"}\" >/dev/null\n";

/// F1 regression: a RELATIVE work_dir (the shipped default's shape) must
/// resolve against the server's cwd — never against the bound repo — and
/// dispatch must work with the server's cwd nowhere near the repo.
#[tokio::test]
async fn relative_work_dir_resolves_outside_the_bound_repo() {
    let env = setup(&format!(
        "#!/bin/sh\ncat >/dev/null\n{SPAN_CURL}{COMMIT_WORK}exit 0\n"
    )).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // This is the one test whose work_dir lands under the crate directory
    // rather than in a tempdir — that is the point of it — so it owns the
    // cleanup. A drop guard, not a trailing statement: an assertion below
    // unwinds past a trailing statement and leaves `crates/server/
    // tmp-e2e-worktrees-<pid>/` behind, which is exactly what a failing run
    // used to do.
    struct ScratchDir(std::path::PathBuf);
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let rel = format!("tmp-e2e-worktrees-{}", std::process::id());
    let scratch = ScratchDir(std::path::absolute(&rel).unwrap());
    let mut cfg = (*env.state.supervisor).clone();
    cfg.work_dir = rel.clone().into(); // relative, like the default
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    let run_id = match surge_server::supervisor::dispatch_issue(&state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Succeeded);
    // The worktree never existed inside the bound repo (INV-DATA-1/EXEC-2).
    assert!(!env.repo.path().join(&rel).exists(), "worktree landed inside the bound repo");
    assert!(!env.repo.path().join("surge-worktrees").exists());
    // It lived under cwd and was reaped.
    wait_reaped(&scratch.0.join("prj_fix/iss_1")).await;
}

/// F4 regression (spawn half): a dispatch that fails after the lease claim
/// leaks nothing — run failed with a reason span, lease released, no worktree.
#[tokio::test]
async fn failed_spawn_releases_the_lease_and_reaps() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let mut cfg = (*env.state.supervisor).clone();
    cfg.worker_cmd = vec!["/nonexistent-worker-binary".into()];
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    assert!(surge_server::supervisor::dispatch_issue(&state, &issue.id).await.is_err());
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    assert!(issue.lease.is_none(), "lease released (F4)");
    assert!(!worktree_dir(&env).exists(), "worktree reaped (F4)");
    // The run is a visible failure with the reason, not an orphaned `running`.
    let runs = surge_store::observatory::list_runs(&env.state.pool, Some("prj_fix")).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Failed);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &runs[0].id).await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("worker spawn failed")));
}

/// F4 regression (observability half): a failing worker's stderr tail lands
/// in the failure span — refusals are data (INV-ERR-1).
#[tokio::test]
async fn worker_stderr_tail_is_a_visible_record() {
    let env = setup("#!/bin/sh\ncat >/dev/null\necho 'boom: mcp config invalid' >&2\nexit 3\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Failed);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
        if spans.iter().any(|s| {
            let b = s.body.as_deref().unwrap_or("");
            b.contains("exited with 3") && b.contains("boom: mcp config invalid")
        }) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "stderr tail never landed in a span: {spans:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// NEW-1 regression: a plugin dir without its MCP entry point must refuse the
/// spawn — a worker with no tools and no hooks runs blind, and `claude -p`
/// exits 0 regardless, which would report `verified` with nothing behind it.
#[tokio::test]
async fn missing_plugin_tree_refuses_the_spawn() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let empty = tempfile::tempdir().unwrap();
    let mut cfg = (*env.state.supervisor).clone();
    cfg.plugin_dir = empty.path().to_path_buf();
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    let err = surge_server::supervisor::dispatch_issue(&state, &issue.id).await.unwrap_err();
    assert!(err.to_string().contains("mcp/server.mjs"), "names the missing entry: {err}");
    // And it cleans up exactly like any other post-lease failure (F4).
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert!(issue.lease.is_none());
    assert!(!worktree_dir(&env).exists());
}

/// NEW-2 regression: exit 0 with no worker spans is not success — the worker
/// never reached Surge, and calling that `verified` hides it.
#[tokio::test]
async fn clean_exit_without_spans_is_not_verified() {
    let env = setup("#!/bin/sh\ncat >/dev/null\nexit 0\n").await; // never appends a span
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Failed);
    // The issue's verdict and the reason span both land after the run row.
    wait_lease_released(&env, "iss_1").await;
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("could not \\
                 reach Surge") || s.body.as_deref().unwrap_or("").contains("appended no spans")));
}

/// N6 regression: the lease-lost refusal is a visible record like every other
/// refusal — phase.md:43 promises a refusal run whose span carries the reason
/// (INV-ERR-1). This branch used to write the run and no span at all, so the
/// reason existed only in the HTTP response.
#[tokio::test]
async fn already_leased_refusal_carries_its_reason_span() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    // Someone else holds it already (an interactive session claimed it, §06).
    assert!(surge_store::issues::claim_lease(
        &env.state.pool, &issue.id, "someone-else", "run_elsewhere", 1, 60_000)
        .await.unwrap());

    let (run_id, reason) = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Refused { run_id, reason } => (run_id, reason),
        _ => panic!("expected refusal"),
    };
    assert!(reason.contains("not eligible or already leased"));
    let run = surge_store::observatory::get_run(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(run.status, RunStatus::Refused);
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert_eq!(spans.len(), 1, "the refusal run carries one span with the reason (N6)");
    assert!(spans[0].body.as_deref().unwrap().contains("already leased"), "{spans:?}");
    // The holder's lease is untouched by the refusal.
    let issue = surge_store::issues::get(&env.state.pool, &issue.id).await.unwrap().unwrap();
    assert_eq!(issue.lease.unwrap().run_id, "run_elsewhere");
}

/// N1 regression: a doc run (no issue, no lease, no worktree — design
/// §23-Fourteen) whose spawn fails goes through the same cleanup guard a
/// work-order dispatch does. It used to propagate with `?` after inserting
/// the run row, leaving a permanently `running` run and no audit entry.
#[tokio::test]
async fn failed_doc_run_spawn_leaves_no_running_run() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    let mut cfg = (*env.state.supervisor).clone();
    cfg.worker_cmd = vec!["/nonexistent-worker-binary".into()];
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    let err = surge_server::supervisor::dispatch_doc_run(&state, "prj_fix").await.unwrap_err();
    assert!(!err.to_string().is_empty());

    let runs = surge_store::observatory::list_runs(&env.state.pool, Some("prj_fix")).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Failed, "no run left `running` (N1)");
    assert!(runs[0].ended_at.is_some());
    let spans = surge_store::observatory::span_tree(&env.state.pool, &runs[0].id).await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("worker spawn failed")),
        "the failure reason is a visible record (INV-ERR-1): {spans:?}");
    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "dispatch.failed"
        && a.project_id.as_deref() == Some("prj_fix")),
        "dispatch.failed audited, carrying the project (N1, N13): {audit:?}");
}

/// The wreckage a SIGKILL used to leave: a `running` run, a held lease and a
/// worktree, all owned by a process that no longer exists.
async fn orphan_from_a_dead_process(env: &Env, run_id: &str, ttl_ms: i64) {
    let issue = create_issue(env).await;
    let mat = surge_store::materializations::fresh_for_project(&env.state.pool, "prj_fix")
        .await.unwrap().unwrap();
    surge_store::observatory::insert_run(&env.state.pool, &surge_domain::observatory::Run {
        id: run_id.into(),
        project_id: "prj_fix".into(),
        issue_id: Some(issue.id.clone()),
        kind: surge_domain::observatory::RunKind::WorkOrder,
        materialization_hash: mat.content_hash.clone(),
        work_order_hash: Some(issue.work_order_hash.clone()),
        status: RunStatus::Running,
        started_at: 1,
        ended_at: None,
        cost: 0.0,
    }).await.unwrap();
    // …including a span the worker opened and never closed (N4-residual).
    surge_store::observatory::append_span(&env.state.pool, &surge_domain::observatory::Span {
        id: format!("sp_open_{run_id}"),
        run_id: run_id.into(),
        parent_span_id: None,
        node_id: None,
        role: surge_domain::observatory::SpanRole::Worker,
        started_at: 1,
        duration_ms: None,
        status: surge_domain::observatory::SpanStatus::Running,
        cost: 0.0,
        depth: 0,
        policy_decision: None,
        body: Some("started the work".into()),
    }).await.unwrap();
    assert!(surge_store::issues::claim_lease(
        &env.state.pool, &issue.id, "worker-1", run_id, 1, ttl_ms).await.unwrap());
    // The worktree residue, created exactly where dispatch puts it.
    let wt = worktree_dir(env);
    std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
    let out = std::process::Command::new("git")
        .arg("-C").arg(env.repo.path())
        .args(["worktree", "add", "-B", "task/iss_1", wt.to_str().unwrap()])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// N2 regression (boot half): lease TTL used to be enforced only inside the
/// per-run monitor, which dies with the process — a SIGKILL mid-run left the
/// run `running` forever, the issue `leased`, the worktree on disk, and every
/// later dispatch refused, recoverable only by editing SQLite. A fresh
/// process now owns nothing it finds running.
#[tokio::test]
async fn boot_reconcile_terminalizes_orphaned_runs() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    orphan_from_a_dead_process(&env, "run_orphan", 600_000).await;

    assert_eq!(surge_server::supervisor::reconcile_orphans(&env.state).await.unwrap(), 1);

    let run = surge_store::observatory::get_run(&env.state.pool, "run_orphan").await.unwrap();
    assert_eq!(run.status, RunStatus::Failed, "terminalized, not left running (N2)");
    assert!(run.ended_at.is_some());
    let spans = surge_store::observatory::span_tree(&env.state.pool, "run_orphan").await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("")
        .contains("supervisor restarted while this run was in flight")),
        "the reason is a visible record (INV-ERR-1): {spans:?}");
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert!(issue.lease.is_none(), "lease released — the issue is not stuck `leased` (N2)");
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    assert!(!worktree_dir(&env).exists(), "worktree residue reaped (INV-EXEC-2)");
    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "run.reconciled"
        && a.project_id.as_deref() == Some("prj_fix")), "{audit:?}");
    // The worker's dangling span is resolved too (N4-residual): its worker is
    // gone, so nothing will ever close it.
    let open = spans.iter().find(|s| s.id == "sp_open_run_orphan").expect("worker span kept");
    assert_ne!(open.status, surge_domain::observatory::SpanStatus::Running);
    assert!(open.policy_decision.as_deref().unwrap_or("").contains("never reported completion"));
    assert_eq!(open.body.as_deref(), Some("started the work"), "the worker's own record is kept");

    // Idempotent: a second boot — or the shutdown drain — adds nothing.
    assert_eq!(surge_server::supervisor::reconcile_orphans(&env.state).await.unwrap(), 0);
    assert_eq!(surge_server::supervisor::drain_on_shutdown(
        &env.state, Duration::from_millis(50)).await, 0);
    let after = surge_store::observatory::span_tree(&env.state.pool, "run_orphan").await.unwrap();
    assert_eq!(after.len(), spans.len(), "no second reason span");
    assert_eq!(
        surge_store::observatory::get_run(&env.state.pool, "run_orphan").await.unwrap().ended_at,
        run.ended_at, "the first verdict stands");
}

/// N2 regression (sweeper half): TTL enforced with no monitor in existence at
/// all — the case a per-run watchdog structurally cannot cover.
#[tokio::test]
async fn sweeper_reclaims_a_lease_no_monitor_is_watching() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    // TTL 1ms from epoch: expired long before this test started.
    orphan_from_a_dead_process(&env, "run_orphan", 1).await;
    let mut cfg = (*env.state.supervisor).clone();
    cfg.sweep_ms = 10; // also the grace a live monitor would win inside
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    assert_eq!(surge_server::supervisor::sweep_expired_leases(&state).await, 1);

    let run = surge_store::observatory::get_run(&env.state.pool, "run_orphan").await.unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    let spans = surge_store::observatory::span_tree(&env.state.pool, "run_orphan").await.unwrap();
    assert!(spans.iter().any(|s| s.body.as_deref().unwrap_or("").contains("lease reclaimed by the sweeper")),
        "{spans:?}");
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert!(issue.lease.is_none());
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Failed);
    assert!(!worktree_dir(&env).exists(), "worktree reaped (INV-EXEC-2)");
    // Nothing left to sweep.
    assert_eq!(surge_server::supervisor::sweep_expired_leases(&state).await, 0);
}

/// N2 regression: a lease that outlived its already-terminal run is reclaimed
/// too — that combination is what made an issue permanently undispatchable
/// ("not eligible or already leased", forever).
#[tokio::test]
async fn sweeper_releases_a_lease_whose_run_is_already_terminal() {
    let env = setup("#!/bin/sh\nexit 0\n").await;
    compile(&env).await;
    orphan_from_a_dead_process(&env, "run_orphan", 1).await;
    // The run reached a terminal state; only the lease was left behind.
    assert!(surge_store::observatory::finish_run_if_running(
        &env.state.pool, "run_orphan", RunStatus::Failed, 2).await.unwrap());
    let mut cfg = (*env.state.supervisor).clone();
    cfg.sweep_ms = 10;
    let state = AppState::with_supervisor(env.state.pool.clone(), cfg);

    assert_eq!(surge_server::supervisor::sweep_expired_leases(&state).await, 1);

    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert!(issue.lease.is_none(), "the stranded lease is released (N2)");
    assert!(!worktree_dir(&env).exists());
    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "lease.reclaimed"
        && a.project_id.as_deref() == Some("prj_fix")), "{audit:?}");
}

/// N4-residual regression, live: walk 3 found spans stuck `running` forever
/// on runs that had already succeeded — a worker opened a "start" span the
/// tool schema let it leave open, and nothing ever resolved it. Closing it is
/// not a capability the runtime has (INV-AUTH-1's five, deliberately), so the
/// supervisor resolves it from the exit it observed (INV-EXEC-3).
#[tokio::test]
async fn a_span_the_worker_never_closed_is_resolved_at_run_end() {
    let env = setup(
        "#!/bin/sh\n\
         cat >/dev/null\n\
         curl -sf -X POST \"$SURGE_API/runtime/runs/$SURGE_RUN_ID/spans\" \\\n\
           -H \"Authorization: Bearer $SURGE_RUNTIME_TOKEN\" -H 'Content-Type: application/json' \\\n\
           -d \"{\\\"id\\\":\\\"sp_open_$SURGE_RUN_ID\\\",\\\"run_id\\\":\\\"$SURGE_RUN_ID\\\",\\\"parent_span_id\\\":null,\\\"node_id\\\":null,\\\"role\\\":\\\"worker\\\",\\\"started_at\\\":1,\\\"duration_ms\\\":null,\\\"status\\\":\\\"running\\\",\\\"cost\\\":0.0,\\\"depth\\\":0,\\\"policy_decision\\\":null,\\\"body\\\":\\\"started the work\\\"}\" >/dev/null\n\
         echo note > note.md\n\
         git add -A && git commit -qm 'did the work'\n\
         exit 0\n",
    ).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Succeeded);

    // The lease release is the monitor's last write; wait for it so the span
    // resolution that precedes it has certainly happened.
    wait_lease_released(&env, "iss_1").await;
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(!spans.is_empty());
    assert!(
        !spans.iter().any(|s| s.status == surge_domain::observatory::SpanStatus::Running),
        "no span is left running once the run is terminal (N4-residual): {spans:?}"
    );
    let open = spans.iter().find(|s| s.id == format!("sp_open_{run_id}")).expect("worker span");
    assert!(open.policy_decision.as_deref().unwrap_or("").contains("never reported completion"),
        "the resolution says why, in the field that survives compaction (INV-OBS-2): {open:?}");
}

/// N2's other half: reconciling a run must not leave its issue a dead end.
/// A failed issue can be put back in the eligible column and dispatched
/// again — recovery is a product action, not a SQL edit.
#[tokio::test]
async fn a_failed_issue_can_be_retried_and_redispatched() {
    let env = setup("#!/bin/sh\ncat >/dev/null\nexit 7\n").await; // fails
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Failed);
    // The verdict lands after the run row — and reading it early would also
    // make the refusal below pass for the wrong reason (refused as `leased`
    // rather than as `failed`).
    wait_lease_released(&env, "iss_1").await;
    let failed = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(failed.status, surge_domain::board::OrchestrationStatus::Failed);
    // A failed issue is not dispatchable...
    assert!(matches!(
        surge_server::supervisor::dispatch_issue(&env.state, "iss_1").await.unwrap(),
        surge_server::supervisor::DispatchOutcome::Refused { .. }
    ));
    // ...until it is retried, which is an ordinary human action.
    assert!(surge_store::issues::mark_eligible_again(&env.state.pool, "iss_1").await.unwrap());
    let retried = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(retried.status, surge_domain::board::OrchestrationStatus::Eligible);
    assert_eq!(retried.retry_count, failed.retry_count + 1, "the count is on the card (§06)");

    // The redispatch reuses the failed run's worktree path, and the reap is
    // the monitor's LAST act — later than the lease release this test already
    // waited on. Redispatching before it lands makes `git worktree add` fail
    // on a directory that is about to be deleted.
    wait_reaped(&worktree_dir(&env)).await;
    // The redispatched worker stays alive, so the lease it takes is still
    // held when the guard below reads it. The `exit 7` worker above would
    // race that read to its own exit — the monitor releases the lease the
    // moment it observes the process gone, and under load it wins. Holding
    // the lease for the fixture's 120s TTL makes the state durable rather
    // than making the test wait for a window that may already have closed.
    let slow = env.work.path().join("slow-worker.sh");
    std::fs::write(&slow, "#!/bin/sh\ncat >/dev/null\nsleep 30\n").unwrap();
    let mut cfg = (*env.state.supervisor).clone();
    cfg.worker_cmd = vec!["/bin/sh".into(), slow.to_string_lossy().into_owned()];
    let held = AppState::with_supervisor(env.state.pool.clone(), cfg);
    assert!(matches!(
        surge_server::supervisor::dispatch_issue(&held, "iss_1").await.unwrap(),
        surge_server::supervisor::DispatchOutcome::Spawned { .. }
    ));
    // Guard: verified and leased issues are never retryable. The release is
    // run-guarded now (concurrency review), so it needs the holder's run id.
    let holder = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap()
        .lease.expect("the redispatch holds the lease").run_id;
    assert!(surge_store::issues::release_lease(
        &env.state.pool, "iss_1", &holder, surge_domain::board::OrchestrationStatus::Verified)
        .await
        .unwrap());
    assert!(!surge_store::issues::mark_eligible_again(&env.state.pool, "iss_1").await.unwrap());
}

/// S2: a runtime credential must not outlive its run. It stays live while the
/// worker runs (an abort has to reach it through a status poll), and is dead
/// once the supervisor has observed the process gone.
#[tokio::test]
async fn a_runtime_token_dies_with_its_run() {
    let env = setup(&format!(
        "#!/bin/sh\ncat >/dev/null\n{SPAN_CURL}{COMMIT_WORK}exit 0\n"
    )).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let live = |pool: sqlx::SqlitePool| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM token WHERE kind = 'runtime' AND revoked_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    assert_eq!(live(env.state.pool.clone()).await, 0);
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Succeeded);
    // The monitor revokes after observing the exit; give it a beat.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while live(env.state.pool.clone()).await > 0 {
        assert!(std::time::Instant::now() < deadline, "runtime token outlived its run");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The binding is what makes that possible at all.
    let bound: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM token WHERE run_id = ?")
        .bind(&run_id)
        .fetch_one(&env.state.pool)
        .await
        .unwrap();
    assert_eq!(bound, 1, "the credential is bound to its run");
}

/// F6: a `succeeded` doc run held a `refused` span, because the prompt said
/// "run the doc pipeline compiled into .claude/" and the compiled tree
/// carries every node kind's agents — so the worker also attempted the agent
/// node and self-reported a refusal. The Observatory rendered a green run
/// with a red row in it. The prompt is scoped to the doc node now, and says
/// nothing about work orders or heartbeats: a doc run holds no issue and no
/// lease (design §23-Fourteen).
#[tokio::test]
async fn the_doc_run_prompt_is_scoped_to_the_doc_node() {
    // The worker runs in the bound repo itself; it captures its own prompt.
    let env = setup("#!/bin/sh\ncat > prompt.txt\nexit 0\n").await;
    compile(&env).await;
    let run_id = match surge_server::supervisor::dispatch_doc_run(&env.state, "prj_fix").await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        other => panic!("expected spawn, got {other:?}"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Succeeded);

    let prompt = std::fs::read_to_string(env.repo.path().join("prompt.txt")).unwrap();
    assert!(prompt.contains("ONLY the doc node"), "scoped to the doc node (F6): {prompt}");
    assert!(prompt.contains("do not attempt"), "the other nodes are named and excluded: {prompt}");
    assert!(!prompt.contains("surge_heartbeat"), "a doc run holds no lease to beat: {prompt}");
    assert!(!prompt.contains("surge_fetch_work_order"), "and no issue to fetch for: {prompt}");
}

/// S4: abort is the one human-initiated destructive act, and it wrote no span
/// — the observatory showed a stopped run with no stated reason.
#[tokio::test]
async fn abort_writes_a_reason_span_and_audits_the_project() {
    let env = setup("#!/bin/sh\nsleep 30\n").await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    let resp = surge_server::supervisor::abort_run(&env.state, &run_id).await;
    assert!(resp, "abort landed");
    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(
        spans.iter().any(|s| s.id.starts_with("sp_abort_")
            && s.policy_decision.as_deref().unwrap_or("").contains("next tool call")),
        "the abort states its reason: {spans:?}"
    );
    let audited = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    let row = audited.iter().find(|a| a.action == "run.aborted").expect("run.aborted audited");
    assert_eq!(row.project_id.as_deref(), Some("prj_fix"), "filterable by project (N13)");
}

/// Read git state from the bound repo the way the assertions below need it.
fn git_out(repo: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new("git")
        .arg("-C").arg(repo).args(args).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// R1 (smoke walk 6): the observability floor asked "were there any spans at
/// all", never "did anything come of them". Walk 6's evidence was a task
/// branch with no commit on it — `docs/note_C.md` was never created — while
/// Surge recorded `succeeded` + `verified` and reaped the worktree, so the
/// evidence of non-work left disk too.
///
/// The repro is the real shape: exit 0, spans present (so the NEW-2 count
/// floor is satisfied and provably not what fails this run), no commit.
#[tokio::test]
async fn a_clean_exit_with_no_commit_is_not_verified() {
    let env = setup(&format!("#!/bin/sh\ncat >/dev/null\n{SPAN_CURL}exit 0\n")).await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let base = git_out(env.repo.path(), &["rev-parse", "HEAD"]).1;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(wait_terminal(&env, &run_id, 5_000).await, RunStatus::Failed);

    wait_lease_released(&env, "iss_1").await;
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(
        issue.status,
        surge_domain::board::OrchestrationStatus::Failed,
        "a run with nothing on its task branch does not verify its issue"
    );

    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(
        spans.iter().any(|s| s.id == format!("sp_w_{run_id}")),
        "the worker's span is still there — the NEW-2 count floor was satisfied: {spans:?}"
    );
    assert!(
        spans.iter().any(|s| s.id == format!("sp_end_{run_id}")
            && s.body.as_deref().unwrap_or("").contains("no commit")),
        "the run states why it failed, in a span (INV-ERR-1): {spans:?}"
    );

    // The floor's premise, pinned: walk 6 observed that task branches survive
    // the reap, which is what lets the verdict be read from the bound repo's
    // ref store rather than from a directory that is about to be deleted.
    // If a reap ever started deleting the branch, this goes red.
    wait_reaped(&worktree_dir(&env)).await;
    let (ok, head) = git_out(env.repo.path(), &["rev-parse", "--verify", "task/iss_1"]);
    assert!(ok, "the task branch outlives the worktree");
    assert_eq!(head, base, "and still sits on the commit it was created from");
}

/// The false-failure guard. `crates/server/seed/implementer.md` tells every
/// worker to append one finished span per unit of work "carrying the status
/// it earned", and `doc-writer.md` promises that "an honest `error` span
/// costs you nothing"; the plugin's own tool description says spans are
/// "observability, never control flow (INV-EXEC-3)".
///
/// So a worker that hits a problem at one unit, says so honestly, fixes it
/// and commits has succeeded — and must be recorded that way. If this test
/// ever goes red, someone has re-introduced span-content control flow and
/// broken the contract that buys honest span content.
#[tokio::test]
async fn an_honest_error_span_with_a_commit_still_verifies() {
    let env = setup(&format!(
        "#!/bin/sh\n\
         cat >/dev/null\n\
         {SPAN_CURL}\
         curl -sf -X POST \"$SURGE_API/runtime/runs/$SURGE_RUN_ID/spans\" \\\n\
           -H \"Authorization: Bearer $SURGE_RUNTIME_TOKEN\" -H 'Content-Type: application/json' \\\n\
           -d \"{{\\\"id\\\":\\\"sp_verify_$SURGE_RUN_ID\\\",\\\"run_id\\\":\\\"$SURGE_RUN_ID\\\",\\\"parent_span_id\\\":null,\\\"node_id\\\":null,\\\"role\\\":\\\"worker\\\",\\\"started_at\\\":2,\\\"duration_ms\\\":1,\\\"status\\\":\\\"error\\\",\\\"cost\\\":0.0,\\\"depth\\\":0,\\\"policy_decision\\\":null,\\\"body\\\":\\\"verify unit failed — tests red, fixing\\\"}}\" >/dev/null\n\
         {COMMIT_WORK}\
         exit 0\n"
    ))
    .await;
    compile(&env).await;
    let issue = create_issue(&env).await;
    let run_id = match surge_server::supervisor::dispatch_issue(&env.state, &issue.id).await.unwrap() {
        surge_server::supervisor::DispatchOutcome::Spawned { run_id } => run_id,
        _ => panic!("expected spawn"),
    };
    assert_eq!(
        wait_terminal(&env, &run_id, 5_000).await,
        RunStatus::Succeeded,
        "an honest mid-run error span is observability, not a verdict (INV-EXEC-3)"
    );

    wait_lease_released(&env, "iss_1").await;
    let issue = surge_store::issues::get(&env.state.pool, "iss_1").await.unwrap().unwrap();
    assert_eq!(issue.status, surge_domain::board::OrchestrationStatus::Verified);

    let spans = surge_store::observatory::span_tree(&env.state.pool, &run_id).await.unwrap();
    assert!(
        spans.iter().any(|s| s.id == format!("sp_verify_{run_id}")
            && s.status == surge_domain::observatory::SpanStatus::Error),
        "the honest error span is kept, exactly as written (INV-OBS-2): {spans:?}"
    );
    let (ok, count) = git_out(env.repo.path(), &["rev-list", "--count", "task/iss_1"]);
    assert!(ok);
    assert_eq!(count, "2", "the work landed as a commit on the task branch");
}

