//! Issue repository — the lease lifecycle lives here (§06). Lease writes are
//! conditional single-statement updates so two claimants cannot both win, and
//! the engine's all-or-nothing lease CHECK backs every transition.

use sqlx::SqlitePool;
use surge_domain::board::{Gate2State, Issue, Lease, OrchestrationStatus};

fn gate2_columns(g: &Gate2State) -> (&'static str, Option<String>, Option<i64>) {
    match g {
        Gate2State::Pending => ("pending", None, None),
        Gate2State::Reviewed { by, at } => ("reviewed", Some(by.clone()), Some(*at)),
    }
}

pub async fn insert(pool: &SqlitePool, i: &Issue) -> anyhow::Result<()> {
    anyhow::ensure!(i.lease.is_none(), "leases are claimed, never inserted");
    let status = i.status.as_str();
    let (gate2, g_by, g_at) = gate2_columns(&i.gate2);
    let wave_int = i.is_wave_integration as i64;
    sqlx::query!(
        "INSERT INTO issue (id, project_id, title, wave, phase, status, work_order_hash,
                            gate2, gate2_reviewed_by, gate2_reviewed_at, retry_count,
                            disposition, priority, is_wave_integration, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        i.id, i.project_id, i.title, i.wave, i.phase, status, i.work_order_hash,
        gate2, g_by, g_at, i.retry_count, i.disposition, i.priority, wave_int, i.created_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Issue>> {
    let row = sqlx::query!(
        "SELECT id, project_id, title, wave, phase, status, work_order_hash,
                gate2, gate2_reviewed_by, gate2_reviewed_at,
                lease_owner, lease_run_id, lease_expires_at, lease_heartbeat_at,
                retry_count, disposition, priority, is_wave_integration, created_at
         FROM issue WHERE id = ?",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Issue {
        id: r.id,
        project_id: r.project_id,
        title: r.title,
        wave: r.wave,
        phase: r.phase,
        status: parse_status(&r.status),
        work_order_hash: r.work_order_hash,
        gate2: match r.gate2.as_str() {
            "reviewed" => Gate2State::Reviewed {
                by: r.gate2_reviewed_by.expect("CHECK"),
                at: r.gate2_reviewed_at.expect("CHECK"),
            },
            _ => Gate2State::Pending,
        },
        lease: r.lease_owner.map(|owner| Lease {
            owner,
            run_id: r.lease_run_id.expect("CHECK: all-or-nothing lease"),
            expires_at: r.lease_expires_at.expect("CHECK"),
            last_heartbeat_at: r.lease_heartbeat_at.expect("CHECK"),
        }),
        retry_count: r.retry_count,
        disposition: r.disposition,
        priority: r.priority,
        is_wave_integration: r.is_wave_integration != 0,
        created_at: r.created_at,
    }))
}

fn parse_status(s: &str) -> OrchestrationStatus {
    match s {
        "draft" => OrchestrationStatus::Draft,
        "eligible" => OrchestrationStatus::Eligible,
        "dispatched" => OrchestrationStatus::Dispatched,
        "leased" => OrchestrationStatus::Leased,
        "verifying" => OrchestrationStatus::Verifying,
        "verified" => OrchestrationStatus::Verified,
        "failed" => OrchestrationStatus::Failed,
        "aborted" => OrchestrationStatus::Aborted,
        _ => OrchestrationStatus::Cut,
    }
}

/// Claim the lease: single conditional UPDATE — only an unleased, eligible
/// issue can be claimed, and exactly one claimant wins (INV-EXEC-1).
pub async fn claim_lease(
    pool: &SqlitePool,
    issue_id: &str,
    owner: &str,
    run_id: &str,
    now: i64,
    ttl_ms: i64,
) -> anyhow::Result<bool> {
    let expires = now + ttl_ms;
    let res = sqlx::query!(
        "UPDATE issue SET status = 'leased', lease_owner = ?, lease_run_id = ?,
                          lease_expires_at = ?, lease_heartbeat_at = ?
         WHERE id = ? AND lease_owner IS NULL AND status = 'eligible'",
        owner,
        run_id,
        expires,
        now,
        issue_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Heartbeat: refresh the lease clock (§06 — expiry follows the last beat).
pub async fn heartbeat(
    pool: &SqlitePool,
    issue_id: &str,
    now: i64,
    ttl_ms: i64,
) -> anyhow::Result<bool> {
    let expires = now + ttl_ms;
    let res = sqlx::query!(
        "UPDATE issue SET lease_heartbeat_at = ?, lease_expires_at = ?
         WHERE id = ? AND lease_owner IS NOT NULL",
        now,
        expires,
        issue_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// End the lease and record the terminal status derived from Surge-observed
/// facts (INV-EXEC-3) — exit codes and supervisor metering, never span content.
pub async fn release_lease(
    pool: &SqlitePool,
    issue_id: &str,
    status: OrchestrationStatus,
) -> anyhow::Result<()> {
    let status_s = status.as_str();
    sqlx::query!(
        "UPDATE issue SET status = ?, lease_owner = NULL, lease_run_id = NULL,
                          lease_expires_at = NULL, lease_heartbeat_at = NULL
         WHERE id = ?",
        status_s,
        issue_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One held lease, flattened for the supervisor's boot reconcile and its
/// standing sweeper: enough to terminalize the run, release the lease and
/// name the worktree without a second query per row (smoke walk 3, N2).
#[derive(Debug, Clone)]
pub struct HeldLease {
    pub issue_id: String,
    pub project_id: String,
    pub run_id: String,
    pub expires_at: i64,
}

/// Every lease currently held, whatever its clock says. The sweeper reads
/// this on its own schedule: TTL enforcement must not depend on a per-run
/// monitor task existing, since that task dies with the process (N2).
pub async fn held_leases(pool: &SqlitePool) -> anyhow::Result<Vec<HeldLease>> {
    let rows = sqlx::query!(
        "SELECT id, project_id, lease_run_id, lease_expires_at
         FROM issue WHERE lease_owner IS NOT NULL
         ORDER BY lease_expires_at"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| HeldLease {
            issue_id: r.id,
            project_id: r.project_id,
            run_id: r.lease_run_id.expect("CHECK: all-or-nothing lease"),
            expires_at: r.lease_expires_at.expect("CHECK"),
        })
        .collect())
}

/// Operator recovery: move a terminally-failed issue back to eligible so it
/// can be dispatched again (§06 — the retry count is on the card). Guarded to
/// terminal, unleased states: `verified` work is done, and a `leased` issue
/// has a live claimant. Auto-retry policy and the cap-at-3 rule are the
/// dispatcher's (Phase 2); this is the human-initiated path that keeps a
/// reconciled run from being a dead end (smoke walk 3, N2).
pub async fn mark_eligible_again(pool: &SqlitePool, issue_id: &str) -> anyhow::Result<bool> {
    let res = sqlx::query!(
        "UPDATE issue SET status = 'eligible', retry_count = retry_count + 1
         WHERE id = ? AND lease_owner IS NULL AND status IN ('failed', 'aborted')",
        issue_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}
