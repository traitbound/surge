//! Run & span repository. The span tree is a parent-id self-reference read
//! back in depth-first order by recursive CTE (ADR-2).

use sqlx::SqlitePool;
use surge_domain::observatory::{Run, RunKind, RunStatus, Span, SpanRole, SpanStatus};

pub async fn insert_run(pool: &SqlitePool, run: &Run) -> anyhow::Result<()> {
    let kind = run.kind.as_str();
    let status = run.status.as_str();
    sqlx::query!(
        "INSERT INTO run (id, project_id, issue_id, kind, materialization_hash,
                          work_order_hash, status, started_at, ended_at, cost)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        run.id,
        run.project_id,
        run.issue_id,
        kind,
        run.materialization_hash,
        run.work_order_hash,
        status,
        run.started_at,
        run.ended_at,
        run.cost
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn append_span(pool: &SqlitePool, span: &Span) -> anyhow::Result<()> {
    let role = span.role.as_str();
    let status = span.status.as_str();
    sqlx::query!(
        "INSERT INTO span (id, run_id, parent_span_id, node_id, role, started_at,
                           duration_ms, status, cost, depth, policy_decision, body)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        span.id,
        span.run_id,
        span.parent_span_id,
        span.node_id,
        role,
        span.started_at,
        span.duration_ms,
        status,
        span.cost,
        span.depth,
        span.policy_decision,
        span.body
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn parse_run_kind(s: &str) -> RunKind {
    match s {
        "doc" => RunKind::Doc,
        _ => RunKind::WorkOrder,
    }
}

fn parse_run_status(s: &str) -> RunStatus {
    match s {
        "running" => RunStatus::Running,
        "succeeded" => RunStatus::Succeeded,
        "failed" => RunStatus::Failed,
        "aborted" => RunStatus::Aborted,
        _ => RunStatus::Refused,
    }
}

fn parse_role(s: &str) -> SpanRole {
    match s {
        "coordinator" => SpanRole::Coordinator,
        "verifier" => SpanRole::Verifier,
        _ => SpanRole::Worker,
    }
}

fn parse_span_status(s: &str) -> SpanStatus {
    match s {
        "running" => SpanStatus::Running,
        "ok" => SpanStatus::Ok,
        "error" => SpanStatus::Error,
        _ => SpanStatus::Refused,
    }
}

pub async fn get_run(pool: &SqlitePool, run_id: &str) -> anyhow::Result<Run> {
    let r = sqlx::query!(
        "SELECT id, project_id, issue_id, kind, materialization_hash,
                work_order_hash, status, started_at, ended_at, cost
         FROM run WHERE id = ?",
        run_id
    )
    .fetch_one(pool)
    .await?;
    Ok(Run {
        id: r.id,
        project_id: r.project_id,
        issue_id: r.issue_id,
        kind: parse_run_kind(&r.kind),
        materialization_hash: r.materialization_hash,
        work_order_hash: r.work_order_hash,
        status: parse_run_status(&r.status),
        started_at: r.started_at,
        ended_at: r.ended_at,
        cost: r.cost,
    })
}

/// Runs newest-first, optionally scoped to one project — the Observatory's
/// run list (phase 0: polling read, no SSE).
pub async fn list_runs(pool: &SqlitePool, project_id: Option<&str>) -> anyhow::Result<Vec<Run>> {
    let rows = sqlx::query!(
        "SELECT id, project_id, issue_id, kind, materialization_hash,
                work_order_hash, status, started_at, ended_at, cost
         FROM run
         WHERE (?1 IS NULL OR project_id = ?1)
         ORDER BY started_at DESC, id DESC",
        project_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Run {
            id: r.id,
            project_id: r.project_id,
            issue_id: r.issue_id,
            kind: parse_run_kind(&r.kind),
            materialization_hash: r.materialization_hash,
            work_order_hash: r.work_order_hash,
            status: parse_run_status(&r.status),
            started_at: r.started_at,
            ended_at: r.ended_at,
            cost: r.cost,
        })
        .collect())
}

/// Runs still marked `running` — what a fresh process sees of the runs the
/// previous one was watching when it died. The supervisor's boot reconcile
/// owns none of them (smoke walk 3, N2).
pub async fn running_runs(pool: &SqlitePool) -> anyhow::Result<Vec<Run>> {
    let rows = sqlx::query!(
        "SELECT id, project_id, issue_id, kind, materialization_hash,
                work_order_hash, status, started_at, ended_at, cost
         FROM run WHERE status = 'running' ORDER BY started_at"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Run {
            id: r.id,
            project_id: r.project_id,
            issue_id: r.issue_id,
            kind: parse_run_kind(&r.kind),
            materialization_hash: r.materialization_hash,
            work_order_hash: r.work_order_hash,
            status: parse_run_status(&r.status),
            started_at: r.started_at,
            ended_at: r.ended_at,
            cost: r.cost,
        })
        .collect())
}

