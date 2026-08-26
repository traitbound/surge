//! The runtime-token surface (INV-AUTH-1): fetch work order · claim lease ·
//! heartbeat · append spans · poll own-run status. A human token passes
//! everywhere here — everything a machine may do is a subset of what a human
//! may do (design §04).
//!
//! # Scope
//!
//! A supervisor-spawned credential names the run it was minted for
//! (INV-AUTH-4 injects it at spawn; migration 0004 binds it). Comparing
//! project ids alone — all this module used to do — let ANY live runtime
//! token in project P act on ANY run or issue in P. The exploitable case:
//! worker A's token calling `POST /runtime/issues/B/heartbeat` for dead
//! worker B, refreshing B's lease forever. The sweeper then never reclaims
//! it, and `retry` refuses because B still shows a live lease owner — the
//! issue is stuck with no recovery but hand-editing SQLite (auth review
//! 2026-08-26).
//!
//! | identity | poll · append span | work order | heartbeat | claim |
//! |---|---|---|---|---|
//! | human | any run | any issue | any issue | any eligible issue |
//! | runtime, run-bound | its own run | its own run's issue | only the issue its run leases | never — its run already holds one |
//! | runtime, project token | any run in its project | any issue in its project | any issue in its project | any eligible issue in its project |
//!
//! The project token keeps project scope because it has no run to be scoped
//! to: it is the interactive / `surge auth` credential, and claiming is
//! exactly what it exists for (INV-EXEC-1 — interactive sessions claim rather
//! than get spawned). That is also why F1 gave it a rotation and a 15-minute
//! expiry: project scope is only tolerable on a credential that is singular
//! and short-lived.
//!
//! Every refusal here is loud and audited (INV-AUTH-2 discipline applied to
//! scope, INV-ERR-1 — refusals are data).

use crate::{now_ms, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use surge_domain::observatory::{RunStatus, Span};
use surge_store::tokens::Identity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/runs/{id}", get(poll_run))
        .route("/runs/{id}/spans", post(append_span))
        .route("/issues/{id}/work-order", get(fetch_work_order))
        .route("/issues/{id}/claim", post(claim_lease))
        .route("/issues/{id}/heartbeat", post(heartbeat))
}

fn internal(e: anyhow::Error, what: &str) -> Response {
    eprintln!("{what}: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": what })))
        .into_response()
}

fn actor_of(identity: &Identity) -> String {
    match identity {
        Identity::Human => "human".to_string(),
        Identity::Runtime { project_id, run_id: None } => format!("rt:{project_id}"),
        Identity::Runtime { project_id, run_id: Some(run) } => format!("rt:{project_id}:{run}"),
    }
}

/// One refusal, one audit row, one reason string the caller can read. The
/// audit subject carries the path *and* the reason, so the trail says what
/// was refused and why without a second lookup (INV-ERR-1).
async fn refuse(
    state: &AppState,
    identity: &Identity,
    action: &str,
    project_id: Option<&str>,
    path: &str,
    status: StatusCode,
    reason: String,
) -> Response {
    if let Err(e) = surge_store::audit::record(
        &state.pool,
        action,
        &format!("{path} — {reason}"),
        &actor_of(identity),
        project_id,
        now_ms(),
    )
    .await
    {
        eprintln!("AUDIT WRITE FAILED for {action} on {path}: {e}");
    }
    (status, Json(serde_json::json!({ "error": reason }))).into_response()
}

/// Scope one call against a run: own project, and — for a run-bound token —
/// own run. See the module docs for the whole table.
async fn scope_run(
    state: &AppState,
    identity: &Identity,
    run: &surge_domain::observatory::Run,
    path: &str,
) -> Option<Response> {
    let (project_id, bound) = match identity {
        Identity::Human => return None,
        Identity::Runtime { project_id, run_id } => (project_id, run_id),
    };
    let reason = if project_id != &run.project_id {
        "run belongs to another project (audited)".to_string()
    } else if bound.as_deref().is_some_and(|b| b != run.id) {
        format!(
            "runtime token is bound to run {}; it may not act on run {} (audited)",
            bound.as_deref().unwrap_or_default(),
            run.id
        )
    } else {
        return None;
    };
    Some(
        refuse(
            state,
            identity,
            "auth.runtime_refused_scope",
            Some(project_id),
            path,
            StatusCode::FORBIDDEN,
            reason,
        )
        .await,
    )
}

/// Capability 5: poll own-run status — the read that makes an abort land at
/// the worker's next tool call (INV-AUTH-1). Terminal runs are readable by
/// design: reading `aborted` is how the order arrives (§06-06).
async fn poll_run(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(run_id): Path<String>,
) -> Response {
    let run = match surge_store::observatory::get_run(&state.pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown run" })))
                .into_response()
        }
    };
    if let Some(refused) = scope_run(&state, &identity, &run, "/runtime/runs").await {
        return refused;
    }
    Json(run).into_response()
}

