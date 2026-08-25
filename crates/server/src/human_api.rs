//! Privileged (human-token) endpoints. Every privileged act writes an audit
//! entry (INV-OBS-1).

use crate::{now_ms, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use surge_domain::project::Project;
use surge_store::tokens::TokenKind;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create_project))
        .route("/projects/{id}/runtime-token", post(mint_runtime_token))
        .route("/session/rotate", post(rotate_session))
        .route("/audit", get(recent_audit))
}

fn internal(e: anyhow::Error, what: &str) -> Response {
    eprintln!("{what}: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": what })))
        .into_response()
}

#[derive(Deserialize)]
struct CreateProject {
    id: String,
    name: String,
    repo_path: String,
}

/// Registers the project row. Binding (surge.yaml write, INV-DATA-1) is
/// phase 0 item 4 — this only creates the entity runtime tokens scope to.
async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateProject>,
) -> Response {
    let project = Project {
        id: body.id,
        name: body.name,
        repo_path: body.repo_path,
        assigned_pipeline: None,
        pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: surge_domain::project::TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: now_ms(),
    };
    if let Err(e) = surge_store::projects::insert(&state.pool, &project).await {
        return internal(e, "project insert failed");
    }
    if let Err(e) = surge_store::audit::record(
        &state.pool,
        "project.created",
        &project.id,
        "human",
        Some(&project.id),
        now_ms(),
    )
    .await
    {
        return internal(e, "audit write failed");
    }
    (StatusCode::CREATED, Json(project)).into_response()
}

/// Mint a per-project runtime token (INV-AUTH-1). The plaintext exists only
/// in this response — it reaches runtimes via spawn-time env injection or
/// `surge auth` machine-local config (INV-AUTH-4).
async fn mint_runtime_token(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Response {
    match surge_store::projects::exists(&state.pool, &project_id).await {
        Ok(true) => {}
        Ok(false) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown project" })))
                .into_response()
        }
        Err(e) => return internal(e, "project lookup failed"),
    }
    let token = match surge_store::tokens::mint(
        &state.pool,
        TokenKind::Runtime,
        Some(&project_id),
        now_ms(),
    )
    .await
    {
        Ok(t) => t,
        Err(e) => return internal(e, "token mint failed"),
    };
    if let Err(e) = surge_store::audit::record(
        &state.pool,
        "token.runtime_minted",
        &project_id,
        "human",
        Some(&project_id),
        now_ms(),
    )
    .await
    {
        return internal(e, "audit write failed");
    }
    Json(serde_json::json!({ "token": token })).into_response()
}

/// Rotate the human session: every existing session is signed out and the
/// fresh token is returned to the caller (design §04).
async fn rotate_session(State(state): State<AppState>) -> Response {
    let now = now_ms();
    if let Err(e) = surge_store::tokens::revoke_all(&state.pool, TokenKind::Session, now).await {
        return internal(e, "session revoke failed");
    }
    let token = match surge_store::tokens::mint(&state.pool, TokenKind::Session, None, now).await {
        Ok(t) => t,
        Err(e) => return internal(e, "session mint failed"),
    };
    if let Err(e) =
        surge_store::audit::record(&state.pool, "token.session_rotated", "session", "human", None, now)
            .await
    {
        return internal(e, "audit write failed");
    }
    (
        [(
            axum::http::header::SET_COOKIE,
            format!("surge_session={token}; HttpOnly; SameSite=Strict; Path=/"),
        )],
        Json(serde_json::json!({ "token": token })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn recent_audit(State(state): State<AppState>, Query(q): Query<AuditQuery>) -> Response {
    match surge_store::audit::recent(&state.pool, q.limit.unwrap_or(50)).await {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => internal(e, "audit read failed"),
    }
}
