//! What a runtime credential may reach, and how long it lives.
//!
//! Smoke walk 5 and two security reviews found the same shape three times: a
//! credential whose scope is "the project" and whose lifetime is "forever".
//! F1 (an unbound token minted by the human endpoint, revocable by nothing),
//! the heartbeat hijack (worker A refreshing worker B's lease until the issue
//! is unrecoverable), terminal-run span appends, and the reserved span-id
//! namespace are all covered here — plus F4, the last spanless *and*
//! auditless refusal in the product.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use surge_domain::board::{Gate2State, Issue, OrchestrationStatus};
use surge_domain::observatory::{Run, RunKind, RunStatus};
use surge_server::supervisor::SupervisorConfig;
use surge_server::{app, now_ms, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

fn req(method: &str, uri: &str, token: Option<&str>, body: Option<serde_json::Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

async fn audit_actions(pool: &sqlx::SqlitePool) -> Vec<String> {
    surge_store::audit::recent(pool, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect()
}

fn span(id: &str, run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "run_id": run_id, "parent_span_id": null, "node_id": null,
        "role": "worker", "started_at": 1, "duration_ms": 1, "status": "ok",
        "cost": 0.0, "depth": 0, "policy_decision": null, "body": "worked"
    })
}

struct Env {
    state: AppState,
    session: String,
    /// Kept alive: the sweeper resolves worktree paths under it.
    _work: tempfile::TempDir,
}

/// One project, two issues, two running runs — the shape the hijack needs:
/// two workers in the same project, each with its own lease.
async fn setup() -> Env {
    let pool = surge_store::open_in_memory().await.unwrap();
    let work = tempfile::tempdir().unwrap();
    let cfg = SupervisorConfig {
        work_dir: work.path().join("worktrees"),
        sweep_ms: 10,
        ..SupervisorConfig::unconfigured()
    };
    let state = AppState::with_supervisor(pool.clone(), cfg);
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();

    for id in ["prj_a", "prj_other"] {
        surge_store::projects::insert(&pool, &surge_domain::project::Project {
            id: id.into(),
            name: id.into(),
            repo_path: "/tmp".into(),
            assigned_pipeline: None,
            pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
            surge_yaml_written: false,
            tracker: surge_domain::project::TrackerKind::None,
            branch_format: "task/{issue}".into(),
            created_at: 1,
        })
        .await
        .unwrap();
    }
    for id in ["iss_a", "iss_b"] {
        surge_store::issues::insert(&pool, &Issue {
            id: id.into(),
            project_id: "prj_a".into(),
            title: format!("issue {id}"),
            wave: 1,
            phase: "phase-0".into(),
            status: OrchestrationStatus::Eligible,
            work_order_hash: format!("sha256:{id}"),
            gate2: Gate2State::Reviewed { by: "h".into(), at: 1 },
            lease: None,
            retry_count: 0,
            disposition: None,
            priority: 0,
            is_wave_integration: false,
            created_at: 1,
        })
        .await
        .unwrap();
    }
    for (run, issue) in [("run_a", "iss_a"), ("run_b", "iss_b")] {
        surge_store::observatory::insert_run(&pool, &Run {
            id: run.into(),
            project_id: "prj_a".into(),
            issue_id: Some(issue.into()),
            kind: RunKind::WorkOrder,
            materialization_hash: "sha256:mat".into(),
            work_order_hash: Some(format!("sha256:{issue}")),
            status: RunStatus::Running,
            started_at: 1,
            ended_at: None,
            cost: 0.0,
        })
        .await
        .unwrap();
    }
    Env { state, session, _work: work }
}

async fn worker_token(env: &Env, run_id: &str) -> String {
    surge_store::tokens::mint_for_run(
        &env.state.pool,
        TokenKind::Runtime,
        Some("prj_a"),
        Some(run_id),
        now_ms(),
    )
    .await
    .unwrap()
}

/// The heartbeat hijack, end to end. Worker A's token refreshing dead worker
/// B's lease kept the sweeper off it forever, and `retry` then refused
/// because B still showed a live lease owner: an issue stuck with no recovery
/// but hand-editing SQLite. The chain proven here is the recovery the hijack
/// used to block — blocked heartbeat → sweeper reclaims → retry succeeds.
#[tokio::test]
async fn a_worker_cannot_heartbeat_another_workers_lease() {
    let env = setup().await;
    let router = app(env.state.clone());
    let tok_a = worker_token(&env, "run_a").await;

    // A holds a live lease; B's expired long ago (its worker is dead).
    assert!(surge_store::issues::claim_lease(
        &env.state.pool, "iss_a", "worker-1", "run_a", now_ms(), 600_000).await.unwrap());
    assert!(surge_store::issues::claim_lease(
        &env.state.pool, "iss_b", "worker-1", "run_b", 1, 1).await.unwrap());
    let b_before = surge_store::issues::get(&env.state.pool, "iss_b").await.unwrap().unwrap()
        .lease.unwrap().expires_at;

    // The hijack: refused loudly, and B's clock does not move.
    let r = router.clone()
        .oneshot(req("POST", "/runtime/issues/iss_b/heartbeat", Some(&tok_a), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let err = body_json(r).await["error"].as_str().unwrap().to_string();
    assert!(err.contains("bound to run run_a") && err.contains("iss_b"), "{err}");
    let b_after = surge_store::issues::get(&env.state.pool, "iss_b").await.unwrap().unwrap()
        .lease.unwrap().expires_at;
    assert_eq!(b_before, b_after, "the hijacked lease clock never moved");
    assert!(audit_actions(&env.state.pool).await.contains(&"auth.runtime_refused_scope".to_string()));

    // A's own lease still beats — the fix scopes, it does not disable.
    let r = router.clone()
        .oneshot(req("POST", "/runtime/issues/iss_a/heartbeat", Some(&tok_a), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // …so the sweeper reclaims B (the step the hijack used to prevent)…
    assert_eq!(surge_server::supervisor::sweep_expired_leases(&env.state).await, 1);
    let iss_b = surge_store::issues::get(&env.state.pool, "iss_b").await.unwrap().unwrap();
    assert!(iss_b.lease.is_none(), "lease reclaimed");
    assert_eq!(iss_b.status, OrchestrationStatus::Failed);

    // …and retry is possible again, from inside the product.
    let r = router
        .oneshot(req("POST", "/api/issues/iss_b/retry", Some(&env.session), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        surge_store::issues::get(&env.state.pool, "iss_b").await.unwrap().unwrap().status,
        OrchestrationStatus::Eligible
    );
}

/// The rest of the run-bound scope table: own run only, own issue only, and
/// never a new claim (its run already holds a lease — a bound token claiming
/// fresh work is F1's exploit shape).
#[tokio::test]
async fn a_run_bound_token_reaches_only_its_own_run_and_issue() {
    let env = setup().await;
    let router = app(env.state.clone());
    let tok_a = worker_token(&env, "run_a").await;

    // Own run: readable, appendable.
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_a", Some(&tok_a), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = router.clone()
        .oneshot(req("POST", "/runtime/runs/run_a/spans", Some(&tok_a), Some(span("sp_w_a", "run_a"))))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);

    // Another run in the SAME project: refused on both.
    for (method, uri, body) in [
        ("GET", "/runtime/runs/run_b", None),
        ("POST", "/runtime/runs/run_b/spans", Some(span("sp_w_b", "run_b"))),
    ] {
        let r = router.clone().oneshot(req(method, uri, Some(&tok_a), body)).await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{uri}");
        assert!(body_json(r).await["error"].as_str().unwrap().contains("bound to run run_a"));
    }
    assert!(
        surge_store::observatory::span_tree(&env.state.pool, "run_b").await.unwrap().is_empty(),
        "nothing was written to the other run"
    );

    // Its own issue's work order is fetchable — the MCP work-order tool needs
    // exactly this — and another issue's is not.
    let r = router.clone()
        .oneshot(req("GET", "/runtime/issues/iss_a/work-order", Some(&tok_a), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(body_json(r).await["work_order"].as_str().unwrap().contains("issue iss_a"));
    let r = router.clone()
        .oneshot(req("GET", "/runtime/issues/iss_b/work-order", Some(&tok_a), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    // And it may not claim anything, its own issue included.
    for issue in ["iss_a", "iss_b"] {
        let r = router.clone()
            .oneshot(req("POST", &format!("/runtime/issues/{issue}/claim"), Some(&tok_a), None))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{issue}");
        assert!(body_json(r).await["error"].as_str().unwrap().contains("already holds"));
    }
    // No run was created by the refused claims.
    assert_eq!(surge_store::observatory::list_runs(&env.state.pool, Some("prj_a")).await.unwrap().len(), 2);
}

/// F1: the project runtime token rotates, expires and can be revoked. The
/// property that has to hold after a full walk is arithmetic — zero live
/// runtime tokens that are not backing a currently-running run.
#[tokio::test]
async fn the_project_runtime_token_rotates_expires_and_revokes() {
    let env = setup().await;
    let router = app(env.state.clone());
    let mint = || req("POST", "/api/projects/prj_a/runtime-token", Some(&env.session), None);

    let first = body_json(router.clone().oneshot(mint()).await.unwrap()).await;
    let tok1 = first["token"].as_str().unwrap().to_string();
    assert!(first["expires_at"].as_i64().unwrap() > now_ms(), "the token carries a clock");
    assert_eq!(first["rotated_out"], 0);

    // Minting again ROTATES: the old token is dead, not a second live one.
    let second = body_json(router.clone().oneshot(mint()).await.unwrap()).await;
    let tok2 = second["token"].as_str().unwrap().to_string();
    assert_eq!(second["rotated_out"], 1);
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_a", Some(&tok1), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "the rotated-out token is dead");
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_a", Some(&tok2), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let live = surge_store::tokens::live_runtime_tokens(&env.state.pool).await.unwrap();
    assert_eq!(live.len(), 1, "one project token, never two: {live:?}");

    // Explicit revocation — the half that did not exist at all.
    let r = router.clone()
        .oneshot(req("DELETE", "/api/projects/prj_a/runtime-token", Some(&env.session), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(body_json(r).await["revoked"], 1);
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_a", Some(&tok2), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert!(surge_store::tokens::live_runtime_tokens(&env.state.pool).await.unwrap().is_empty());

    // Expiry: a token whose clock ran out is refused before any sweeper runs,
    // and named as expired rather than "invalid" (INV-ERR-1).
    let stale = surge_store::tokens::rotate_project_runtime(
        &env.state.pool,
        "prj_a",
        now_ms() - surge_store::tokens::PROJECT_RUNTIME_TTL_MS - 1,
    )
    .await
    .unwrap();
    let r = router
        .oneshot(req("GET", "/runtime/runs/run_a", Some(&stale.plaintext), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert!(body_json(r).await["error"].as_str().unwrap().contains("expired"));
    let actions = audit_actions(&env.state.pool).await;
    assert!(actions.contains(&"auth.expired_token".to_string()), "{actions:?}");
    assert!(actions.contains(&"token.runtime_rotated".to_string()));
    assert!(actions.contains(&"token.runtime_revoked".to_string()));

    // And the sweeper makes the store agree with the clock, so the hygiene
    // property is checkable by looking at the table.
    assert_eq!(surge_server::supervisor::sweep_expired_tokens(&env.state).await, 1);
    assert!(
        surge_store::tokens::live_runtime_tokens(&env.state.pool).await.unwrap().is_empty(),
        "zero live runtime tokens once no run is backing one (F1)"
    );
}

/// F1's other half, in one assertion: a bare `mint` of a runtime credential —
/// the call that produced the immortal token — is now unrepresentable.
#[tokio::test]
async fn a_runtime_credential_cannot_be_minted_without_a_lifecycle() {
    let env = setup().await;
    let err = surge_store::tokens::mint(&env.state.pool, TokenKind::Runtime, Some("prj_a"), 1)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("lifecycle"), "{err}");
    assert!(surge_store::tokens::live_runtime_tokens(&env.state.pool).await.unwrap().is_empty());
}

/// A terminal run's span record is closed. Appending to one corrupts the
/// append-only history INV-OBS-2 promises to keep — and it was reachable by
/// any live token, human included.
#[tokio::test]
async fn a_terminal_run_refuses_span_appends() {
    let env = setup().await;
    let router = app(env.state.clone());
    let tok_a = worker_token(&env, "run_a").await;
    assert!(surge_store::observatory::finish_run_if_running(
        &env.state.pool, "run_a", RunStatus::Succeeded, now_ms()).await.unwrap());

    // The run's own credential is revoked with it (S2), so use the human
    // token — the superset — to prove the refusal is about the run, not the
    // caller.
    for token in [env.session.as_str(), tok_a.as_str()] {
        let r = router.clone()
            .oneshot(req("POST", "/runtime/runs/run_a/spans", Some(token), Some(span("sp_late", "run_a"))))
            .await
            .unwrap();
        // The worker's token is dead by now (revoked with the run); the human
        // token is alive and still refused. Either way, nothing is appended.
        assert!(
            r.status() == StatusCode::CONFLICT || r.status() == StatusCode::UNAUTHORIZED,
            "unexpected {}", r.status()
        );
    }
    assert!(
        surge_store::observatory::span_tree(&env.state.pool, "run_a").await.unwrap().is_empty(),
        "no span landed on the ended run"
    );
    let actions = audit_actions(&env.state.pool).await;
    assert!(actions.contains(&"span.append_refused".to_string()), "{actions:?}");
    // Polling a terminal run still works — that is how an abort lands (§06-06).
    let r = router.oneshot(req("GET", "/runtime/runs/run_a", Some(&env.session), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

/// Span-id squatting: `span.id` is the primary key and the supervisor's
/// termination records use predictable ids, so a worker that pre-inserted one
/// could make the supervisor's later write collide — silently suppressing a
/// run's termination record. The namespace is reserved at the API.
#[tokio::test]
async fn reserved_span_ids_are_refused_so_terminations_cannot_be_suppressed() {
    let env = setup().await;
    let router = app(env.state.clone());
    let tok_a = worker_token(&env, "run_a").await;

    for id in ["sp_end_run_a", "sp_abort_run_a", "sp_fail_run_a", "sp_orphan_run_a", "sp_run_a"] {
        let r = router.clone()
            .oneshot(req("POST", "/runtime/runs/run_a/spans", Some(&tok_a), Some(span(id, "run_a"))))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{id} must be refused");
        assert!(body_json(r).await["error"].as_str().unwrap().contains("reserved"));
    }
    // The worker's own namespace is untouched.
    let r = router.clone()
        .oneshot(req("POST", "/runtime/runs/run_a/spans", Some(&tok_a), Some(span("sp_w_1", "run_a"))))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);

    // And the supervisor's own record still lands afterwards — the write the
    // squat was aiming to break.
    assert!(surge_server::supervisor::abort_run(&env.state, "run_a").await);
    let spans = surge_store::observatory::span_tree(&env.state.pool, "run_a").await.unwrap();
    assert!(
        spans.iter().any(|s| s.id == "sp_abort_run_a"),
        "the abort record was written, not collided out: {spans:?}"
    );
}

/// F4: `POST /api/issues/{unknown}/dispatch` answered `500 dispatch failed`
/// with the reason on the server's stderr only — no run, no span, no audit
/// row. It is the first thing a new operator hits after a typo.
#[tokio::test]
async fn dispatching_an_unknown_issue_is_a_named_404_with_an_audit_row() {
    let env = setup().await;
    let router = app(env.state.clone());

    let r = router
        .oneshot(req("POST", "/api/issues/iss_typo/dispatch", Some(&env.session), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let body = body_json(r).await;
    assert_eq!(body["refused"], true);
    let err = body["error"].as_str().unwrap();
    assert!(err.contains("no such issue") && err.contains("iss_typo"), "{err}");

    let audit = surge_store::audit::recent(&env.state.pool, 50).await.unwrap();
    assert!(
        audit.iter().any(|a| a.action == "dispatch.refused" && a.subject.contains("iss_typo")),
        "the refusal is a record, not a stderr line: {audit:?}"
    );
    // No run was invented for an issue that does not exist.
    assert!(surge_store::observatory::list_runs(&env.state.pool, None).await.unwrap().len() == 2);
}