/// The run's span tree in depth-first order: children under their parent,
/// siblings by start time. The Observatory's waterfall reads this directly.
pub async fn span_tree(pool: &SqlitePool, run_id: &str) -> anyhow::Result<Vec<Span>> {
    let rows = sqlx::query!(
        r#"WITH RECURSIVE tree(id, sort_path) AS (
               SELECT s.id, printf('%012d', s.started_at)
               FROM span s
               WHERE s.run_id = ?1 AND s.parent_span_id IS NULL
               UNION ALL
               SELECT s.id, t.sort_path || '/' || printf('%012d', s.started_at)
               FROM span s
               JOIN tree t ON s.parent_span_id = t.id
               WHERE s.run_id = ?1
           )
           SELECT s.id AS "id!: String", s.run_id, s.parent_span_id, s.node_id, s.role,
                  s.started_at, s.duration_ms, s.status, s.cost, s.depth,
                  s.policy_decision, s.body
           FROM tree t JOIN span s ON s.id = t.id
           ORDER BY t.sort_path"#,
        run_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Span {
            id: r.id,
            run_id: r.run_id,
            parent_span_id: r.parent_span_id,
            node_id: r.node_id,
            role: parse_role(&r.role),
            started_at: r.started_at,
            duration_ms: r.duration_ms,
            status: parse_span_status(&r.status),
            cost: r.cost,
            depth: r.depth,
            policy_decision: r.policy_decision,
            body: r.body,
        })
        .collect())
}

/// Terminal transition observed by the supervisor (INV-EXEC-3). Guarded:
/// only a still-running run moves — an abort that already landed stands.
pub async fn finish_run_if_running(
    pool: &SqlitePool,
    run_id: &str,
    status: RunStatus,
    ended_at: i64,
) -> anyhow::Result<bool> {
    let status_s = status.as_str();
    let res = sqlx::query!(
        "UPDATE run SET status = ?, ended_at = ? WHERE id = ? AND status = 'running'",
        status_s,
        ended_at,
        run_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Resolve every span still `running` on a run the supervisor has just
/// observed end (smoke walk 3, N4-residual). A worker can open a span and
/// never close it — the tool schema permits `status: "running"` — and Surge
/// cannot ask it what happened afterwards: the runtime token's five
/// capabilities (INV-AUTH-1) deliberately include no close-span call, and
/// widening them would let the supervised side mutate what Surge recorded
/// (INV-EXEC-3). So the resolution comes from the Surge-observed fact that
/// the run ended.
///
/// `error`, not `ok`: the span genuinely never reported completion, and `ok`
/// would claim an outcome nobody recorded — while `refused` means Surge
/// declined something, which is not what happened. (A distinct
/// `unresolved` status would read better still, but SpanStatus is a
/// ts-rs-exported enum with a schema CHECK and UI rendering behind it;
/// that is a wider change than this finding.) The reason lands in the policy
/// field, which survives compaction (INV-OBS-2), and the worker's body and
/// timings are left exactly as it wrote them.
pub async fn resolve_dangling_spans(
    pool: &SqlitePool,
    run_id: &str,
    reason: &str,
) -> anyhow::Result<u64> {
    let res = sqlx::query!(
        "UPDATE span SET status = 'error', policy_decision = ?
         WHERE run_id = ? AND status = 'running'",
        reason,
        run_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// The abort ledger write (§06): takes effect at the executor's next status
/// poll. Returns false if the run was already terminal.
pub async fn abort_run(pool: &SqlitePool, run_id: &str, now: i64) -> anyhow::Result<bool> {
    let res = sqlx::query!(
        "UPDATE run SET status = 'aborted', ended_at = ? WHERE id = ? AND status = 'running'",
        now,
        run_id
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}
