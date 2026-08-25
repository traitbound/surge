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