/// Capability 4: append spans. Span content is observability, never control
/// flow (INV-EXEC-3) — nothing here transitions orchestration state.
///
/// Two refusals guard the record itself:
///
/// * **Terminal runs.** A live token could write spans onto a run that had
///   already ended, so the append-only record INV-OBS-2 promises to keep grew
///   after the fact (smoke walk 5). A run that has ended is closed.
/// * **Reserved span ids.** `span.id` is the primary key and the supervisor's
///   own records use predictable ids (`sp_end_…`, `sp_abort_…`), so a worker
///   could pre-insert one and make the supervisor's later write collide —
///   silently suppressing a termination record (concurrency review
///   2026-08-26). Server-generating ids instead would break the idempotency
///   the plugin's tools rely on (they name a span deterministically so a
///   retried tool call cannot double-write) and would strand
///   `supervisor::unobserved`, which reads the same prefixes to tell a
///   supervisor span from a worker's. Reserving the namespace at the API
///   keeps both, and the refusal is visible rather than a swallowed
///   collision.
async fn append_span(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(run_id): Path<String>,
    Json(mut span): Json<Span>,
) -> Response {
    const PATH: &str = "/runtime/runs/spans";
    let run = match surge_store::observatory::get_run(&state.pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown run" })))
                .into_response()
        }
    };
    if let Some(refused) = scope_run(&state, &identity, &run, PATH).await {
        return refused;
    }
    if run.status != RunStatus::Running {
        let reason = format!(
            "span append refused — run {} already ended ({}); the span record is append-only \
             while a run is live and closed once it is not (INV-OBS-2)",
            run.id,
            run.status.as_str()
        );
        return refuse(
            &state,
            &identity,
            "span.append_refused",
            Some(&run.project_id),
            PATH,
            StatusCode::CONFLICT,
            reason,
        )
        .await;
    }
    if crate::supervisor::is_reserved_span_id(&span.id) {
        let reason = format!(
            "span append refused — span id {} is reserved for supervisor-written \
             termination and refusal records (INV-ERR-1)",
            span.id
        );
        return refuse(
            &state,
            &identity,
            "span.append_refused",
            Some(&run.project_id),
            PATH,
            StatusCode::FORBIDDEN,
            reason,
        )
        .await;
    }
    span.run_id = run_id;
    if let Err(e) = surge_store::observatory::append_span(&state.pool, &span).await {
        return internal(e, "span append failed");
    }
    (StatusCode::CREATED, Json(serde_json::json!({ "ok": true }))).into_response()
}

/// Project-scope guard shared by the issue-addressed capabilities. Run scope
/// is per capability — what "its own issue" means differs between fetching a
/// work order, heartbeating a lease and claiming one.
#[allow(clippy::result_large_err)] // the Err is a ready-to-send Response by design
async fn issue_for(
    state: &AppState,
    identity: &Identity,
    issue_id: &str,
    path: &str,
) -> Result<surge_domain::board::Issue, Response> {
    let issue = match surge_store::issues::get(&state.pool, issue_id).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown issue" })))
                .into_response())
        }
        Err(e) => return Err(internal(e, "issue lookup failed")),
    };
    if let Identity::Runtime { project_id, .. } = identity {
        if project_id != &issue.project_id {
            return Err(refuse(
                state,
                identity,
                "auth.runtime_refused_scope",
                Some(project_id),
                path,
                StatusCode::FORBIDDEN,
                "issue belongs to another project (audited)".to_string(),
            )
            .await);
        }
    }
    Ok(issue)
}

/// The run a token is bound to, if it is bound to one.
fn bound_run(identity: &Identity) -> Option<&str> {
    match identity {
        Identity::Runtime { run_id: Some(run), .. } => Some(run.as_str()),
        _ => None,
    }
}

/// Capability 1: fetch work order / lease / materialization hash — what a
/// session needs at start (the compiled `.claude/` files on disk are the
/// pipeline itself; this carries the rest). A run-bound token may fetch for
/// the issue its own run was dispatched for, and no other.
async fn fetch_work_order(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    const PATH: &str = "/runtime/issues/work-order";
    let issue = match issue_for(&state, &identity, &issue_id, PATH).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    if let Some(bound) = bound_run(&identity) {
        let own_issue = surge_store::observatory::get_run(&state.pool, bound)
            .await
            .ok()
            .and_then(|r| r.issue_id);
        if own_issue.as_deref() != Some(issue.id.as_str()) {
            let reason = format!(
                "runtime token is bound to run {bound}, which was not dispatched for issue \
                 {issue_id} (audited)"
            );
            return refuse(
                &state,
                &identity,
                "auth.runtime_refused_scope",
                Some(&issue.project_id),
                PATH,
                StatusCode::FORBIDDEN,
                reason,
            )
            .await;
        }
    }
    let wo = surge_compiler::work_order::render_work_order(&issue);
    let mat = match surge_store::materializations::fresh_for_project(&state.pool, &issue.project_id).await
    {
        Ok(m) => m,
        Err(e) => return internal(e, "materialization lookup failed"),
    };
    Json(serde_json::json!({
        "issue_id": issue.id,
        "work_order": wo,
        "work_order_hash": issue.work_order_hash,
        "lease": issue.lease,
        "materialization_hash": mat.map(|m| m.content_hash),
    }))
    .into_response()
}

