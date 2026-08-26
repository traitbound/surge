//! Project repository — project rows scope runtime tokens; binding
//! (`surge.yaml` written into the repo, INV-DATA-1) is recorded on the row via
//! [`mark_surge_yaml_written`].

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

/// Record that binding wrote `surge.yaml` into the project's repo (phase 0
/// item 4, INV-DATA-1). Returns whether a row was updated.
pub async fn mark_surge_yaml_written<'e, E>(executor: E, id: &str) -> anyhow::Result<bool> 
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let res = sqlx::query!("UPDATE project SET surge_yaml_written = 1 WHERE id = ?", id)
        .execute(executor)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn exists(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let row = sqlx::query!("SELECT COUNT(*) AS n FROM project WHERE id = ?", id)
        .fetch_one(pool)
        .await?;
    Ok(row.n > 0)
}

/// Shared row → entity mapping for the read paths. Assignment modelling lands
/// with the compiler/assignment tasks, so `assigned_pipeline` is `None`.
#[allow(clippy::too_many_arguments)]
fn project_from(
    id: String,
    name: String,
    repo_path: String,
    pipeline_status: String,
    surge_yaml_written: i64,
    tracker: String,
    branch_format: String,
    created_at: i64,
) -> Project {
    Project {
        id,
        name,
        repo_path,
        assigned_pipeline: None,
        pipeline_status: match pipeline_status.as_str() {
            "stale" => surge_domain::project::PipelineAssignmentStatus::Stale,
            _ => surge_domain::project::PipelineAssignmentStatus::Published,
        },
        surge_yaml_written: surge_yaml_written != 0,
        tracker: match tracker.as_str() {
            "linear" => TrackerKind::Linear,
            "github" => TrackerKind::Github,
            "builtin" => TrackerKind::Builtin,
            _ => TrackerKind::None,
        },
        branch_format,
        created_at,
    }
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
    Ok(row.map(|r| {
        project_from(
            r.id,
            r.name,
            r.repo_path,
            r.pipeline_status,
            r.surge_yaml_written,
            r.tracker,
            r.branch_format,
            r.created_at,
        )
    }))
}

/// Every bound project, newest first — the Registry's card grid.
pub async fn list(pool: &SqlitePool) -> anyhow::Result<Vec<Project>> {
    let rows = sqlx::query!(
        "SELECT id, name, repo_path, pipeline_status, surge_yaml_written, tracker,
                branch_format, created_at
         FROM project ORDER BY created_at DESC, id"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            project_from(
                r.id,
                r.name,
                r.repo_path,
                r.pipeline_status,
                r.surge_yaml_written,
                r.tracker,
                r.branch_format,
                r.created_at,
            )
        })
        .collect())
}
