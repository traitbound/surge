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
        .route("/projects", post(create_project).get(list_projects))
        .route("/projects/{id}/bind", post(bind_project))
        .route("/projects/{id}/runtime-token", post(mint_runtime_token))
        .route("/projects/{id}/compile", post(crate::compile_api::compile_project))
        .route("/session/rotate", post(rotate_session))
        .route("/audit", get(recent_audit))
        .route("/issues", post(create_issue))
        .route("/issues/{id}/dispatch", post(dispatch_issue))
        .route("/runs", get(list_runs))
        .route("/runs/{id}/abort", post(abort_run))
        .route("/runs/{id}/spans", get(run_spans))
        .route("/projects/{id}/doc-run", post(dispatch_doc_run))
}

/// Registry read: every bound project (the UI's card grid, design §08).
async fn list_projects(State(state): State<AppState>) -> Response {
    match surge_store::projects::list(&state.pool).await {
        Ok(projects) => Json(projects).into_response(),
        Err(e) => internal(e, "project list failed"),
    }
}

#[derive(Deserialize)]
struct RunsQuery {
    project_id: Option<String>,
}

/// Observatory read: runs newest-first, optionally scoped to one project.
async fn list_runs(State(state): State<AppState>, Query(q): Query<RunsQuery>) -> Response {
    match surge_store::observatory::list_runs(&state.pool, q.project_id.as_deref()).await {
        Ok(runs) => Json(runs).into_response(),
        Err(e) => internal(e, "run list failed"),
    }
}

/// Observatory read: one run's span tree in depth-first order (waterfall rows).
async fn run_spans(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match surge_store::observatory::span_tree(&state.pool, &id).await {
        Ok(spans) => Json(spans).into_response(),
        Err(e) => internal(e, "span tree read failed"),
    }
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

/// Bind the project to its repo: write the `surge.yaml` base file into
/// `repo_path` and NOTHING else (INV-DATA-1 — binding's only write; the
/// compiler later re-emits the same header with its step blocks). Refusals
/// are visible records with audit entries (INV-ERR-1).
async fn bind_project(State(state): State<AppState>, Path(project_id): Path<String>) -> Response {
    let now = now_ms();
    let project = match surge_store::projects::get(&state.pool, &project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            if let Err(e) = surge_store::audit::record(
                &state.pool,
                "project.bind_refused",
                &format!("unknown project: {project_id}"),
                "human",
                None,
                now,
            )
            .await
            {
                return internal(e, "audit write failed");
            }
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown project" })))
                .into_response();
        }
        Err(e) => return internal(e, "project lookup failed"),
    };
    if !std::path::Path::new(&project.repo_path).is_dir() {
        let reason = format!("repo path is not a directory: {}", project.repo_path);
        if let Err(e) = surge_store::audit::record(
            &state.pool,
            "project.bind_refused",
            &reason,
            "human",
            Some(&project_id),
            now,
        )
        .await
        {
            return internal(e, "audit write failed");
        }
        return (StatusCode::CONFLICT, Json(serde_json::json!({ "error": reason }))).into_response();
    }

    let yaml = surge_compiler::surge_yaml_base(&project);
    if let Err(e) = std::fs::write(std::path::Path::new(&project.repo_path).join("surge.yaml"), yaml)
    {
        return internal(e.into(), "surge.yaml write failed");
    }
    if let Err(e) = surge_store::projects::mark_surge_yaml_written(&state.pool, &project_id).await {
        return internal(e, "bind flag update failed");
    }
    if let Err(e) = surge_store::audit::record(
        &state.pool,
        "project.bound",
        &project_id,
        "human",
        Some(&project_id),
        now,
    )
    .await
    {
        return internal(e, "audit write failed");
    }
    match surge_store::projects::get(&state.pool, &project_id).await {
        Ok(Some(p)) => Json(p).into_response(),
        Ok(None) => internal(anyhow::anyhow!("project vanished mid-bind"), "project reload failed"),
        Err(e) => internal(e, "project reload failed"),
    }
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

#[derive(Deserialize)]
struct CreateIssue {
    id: String,
    project_id: String,
    title: String,
    wave: i64,
    phase: String,
}

/// Phase 0 issue creation (fixture issues; taskgraph generation is phase 2).
/// The work-order hash is pinned at creation from the rendered content, and
/// Gate-2 is recorded so the issue is dispatchable (§06-01).
async fn create_issue(State(state): State<AppState>, Json(body): Json<CreateIssue>) -> Response {
    let now = now_ms();
    let mut issue = surge_domain::board::Issue {
        id: body.id,
        project_id: body.project_id,
        title: body.title,
        wave: body.wave,
        phase: body.phase,
        status: surge_domain::board::OrchestrationStatus::Eligible,
        work_order_hash: String::new(),
        gate2: surge_domain::board::Gate2State::Reviewed { by: "human".into(), at: now },
        lease: None,
        retry_count: 0,
        disposition: None,
        priority: 0,
        is_wave_integration: false,
        created_at: now,
    };
    issue.work_order_hash = surge_compiler::work_order::work_order_hash(
        &surge_compiler::work_order::render_work_order(&issue),
    );
    if let Err(e) = surge_store::issues::insert(&state.pool, &issue).await {
        return internal(e, "issue insert failed");
    }
    if let Err(e) = surge_store::audit::record(
        &state.pool, "issue.created", &issue.id, "human", Some(&issue.project_id), now,
    )
    .await
    {
        return internal(e, "audit write failed");
    }
    (StatusCode::CREATED, Json(issue)).into_response()
}

async fn dispatch_issue(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match crate::supervisor::dispatch_issue(&state, &id).await {
        Ok(crate::supervisor::DispatchOutcome::Spawned { run_id }) => {
            Json(serde_json::json!({ "run_id": run_id, "refused": false })).into_response()
        }
        Ok(crate::supervisor::DispatchOutcome::Refused { run_id, reason }) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "run_id": run_id, "refused": true, "error": reason })),
        )
            .into_response(),
        Err(e) => internal(e, "dispatch failed"),
    }
}

/// The abort ledger write (§06-06): effective at the worker's next status
/// poll; the lease clock is the backstop if heartbeats stop first.
async fn abort_run(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let now = now_ms();
    match surge_store::observatory::abort_run(&state.pool, &id, now).await {
        Ok(true) => {
            if let Err(e) =
                surge_store::audit::record(&state.pool, "run.aborted", &id, "human", None, now).await
            {
                return internal(e, "audit write failed");
            }
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        Ok(false) => (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "run is not running" })))
            .into_response(),
        Err(e) => internal(e, "abort failed"),
    }
}

async fn dispatch_doc_run(State(state): State<AppState>, Path(project_id): Path<String>) -> Response {
    match crate::supervisor::dispatch_doc_run(&state, &project_id).await {
        Ok(run_id) => Json(serde_json::json!({ "run_id": run_id })).into_response(),
        Err(e) => internal(e, "doc run dispatch failed"),
    }
}
