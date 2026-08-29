//! Materialization repository (INV-ID-1). Inserting a fresh materialization
//! and staling the project's previous ones happen on a caller-supplied
//! connection so BOTH commit with the caller's audit entry in one transaction
//! (INV-DATA-8).
//!
//! The previous note here said INV-DATA-8 "lives in the server handler until
//! the event bus lands" — conflating this invariant with ADR-3's commit
//! broadcast. The broadcast is legitimately deferred to Phase 2; the
//! invariant never was, and that sentence is how the gap stayed invisible
//! through five smoke walks (review 2026-08-26).

use sqlx::SqlitePool;
use surge_domain::materialization::Materialization;

/// Record the project's fresh materialization, staling any previous fresh
/// ones, atomically. Recompiling identical content is a cache hit: the same
/// cache key re-freshens the existing row rather than minting a duplicate
/// identity (INV-ID-1: one hash, one materialization).
/// Takes a connection rather than the pool so the caller can compose this
/// with its audit entry in ONE transaction (INV-DATA-8). Compiling changes
/// dispatch eligibility (INV-ID-1); a crash between the state change and the
/// audit row left a privileged act unrecorded.
pub async fn insert_fresh(
    conn: &mut sqlx::SqliteConnection,
    m: &Materialization,
) -> anyhow::Result<()> {
    sqlx::query!("UPDATE materialization SET fresh = 0 WHERE project_id = ? AND fresh = 1", m.project_id)
        .execute(&mut *conn)
        .await?;
    // Deliberately NOT `m.fresh` (ESC-5 / ESC-3 follow-up). This function's
    // name is the contract: it inserts a fresh materialization, staling the
    // predecessors above in the same statement's atomicity. Reading a
    // caller-supplied `fresh` here would let a caller passing `fresh: false`
    // stale every predecessor and then insert a non-fresh successor,
    // producing a project with materialization rows where none is fresh —
    // exactly the state `PipelineAssignmentStatus::NotCompiled`'s derivation
    // (ESC-3) is built on being unreachable. Hardcoding the literal makes
    // that state unwritable through this function rather than merely
    // rejected by a runtime check. `Materialization::fresh` stays meaningful
    // on the way out (`fresh_for_project` reads the real column); it is only
    // ignored on the way in, here.
    sqlx::query!(
        "INSERT INTO materialization (id, content_hash, cache_key, pipeline_id, project_id,
                                      signed_by, fresh, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?)
         ON CONFLICT(cache_key) DO UPDATE SET fresh = excluded.fresh,
                                              created_at = excluded.created_at",
        m.id,
        m.content_hash,
        m.cache_key,
        m.pipeline_id,
        m.project_id,
        m.signed_by,
        m.created_at
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Standalone insert for callers with nothing else to commit (fixtures,
/// tests). Production paths use [`insert_fresh`] inside their own transaction
/// so the audit entry lands with it (INV-DATA-8).
pub async fn insert_fresh_committed(pool: &SqlitePool, m: &Materialization) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    insert_fresh(&mut tx, m).await?;
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
