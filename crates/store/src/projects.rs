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
