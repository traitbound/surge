//! The runtime-token surface (INV-AUTH-1). Two of the five capabilities land
//! here now — append spans, poll own-run status; fetch work order/lease,
//! claim lease and heartbeat arrive with items 6–7. Every handler scopes a
//! runtime identity to its own project; a human token passes everywhere.

use crate::{now_ms, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use surge_domain::observatory::Span;
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

/// Own-project check: a runtime token sees only its project's runs. The
/// refusal is loud and audited (INV-AUTH-2 discipline applied to scope).
async fn scope_check(state: &AppState, identity: &Identity, run_project: &str, path: &str) -> Option<Response> {
    match identity {
        Identity::Human => None,
        Identity::Runtime { project_id } if project_id == run_project => None,
        Identity::Runtime { project_id } => {
            let actor = format!("rt:{project_id}");
            if let Err(e) = surge_store::audit::record(
                &state.pool,
                "auth.runtime_refused_scope",
                path,
                &actor,
                Some(project_id),
                now_ms(),
            )
            .await
            {
                eprintln!("AUDIT WRITE FAILED for scope refusal on {path}: {e}");
            }
            Some(
                (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "run belongs to another project (audited)" })),
                )
                    .into_response(),
            )
        }
    }
}

/// Capability 5: poll own-run status — the read that makes an abort land at
/// the worker's next tool call (INV-AUTH-1).
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
    if let Some(refused) = scope_check(&state, &identity, &run.project_id, "/runtime/runs").await {
        return refused;
    }
    Json(run).into_response()
}

/// Capability 4: append spans. Span content is observability, never control
/// flow (INV-EXEC-3) — nothing here transitions orchestration state.
async fn append_span(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(run_id): Path<String>,
    Json(mut span): Json<Span>,
) -> Response {
    let run = match surge_store::observatory::get_run(&state.pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown run" })))
                .into_response()
        }
    };
    if let Some(refused) =
        scope_check(&state, &identity, &run.project_id, "/runtime/runs/spans").await
    {
        return refused;
    }
    span.run_id = run_id;
    if let Err(e) = surge_store::observatory::append_span(&state.pool, &span).await {
        return internal(e, "span append failed");
    }
    (StatusCode::CREATED, Json(serde_json::json!({ "ok": true }))).into_response()
}

/// Issue-scoped guard shared by the work-order/lease/heartbeat capabilities.
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
    if let Some(refused) = scope_check(state, identity, &issue.project_id, path).await {
        return Err(refused);
    }
    Ok(issue)
}

/// Capability 1: fetch work order / lease / materialization hash — what a
/// session needs at start (the compiled `.claude/` files on disk are the
/// pipeline itself; this carries the rest).
async fn fetch_work_order(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    let issue = match issue_for(&state, &identity, &issue_id, "/runtime/issues/work-order").await {
        Ok(i) => i,
        Err(r) => return r,
    };
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
async fn claim_lease(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    let issue = match issue_for(&state, &identity, &issue_id, "/runtime/issues/claim").await {
        Ok(i) => i,
        Err(r) => return r,
    };
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
        Identity::Runtime { project_id } => format!("rt:{project_id}"),
    };
    match surge_store::issues::claim_lease(
        &state.pool, &issue_id, &owner, &run_id, now, state.supervisor.lease_ttl_ms,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            let _ = surge_store::observatory::finish_run_if_running(
                &state.pool, &run_id, surge_domain::observatory::RunStatus::Refused, now,
            )
            .await;
            return (StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "issue is not eligible or already leased" })))
                .into_response();
        }
        Err(e) => return internal(e, "lease claim failed"),
    }
    Json(serde_json::json!({ "run_id": run_id, "expires_at": now + state.supervisor.lease_ttl_ms }))
        .into_response()
}

/// Capability 3: heartbeat — the lease clock follows the last beat (§06).
async fn heartbeat(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(issue_id): Path<String>,
) -> Response {
    if let Err(r) = issue_for(&state, &identity, &issue_id, "/runtime/issues/heartbeat").await {
        return r;
    }
    let now = now_ms();
    match surge_store::issues::heartbeat(&state.pool, &issue_id, now, state.supervisor.lease_ttl_ms).await
    {
        Ok(true) => Json(serde_json::json!({ "expires_at": now + state.supervisor.lease_ttl_ms }))
            .into_response(),
        Ok(false) => (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "no live lease" })))
            .into_response(),
        Err(e) => internal(e, "heartbeat failed"),
    }
}