/// Capability 2: claim a lease (interactive sessions — human-launched, they
/// claim rather than get spawned, INV-EXEC-1).
///
/// A run-bound token cannot claim at all: it was minted for a run whose lease
/// the supervisor already holds, so a claim from it is either a mistake or
/// the F1 exploit — a credential outliving its run and taking a brand-new
/// lease on unrelated work.
async fn claim_lease(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    const PATH: &str = "/runtime/issues/claim";
    let issue = match issue_for(&state, &identity, &issue_id, PATH).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    if let Some(bound) = bound_run(&identity) {
        let reason = format!(
            "claim refused — this runtime token is bound to run {bound}, which already holds \
             its lease; claiming is for interactive sessions (INV-EXEC-1, audited)"
        );
        return refuse(
            &state,
            &identity,
            "auth.runtime_refused_scope",
            Some(&issue.project_id),
            PATH,
            StatusCode::FORBIDDEN,
            reason,
        )
        .await;
    }
    let now = now_ms();
    let mat = match surge_store::materializations::fresh_for_project(&state.pool, &issue.project_id).await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "no fresh materialization; compile first (INV-ID-1)" })))
                .into_response()
        }
        Err(e) => return internal(e, "materialization lookup failed"),
    };
    let run_id = format!("run_{}", &surge_store::tokens::hash(&format!("{issue_id}{now}"))[..12]);
    let run = surge_domain::observatory::Run {
        id: run_id.clone(),
        project_id: issue.project_id.clone(),
        issue_id: Some(issue.id.clone()),
        kind: surge_domain::observatory::RunKind::WorkOrder,
        materialization_hash: mat.content_hash,
        work_order_hash: Some(issue.work_order_hash.clone()),
        status: surge_domain::observatory::RunStatus::Running,
        started_at: now,
        ended_at: None,
        cost: 0.0,
    };
    if let Err(e) = surge_store::observatory::insert_run(&state.pool, &run).await {
        return internal(e, "run insert failed");
    }
    let owner = match &identity {
        Identity::Human => "interactive".to_string(),
        Identity::Runtime { project_id, .. } => format!("rt:{project_id}"),
    };
    match surge_store::issues::claim_lease(
        &state.pool, &issue_id, &owner, &run_id, now, state.supervisor.lease_ttl_ms,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            // Every refusal is data, whichever token kind reached it: this
            // branch used to terminalize the run and return, leaving the only
            // spanless, auditless refusal in the product (smoke walk 4, S3).
            let reason = "lease claim refused — issue is not eligible or already leased";
            let _ = surge_store::observatory::finish_run_if_running(
                &state.pool, &run_id, surge_domain::observatory::RunStatus::Refused, now,
            )
            .await;
            crate::supervisor::log_span_failure(
                crate::supervisor::refusal_span(&state, &run_id, reason, now).await,
                "claim refusal",
                &run_id,
            );
            let _ = surge_store::audit::record(
                &state.pool, "lease.claim_refused", &issue_id, &owner, Some(&issue.project_id), now,
            )
            .await;
            return (StatusCode::CONFLICT, Json(serde_json::json!({ "error": reason })))
                .into_response();
        }
        Err(e) => return internal(e, "lease claim failed"),
    }
    Json(serde_json::json!({ "run_id": run_id, "expires_at": now + state.supervisor.lease_ttl_ms }))
        .into_response()
}

/// Capability 3: heartbeat — the lease clock follows the last beat (§06).
///
/// A run-bound token beats only for the issue its own run leases. Anything
/// else is the heartbeat hijack: refreshing a dead worker's lease keeps the
/// sweeper off it forever and makes `retry` refuse, which is unrecoverable
/// from inside the product (auth review 2026-08-26).
async fn heartbeat(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    const PATH: &str = "/runtime/issues/heartbeat";
    let issue = match issue_for(&state, &identity, &issue_id, PATH).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    if let Some(bound) = bound_run(&identity) {
        let holder = issue.lease.as_ref().map(|l| l.run_id.as_str());
        if holder != Some(bound) {
            let reason = format!(
                "heartbeat refused — this runtime token is bound to run {bound}, which does not \
                 hold issue {issue_id}'s lease ({}); refreshing another run's lease is not a \
                 runtime capability (INV-AUTH-1, audited)",
                holder.unwrap_or("no live lease")
            );
            return refuse(
                &state,
                &identity,
                "auth.runtime_refused_scope",
                Some(&issue.project_id),
                PATH,
                StatusCode::FORBIDDEN,
                reason,
            )
            .await;
        }
    }
    let now = now_ms();
    match surge_store::issues::heartbeat(
        &state.pool,
        &issue_id,
        bound_run(&identity),
        now,
        state.supervisor.lease_ttl_ms,
    )
    .await
    {
        Ok(true) => Json(serde_json::json!({ "expires_at": now + state.supervisor.lease_ttl_ms }))
            .into_response(),
        Ok(false) => (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "no live lease" })))
            .into_response(),
        Err(e) => internal(e, "heartbeat failed"),
    }
}
