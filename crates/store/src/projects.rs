//! Project repository — project rows scope runtime tokens; binding
//! (`surge.yaml` written into the repo, INV-DATA-1) is recorded on the row via
//! [`mark_surge_yaml_written`].
//!
//! `Project::pipeline_status` is **derived here, not stored** (ESC-3): the read
//! paths compute it from `materialization.fresh`, the same signal
//! `supervisor::dispatch_issue` gates on (INV-ID-1), so the badge and the
//! dispatch decision cannot disagree. The `project.pipeline_status` column from
//! migration 0002 has never had a writer and is now read by nothing either; it
//! stays until a migration that rebuilds `project` for another reason removes
//! it (SQLite refuses `DROP COLUMN` on a column named in a CHECK constraint, so
//! dropping it alone means a full table rebuild).

use sqlx::SqlitePool;
use surge_domain::project::{Project, TrackerKind};

/// Insert a project row. `p.pipeline_status` is not persisted — it is derived
/// on every read (see the module note); a caller cannot assert a project into
/// a compiled state.
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
///
/// `has_fresh_materialization` comes from the row's `EXISTS` subquery, so the
/// status is a fact about the store at read time rather than a remembered one.
#[allow(clippy::too_many_arguments)]
fn project_from(
    id: String,
    name: String,
    repo_path: String,
    has_fresh_materialization: bool,
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
        pipeline_status: if has_fresh_materialization {
            surge_domain::project::PipelineAssignmentStatus::Published
        } else {
            surge_domain::project::PipelineAssignmentStatus::NotCompiled
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
        "SELECT id, name, repo_path, surge_yaml_written, tracker,
                branch_format, created_at,
                EXISTS(SELECT 1 FROM materialization m
                       WHERE m.project_id = project.id AND m.fresh = 1) AS has_fresh
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
            r.has_fresh != 0,
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
        "SELECT id, name, repo_path, surge_yaml_written, tracker,
                branch_format, created_at,
                EXISTS(SELECT 1 FROM materialization m
                       WHERE m.project_id = project.id AND m.fresh = 1) AS has_fresh
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
                r.has_fresh != 0,
                r.surge_yaml_written,
                r.tracker,
                r.branch_format,
                r.created_at,
            )
        })
        .collect())
}
