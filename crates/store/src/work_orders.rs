//! Work-order repository. The rendered file is one of the five repo writes
//! (INV-DATA-1), gitignored and reproducible (INV-DATA-7); the row remembers
//! what was rendered so hash mismatches are detectable (design §05).

use sqlx::SqlitePool;
use surge_domain::board::WorkOrder;

pub async fn insert(pool: &SqlitePool, wo: &WorkOrder) -> anyhow::Result<()> {
    sqlx::query!(
        "INSERT INTO work_order (id, issue_id, path, revision, content_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        wo.id, wo.issue_id, wo.path, wo.revision, wo.content_hash, wo.created_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn latest_for_issue(pool: &SqlitePool, issue_id: &str) -> anyhow::Result<Option<WorkOrder>> {
    let row = sqlx::query!(
        "SELECT id, issue_id, path, revision, content_hash, created_at
         FROM work_order WHERE issue_id = ? ORDER BY revision DESC LIMIT 1",
        issue_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| WorkOrder {
        id: r.id,
        issue_id: r.issue_id,
        path: r.path,
        revision: r.revision,
        content_hash: r.content_hash,
        created_at: r.created_at,
    }))
}
