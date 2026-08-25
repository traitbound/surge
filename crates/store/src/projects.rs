//! Project repository — the minimal subset the token boundary needs (a row to
//! scope runtime tokens to). Binding semantics (surge.yaml write) land with
//! phase 0 item 4.

use sqlx::SqlitePool;
use surge_domain::project::{Project, TrackerKind};

pub async fn insert(pool: &SqlitePool, p: &Project) -> anyhow::Result<()> {
    anyhow::ensure!(
        p.assigned_pipeline.is_none(),
        "assignment lands with the compiler task"
    );
    let tracker = match p.tracker {
        TrackerKind::Linear => "linear",
        TrackerKind::Github => "github",
        TrackerKind::Builtin => "builtin",
        TrackerKind::None => "none",
    };
    let yaml = p.surge_yaml_written as i64;
    sqlx::query!(
        "INSERT INTO project (id, name, repo_path, surge_yaml_written, tracker,
                              branch_format, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        p.id,
        p.name,
        p.repo_path,
        yaml,
        tracker,
        p.branch_format,
        p.created_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn exists(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let row = sqlx::query!("SELECT COUNT(*) AS n FROM project WHERE id = ?", id)
        .fetch_one(pool)
        .await?;
    Ok(row.n > 0)
}

pub async fn get(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Project>> {
    let row = sqlx::query!(
        "SELECT id, name, repo_path, pipeline_status, surge_yaml_written, tracker,
                branch_format, created_at
         FROM project WHERE id = ?",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Project {
        id: r.id,
        name: r.name,
        repo_path: r.repo_path,
        // Assignment modelling lands with the compiler/assignment tasks.
        assigned_pipeline: None,
        pipeline_status: match r.pipeline_status.as_str() {
            "stale" => surge_domain::project::PipelineAssignmentStatus::Stale,
            _ => surge_domain::project::PipelineAssignmentStatus::Published,
        },
        surge_yaml_written: r.surge_yaml_written != 0,
        tracker: match r.tracker.as_str() {
            "linear" => TrackerKind::Linear,
            "github" => TrackerKind::Github,
            "builtin" => TrackerKind::Builtin,
            _ => TrackerKind::None,
        },
        branch_format: r.branch_format,
        created_at: r.created_at,
    }))
}
