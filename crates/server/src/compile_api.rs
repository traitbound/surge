//! POST /api/projects/{id}/compile — the approval point for what a pipeline
//! can do (§04). Human token only (middleware). Refusals are visible records
//! (INV-ERR-1); success writes the files, the materialization row and the
//! audit entry.

use crate::{now_ms, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::path::Path as FsPath;
use surge_compiler::{CompileRefusal, LibraryIndex};
use surge_domain::materialization::Materialization;

#[derive(Deserialize)]
pub struct CompileBody {
    pub pipeline_id: String,
}

fn internal(e: anyhow::Error, what: &str) -> Response {
    eprintln!("{what}: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": what })))
        .into_response()
}

pub async fn compile_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(body): Json<CompileBody>,
) -> Response {
    let now = now_ms();
    let project = match surge_store::projects::get(&state.pool, &project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown project" })))
                .into_response()
        }
        Err(e) => return internal(e, "project lookup failed"),
    };
    let (pipeline, nodes, edges) =
        match surge_store::pipelines::load_graph(&state.pool, &body.pipeline_id).await {
            Ok(g) => g,
            Err(_) => {
                return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown pipeline" })))
                    .into_response()
            }
        };

    // Assemble the library index for every pinned reference.
    let mut library = LibraryIndex::new();
    for (kind, r) in surge_compiler::referenced_items(&nodes) {
        match surge_store::library::get(&state.pool, kind, &r.name, r.version).await {
            Ok(Some(item)) => {
                library.insert((kind, r.name.clone(), r.version), item);
            }
            Ok(None) => {} // compile() reports it as Missing, with names
            Err(e) => return internal(e, "library lookup failed"),
        }
    }

    let compiled = match surge_compiler::compile(&pipeline, &nodes, &edges, &library, &project) {
        Ok(c) => c,
        Err(refusal) => {
            // Compile block → visible record (INV-ERR-1) with the reason string.
            let reason = refusal.to_string();
            if let Err(e) = surge_store::audit::record(
                &state.pool, "compile.refused", &reason, "human", Some(&project_id), now,
            )
            .await
            {
                return internal(e, "audit write failed");
            }
            let status = match refusal {
                CompileRefusal::Untrusted { .. } => StatusCode::CONFLICT,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            return (status, Json(serde_json::json!({ "error": reason }))).into_response();
        }
    };

    if let Err(e) = surge_compiler::write_to_repo(FsPath::new(&project.repo_path), &compiled) {
        return internal(e, "repo write failed");
    }

    let materialization = Materialization {
        id: compiled.cache_key.clone(),
        content_hash: compiled.materialization_hash.clone(),
        cache_key: compiled.cache_key.clone(),
        pipeline_id: pipeline.id.clone(),
        project_id: project.id.clone(),
        signed_by: "st_session".into(), // per-token identity naming lands with item 8's auth UX
        fresh: true,
        created_at: now,
    };
    // The materialization row and its audit entry commit together (INV-DATA-8):
    // compiling changes dispatch eligibility (INV-ID-1), so a fresh
    // materialization with no audit row is a privileged act with no record.
    let commit = async {
        let mut tx = state.pool.begin().await?;
        surge_store::materializations::insert_fresh(&mut tx, &materialization).await?;
        surge_store::audit::record(
            &mut *tx,
            "pipeline.compiled",
            &compiled.materialization_hash,
            "human",
            Some(&project_id),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(())
    };
    if let Err(e) = commit.await {
        return internal(e, "compile commit failed");
    }

    Json(serde_json::json!({
        "materialization": materialization,
        "capability_report": compiled.report,
        "files": compiled.files.iter().map(|f| f.rel_path.clone()).collect::<Vec<_>>(),
    }))
    .into_response()
}
