//! Materialization repository (INV-ID-1). Inserting a fresh materialization
//! and staling the project's previous ones commit as one transaction with the
//! caller's audit entry kept adjacent (INV-DATA-8 discipline lives in the
//! server handler until the event bus lands).

use sqlx::SqlitePool;
use surge_domain::materialization::Materialization;

/// Record the project's fresh materialization, staling any previous fresh
/// ones, atomically. Recompiling identical content is a cache hit: the same
/// cache key re-freshens the existing row rather than minting a duplicate
/// identity (INV-ID-1: one hash, one materialization).
pub async fn insert_fresh(pool: &SqlitePool, m: &Materialization) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query!("UPDATE materialization SET fresh = 0 WHERE project_id = ? AND fresh = 1", m.project_id)
        .execute(&mut *tx)
        .await?;
    let fresh = m.fresh as i64;
    sqlx::query!(
        "INSERT INTO materialization (id, content_hash, cache_key, pipeline_id, project_id,
                                      signed_by, fresh, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(cache_key) DO UPDATE SET fresh = excluded.fresh,
                                              created_at = excluded.created_at",
        m.id,
        m.content_hash,
        m.cache_key,
        m.pipeline_id,
        m.project_id,
        m.signed_by,
        fresh,
        m.created_at
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The project's current fresh materialization — dispatch checks this first
/// (INV-ID-1: stale → refusal).
pub async fn fresh_for_project(
    pool: &SqlitePool,
    project_id: &str,
) -> anyhow::Result<Option<Materialization>> {
    let row = sqlx::query!(
        "SELECT id, content_hash, cache_key, pipeline_id, project_id, signed_by, fresh, created_at
         FROM materialization WHERE project_id = ? AND fresh = 1",
        project_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Materialization {
        id: r.id,
        content_hash: r.content_hash,
        cache_key: r.cache_key,
        pipeline_id: r.pipeline_id,
        project_id: r.project_id,
        signed_by: r.signed_by,
        fresh: r.fresh != 0,
        created_at: r.created_at,
    }))
}
