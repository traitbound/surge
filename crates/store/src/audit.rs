//! Audit repository (INV-OBS-1). Append and read — never update, never delete.
//!
//! `record` is generic over the executor so a caller can pass `&mut *tx` and
//! land the audit entry in the SAME transaction as the state change it
//! describes, which is what INV-DATA-8 actually requires. Passing `&pool`
//! still works and is correct only where nothing else is being committed —
//! a refusal that changed no state, for instance. Anywhere state moved, use
//! the transaction: a crash between two autocommits produced a privileged act
//! with no audit trail, which is precisely the hole INV-OBS-1 forbids
//! (review 2026-08-26).

use sqlx::{Executor, Sqlite, SqlitePool};
use surge_domain::audit::AuditEntry;

pub async fn record<'e, E>(
    executor: E,
    action: &str,
    subject: &str,
    actor: &str,
    project_id: Option<&str>,
    at: i64,
) -> anyhow::Result<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let res = sqlx::query!(
        "INSERT INTO audit_entry (action, subject, actor, project_id, at)
         VALUES (?, ?, ?, ?, ?)",
        action,
        subject,
        actor,
        project_id,
        at
    )
    .execute(executor)
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
