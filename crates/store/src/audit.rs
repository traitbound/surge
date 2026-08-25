//! Audit repository (INV-OBS-1). Append and read — never update, never delete.

use sqlx::SqlitePool;
use surge_domain::audit::AuditEntry;

pub async fn record(
    pool: &SqlitePool,
    action: &str,
    subject: &str,
    actor: &str,
    project_id: Option<&str>,
    at: i64,
) -> anyhow::Result<i64> {
    let res = sqlx::query!(
        "INSERT INTO audit_entry (action, subject, actor, project_id, at)
         VALUES (?, ?, ?, ?, ?)",
        action,
        subject,
        actor,
        project_id,
        at
    )
    .execute(pool)
    .await?;
    Ok(res.last_insert_rowid())
}

pub async fn recent(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
    let rows = sqlx::query!(
        "SELECT id, action, subject, actor, project_id, at
         FROM audit_entry ORDER BY id DESC LIMIT ?",
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditEntry {
            id: r.id,
            action: r.action,
            subject: r.subject,
            actor: r.actor,
            project_id: r.project_id,
            at: r.at,
        })
        .collect())
}
