//! The runtime supervisor — Surge owns the actuator (ADR-5, INV-EXEC-1/2/3).
//! Dispatch: fresh-materialization check → work-order hash check → lease →
//! git worktree on the task branch (outside the bound repo: worktrees live in
//! Surge's work dir, since INV-DATA-1 closes the repo write list) → compile
//! into it → spawn one headless worker with the runtime token injected as env
//! (INV-AUTH-4) → watch. Terminal state derives from exit codes and the lease
//! clock only — never span content (INV-EXEC-3).
//!
//! Lease TTL is enforced from three places, deliberately: the per-run
//! [`monitor`] (fast path, dies with the process), the standing
//! [`spawn_lease_sweeper`] task (backstop for runs no monitor is watching),
//! and [`reconcile_orphans`] at boot / [`drain_on_shutdown`] at Ctrl-C
//! (nothing survives a process boundary as `running`). All three funnel
//! through the same guarded terminalization, so they can race without
//! double-writing (smoke walk 3, N2). Terminalizing also resolves the spans
//! a worker opened and never closed: closing them is not a capability the
//! runtime has (INV-AUTH-1), so the supervisor's own observation is what
//! resolves them (N4-residual).

use crate::{now_ms, AppState};
use std::path::PathBuf;
use std::process::Stdio;
use surge_domain::board::OrchestrationStatus;
use surge_domain::observatory::{Run, RunKind, RunStatus, Span, SpanRole, SpanStatus};

#[derive(Clone)]
pub struct SupervisorConfig {
    /// Worker command. The work order is delivered on the worker's STDIN,
    /// never as a positional argument — Claude Code's `--mcp-config` is
    /// variadic and swallows a following positional (smoke 2026-08-25, F2).
    /// Default is the real thing; tests substitute a script (INV-EXEC-1 cares
    /// who spawns, not what).
    pub worker_cmd: Vec<String>,
    /// §06: TTL 10 minutes.
    pub lease_ttl_ms: i64,
    /// Parent directory for per-lease worktrees (never inside a bound repo).
    pub work_dir: PathBuf,
    /// Injected as SURGE_API so hooks/tools can reach Surge (INV-DEPLOY-1 exemption).
    pub api_base: String,
    /// Where the Claude Code plugin lives; injected as SURGE_PLUGIN_DIR — the
    /// compiled .claude/mcp.json and settings.json hooks resolve against it.
    /// It has no default: see [`SupervisorConfig::new`].
    pub plugin_dir: PathBuf,
    /// How often the watchdog looks at the lease clock and the child.
    pub poll_ms: u64,
    /// How often the standing lease sweeper runs — and, doubling as its
    /// grace, how far past expiry a lease must be before the sweeper touches
    /// it, so a live [`monitor`] (polling `poll_ms`) always wins the race (N2).
    pub sweep_ms: u64,
}

/// Deliberately not a path anything could half-work from: a spawn against it
/// fails `plugin_assets::verify` by name, in the operator's face. The old
/// cwd-relative default (`integrations/claude-plugin`) resolved to *nothing*
/// outside the source checkout and Claude Code tolerated it silently — the
/// NEW-1 P0 (smoke walk 3, N14 kept it disarmed).
const UNCONFIGURED_PLUGIN_DIR: &str = "/nonexistent/surge-plugin-dir-not-configured";

impl SupervisorConfig {
    /// Every field but `plugin_dir` has a safe shipped default; `plugin_dir`
    /// gets no default at all, because a *plausible* wrong value there is the
    /// exact shape of the NEW-1 P0 — workers that start, find no MCP tools
    /// and no hooks, and exit 0 having done nothing. `Default` is therefore
    /// not implemented for this type: a future `..Default::default()` caller
    /// cannot silently re-arm the guess, it fails to compile and has to say
    /// where the plugin tree is (smoke walk 3, N14).
    pub fn new(plugin_dir: PathBuf) -> Self {
        Self {
            worker_cmd: vec![
                "claude".into(),
                "-p".into(),
                "--mcp-config".into(),
                ".claude/mcp.json".into(),
            ],
            lease_ttl_ms: 10 * 60 * 1000,
            work_dir: PathBuf::from("surge-worktrees"),
            api_base: format!("http://{}", crate::BIND),
            plugin_dir,
            poll_ms: 500,
            sweep_ms: 5_000,
        }
    }

    /// A supervisor for surfaces that never dispatch (read-only API tests,
    /// [`crate::AppState::new`]). Spelled out rather than defaulted: it
    /// refuses at spawn instead of running a blind worker.
    pub fn unconfigured() -> Self {
        Self::new(PathBuf::from(UNCONFIGURED_PLUGIN_DIR))
    }
}

#[derive(Debug)]
pub enum DispatchOutcome {
    /// The run id of the spawned worker's run.
    Spawned { run_id: String },
    /// Refused — and the refusal itself produced a run with one span carrying
    /// the reason (INV-ERR-1, design §06).
    Refused { run_id: String, reason: String },
    /// There is no such issue, so there is no project, no materialization and
    /// therefore no run to hang a span on — the visible record is the audit
    /// row alone. It used to be an `anyhow!` that `human_api::internal`
    /// flattened into `500 {"error":"dispatch failed"}` with the reason only
    /// on the server's stderr: the first thing a new operator hits after a
    /// typo, and the last spanless *and* auditless refusal in the product
    /// (smoke walk 5, F4).
    NotFound { reason: String },
}

/// Path config may be given relative; consumers resolve it against the
/// server's cwd, ONCE, here — `git -C <repo>` would otherwise resolve the
/// same string against the bound repo and write a worktree inside it
/// (smoke 2026-08-25, F1/F3; INV-DATA-1, INV-EXEC-2).
fn absolutize(p: &std::path::Path) -> anyhow::Result<PathBuf> {
    Ok(std::path::absolute(p)?)
}

/// Span ids the supervisor reserves for its own termination and refusal
/// records. `span.id` is the primary key, so a worker that pre-inserted one
/// of these for another run made the supervisor's later write fail on a
/// collision — and every one of those writes was a swallowed `let _ =`, so
/// the effect was a termination record silently missing, defeating INV-ERR-1
/// invisibly (concurrency review 2026-08-26). `runtime_api::append_span`
/// refuses them at the door; [`log_span_failure`] makes the collision that
/// remains theoretically possible loud rather than silent.
///
/// The generic refusal span is `sp_{run_id}` and every run id starts with
/// `run_`, so `sp_run_` covers that whole family; worker ids
/// (`sp_w_…`, `sp_open_…`, the plugin's `sp_<hex>`) are untouched by design.
const RESERVED_SPAN_PREFIXES: [&str; 5] =
    ["sp_end_", "sp_fail_", "sp_orphan_", "sp_abort_", "sp_run_"];

pub(crate) fn is_reserved_span_id(span_id: &str) -> bool {
    RESERVED_SPAN_PREFIXES.iter().any(|p| span_id.starts_with(p))
}

/// A supervisor span write is INV-ERR-1's visible record; a swallowed failure
/// here is a run that stopped for no stated reason. None of these calls can
/// propagate — every caller is already unwinding something — so they are
/// logged loudly instead of dropped (concurrency review 2026-08-26).
pub(crate) fn log_span_failure(res: anyhow::Result<()>, what: &str, run_id: &str) {
    if let Err(e) = res {
        eprintln!("SUPERVISOR SPAN WRITE FAILED ({what}) for run {run_id}: {e}");
    }
}

/// A refusal is data: a two-second run whose single span carries the reason.
async fn refusal_run(
    state: &AppState,
    project_id: &str,
    issue_id: &str,
    wo_hash: &str,
    mat_hash: &str,
    reason: &str,
) -> anyhow::Result<String> {
    let now = now_ms();
    let run_id = format!("run_{}", &surge_store::tokens::hash(&format!("{issue_id}{now}"))[..12]);
    let run = Run {
        id: run_id.clone(),
        project_id: project_id.into(),
        issue_id: Some(issue_id.into()),
        kind: RunKind::WorkOrder,
        materialization_hash: mat_hash.into(),
        work_order_hash: Some(wo_hash.into()),
        status: RunStatus::Refused,
        started_at: now,
        ended_at: Some(now),
        cost: 0.0,
    };
    surge_store::observatory::insert_run(&state.pool, &run).await?;
    refusal_span(state, &run_id, reason, now).await?;
    surge_store::audit::record(&state.pool, "dispatch.refused", reason, "human", Some(project_id), now)
        .await?;
    Ok(run_id)
}

/// The span that makes a refusal visible (INV-ERR-1, phase.md:43). Every
/// refusal branch appends it — the lease-lost branch used to write a refusal
/// run with no span at all, so the reason existed only in the HTTP response
/// and the audit row (smoke walk 3, N6).
/// The coordinator span recording a human abort (§06-06). Shares the shape of
/// every other terminalization span so the observatory never shows a run that
/// stopped for no stated reason (smoke walk 4, S4).
/// The abort reason span as a value, so it can be appended inside the same
/// transaction as the ledger write (INV-DATA-8).
pub(crate) fn abort_span_row(run_id: &str, now: i64) -> Span {
    let reason = "aborted by the operator — takes effect at the executor's next tool call; \
                  if heartbeats stop first, the lease reclaims at TTL (§06-06)";
    Span {
        id: format!("sp_abort_{run_id}"),
        run_id: run_id.to_string(),
        parent_span_id: None,
        node_id: None,
        role: SpanRole::Coordinator,
        started_at: now,
        duration_ms: Some(0),
        // `Error`, matching the orphan/drain spans: the run ended without
        // completing. Whether abnormal-but-deliberate deserves its own
        // SpanStatus is a Phase 1 call — the variant is ts-rs-exported with a
        // schema CHECK and UI pill mapping behind it.
        status: SpanStatus::Error,
        cost: 0.0,
        depth: 0,
        policy_decision: Some(reason.into()),
        body: Some(reason.into()),
    }
}

pub(crate) async fn refusal_span(
    state: &AppState,
    run_id: &str,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    surge_store::observatory::append_span(&state.pool, &Span {
        id: format!("sp_{run_id}"),
        run_id: run_id.to_string(),
        parent_span_id: None,
        node_id: None,
        role: SpanRole::Coordinator,
        started_at: now,
        duration_ms: Some(0),
        status: SpanStatus::Refused,
        cost: 0.0,
        depth: 0,
        policy_decision: Some(reason.into()),
        body: Some(reason.into()),
    })
    .await
}

/// Dispatch one issue (phase 0: single-task, no queue). See module docs for
/// the sequence; every spawn records run id, materialization hash and
/// work-order hash (INV-EXEC-1) via the run row itself.
pub async fn dispatch_issue(state: &AppState, issue_id: &str) -> anyhow::Result<DispatchOutcome> {
    let now = now_ms();
    let Some(issue) = surge_store::issues::get(&state.pool, issue_id).await? else {
        // F4: a typo is a refusal, not a server error. No run row is possible
        // (a run needs a project and a materialization hash), so the audit row
        // is the visible record and the reason travels in the response body.
        let reason = format!("dispatch refused — no such issue: {issue_id}");
        surge_store::audit::record(&state.pool, "dispatch.refused", &reason, "human", None, now)
            .await?;
        return Ok(DispatchOutcome::NotFound { reason });
    };
    let project = surge_store::projects::get(&state.pool, &issue.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("issue references unknown project"))?;

    // 1. Materialization first (§06-02): stale or absent means refusal.
    let mat = surge_store::materializations::fresh_for_project(&state.pool, &project.id).await?;
    let Some(mat) = mat else {
        let reason = "dispatch refused — no fresh materialization; compile first (INV-ID-1)";
        let run_id =
            refusal_run(state, &project.id, issue_id, &issue.work_order_hash, "none", reason).await?;
        return Ok(DispatchOutcome::Refused { run_id, reason: reason.into() });
    };

    // 2. Work-order hash check (design §05): the rendered file must be the
    //    one the issue pinned.
    let rendered = surge_compiler::work_order::render_work_order(&issue);
    let wo_hash = surge_compiler::work_order::work_order_hash(&rendered);
    if wo_hash != issue.work_order_hash {
        let reason = "dispatch refused — work order changed after issue generation (hash mismatch)";
        let run_id =
            refusal_run(state, &project.id, issue_id, &issue.work_order_hash, &mat.content_hash, reason)
                .await?;
        return Ok(DispatchOutcome::Refused { run_id, reason: reason.into() });
    }

    // 3. Run row + lease (one claimant wins).
    let run_id = format!("run_{}", &surge_store::tokens::hash(&format!("{issue_id}{now}"))[..12]);
    let run = Run {
        id: run_id.clone(),
        project_id: project.id.clone(),
        issue_id: Some(issue.id.clone()),
        kind: RunKind::WorkOrder,
        materialization_hash: mat.content_hash.clone(),
        work_order_hash: Some(wo_hash.clone()),
        status: RunStatus::Running,
        started_at: now,
        ended_at: None,
        cost: 0.0,
    };
    surge_store::observatory::insert_run(&state.pool, &run).await?;
    let cfg = &state.supervisor;
    if !surge_store::issues::claim_lease(&state.pool, issue_id, "worker-1", &run_id, now, cfg.lease_ttl_ms)
        .await?
    {
        let reason = "dispatch refused — issue is not eligible or already leased";
        surge_store::observatory::finish_run_if_running(&state.pool, &run_id, RunStatus::Refused, now)
            .await?;
        // Same visible record as every other refusal branch (N6).
        refusal_span(state, &run_id, reason, now).await?;
        surge_store::audit::record(&state.pool, "dispatch.refused", reason, "human", Some(&project.id), now)
            .await?;
        return Ok(DispatchOutcome::Refused { run_id, reason: reason.into() });
    }

    // 4. Worktree on the task branch (INV-EXEC-2), outside the bound repo —
    //    the path handed to `git -C <repo>` is absolute, always.
    let branch = project.branch_format.replace("{issue}", &issue.id);
    let work_root = absolutize(&cfg.work_dir)?;
    let worktree = work_root.join(&project.id).join(&issue.id);
    let setup = async {
        std::fs::create_dir_all(work_root.join(&project.id))?;
        git(&project.repo_path, &["worktree", "add", "-B", &branch, worktree.to_str().unwrap()])?;
        Ok::<_, anyhow::Error>(WorktreeGuard { repo: project.repo_path.clone().into(), dir: worktree.clone() })
    };
    let guard = match setup.await {
        Ok(g) => g,
        Err(e) => {
            unwind_dispatch(
                state,
                Unwind::Failed,
                &run_id,
                &project.id,
                Some(&issue.id),
                None,
                None,
                &format!("worktree creation failed: {e}"),
            )
            .await;
            return Err(e);
        }
    };

    // The credential is minted before the fallible work below so the failure
    // path can revoke it — an undelivered runtime token must not outlive the
    // dispatch that needed it (INV-AUTH-4; smoke walk 3, N1).
    let runtime_token = match surge_store::tokens::mint_for_run(
        &state.pool,
        surge_store::tokens::TokenKind::Runtime,
        Some(&project.id),
        Some(&run_id),
        now,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            unwind_dispatch(
                state,
                Unwind::Failed,
                &run_id,
                &project.id,
                Some(&issue.id),
                Some(guard),
                None,
                &format!("runtime token mint failed: {e}"),
            )
            .await;
            return Err(e);
        }
    };

    // 5+6. Compile into the worktree, render the work order, spawn — any
    // failure past the lease claim must not leak the lease, the running run,
    // the credential, or the worktree (smoke 2026-08-25, F4; smoke walk 3, N1).
    let prepared: anyhow::Result<String> = async {
        let (pipeline, nodes, edges) =
            surge_store::pipelines::load_graph(&state.pool, &mat.pipeline_id).await?;
        let mut library = surge_compiler::LibraryIndex::new();
        for (kind, r) in surge_compiler::referenced_items(&nodes) {
            if let Some(item) =
                surge_store::library::get(&state.pool, kind, &r.name, r.version).await?
            {
                library.insert((kind, r.name.clone(), r.version), item);
            }
        }
        let compiled = surge_compiler::compile(&pipeline, &nodes, &edges, &library, &project)
            .map_err(|r| anyhow::anyhow!("compile into worktree failed: {r}"))?;
        surge_compiler::write_to_repo(&worktree, &compiled)?;
        let wo_rel = format!("work_orders/{}.md", issue.id);
        let wo_abs = worktree.join(&wo_rel);
        std::fs::create_dir_all(wo_abs.parent().unwrap())?;
        std::fs::write(&wo_abs, &rendered)?;
        surge_store::work_orders::insert(&state.pool, &surge_domain::board::WorkOrder {
            id: format!("wo_{run_id}"),
            issue_id: issue.id.clone(),
            path: wo_rel.clone(),
            revision: 1,
            content_hash: wo_hash.clone(),
            created_at: now,
        })
        .await
        .ok(); // revision uniqueness: a redispatch reuses revision 1's content

        // The work order arrives on stdin (see worker_cmd docs) with a
        // pointer at the surge MCP tools.
        let prompt = format!(
            "{rendered}\nUse the surge MCP tools as you work: surge_fetch_work_order for the \
             authoritative work order and lease, surge_append_span for progress, \
             surge_heartbeat regularly, surge_poll_run to check for aborts.\n"
        );
        Ok(prompt)
    }
    .await;
    let prompt = match prepared {
        Ok(p) => p,
        Err(e) => {
            unwind_dispatch(
                state,
                Unwind::Failed,
                &run_id,
                &project.id,
                Some(&issue.id),
                Some(guard),
                Some(&runtime_token),
                &format!("worktree preparation failed: {e}"),
            )
            .await;
            return Err(e);
        }
    };

    // INV-ID-1 / §06-02, re-checked immediately before the spawn: the
    // freshness read at step 1 is several awaits old by now — worktree, token
    // mint, graph load, compile, work-order write — and a compile landing in
    // that window stales it. Traceability survives either way (the run records
    // the hash it truly ran under), but "stale materialization → dispatch
    // refused" has to mean the graph the worker actually starts on
    // (concurrency review 2026-08-26).
    let still_fresh = surge_store::materializations::fresh_for_project(&state.pool, &project.id)
        .await
        .map(|m| m.is_some_and(|m| m.content_hash == mat.content_hash))
        .unwrap_or(false);
    if !still_fresh {
        let reason = "dispatch refused — the materialization went stale while the worktree was \
                      being prepared; compile is the approval point (INV-ID-1, §06-02)";
        unwind_dispatch(
            state,
            Unwind::Refused,
            &run_id,
            &project.id,
            Some(&issue.id),
            Some(guard),
            Some(&runtime_token),
            reason,
        )
        .await;
        return Ok(DispatchOutcome::Refused { run_id, reason: reason.into() });
    }

    let spawned = spawn_worker(cfg, &worktree, &prompt, &[
        ("SURGE_RUN_ID", run_id.as_str()),
        ("SURGE_ISSUE_ID", issue.id.as_str()),
        ("SURGE_RUNTIME_TOKEN", runtime_token.as_str()),
    ]);
    let (child, stderr_tail) = match spawned {
        Ok(cs) => cs,
        Err(e) => {
            unwind_dispatch(
                state,
                Unwind::Failed,
                &run_id,
                &project.id,
                Some(&issue.id),
                Some(guard),
                Some(&runtime_token),
                &format!("worker spawn failed: {e}"),
            )
            .await;
            return Err(e);
        }
    };

    surge_store::audit::record(&state.pool, "run.dispatched", &run_id, "human", Some(&project.id), now)
        .await?;

    tokio::spawn(monitor(
        state.clone(),
        run_id.clone(),
        project.id.clone(),
        Some(issue.id.clone()),
        child,
        stderr_tail,
        Some(guard),
    ));
    Ok(DispatchOutcome::Spawned { run_id })
}

/// The human abort: ledger write, reason span, audit — in one place so the
/// HTTP handler and tests cannot drift (§06-06).
pub async fn abort_run(state: &AppState, run_id: &str) -> bool {
    let now = now_ms();
    let project_id = surge_store::observatory::get_run(&state.pool, run_id)
        .await
        .ok()
        .map(|r| r.project_id);
    // Ledger write, reason span and audit entry commit as one (INV-DATA-8):
    // abort is a privileged act, and a crash between the ledger and the audit
    // row left a stopped run nobody was recorded as stopping (INV-OBS-1).
    let aborted = async {
        let mut tx = state.pool.begin().await?;
        let moved = surge_store::observatory::abort_run(&mut *tx, run_id, now).await?;
        if moved {
            surge_store::observatory::append_span(&mut *tx, &abort_span_row(run_id, now)).await?;
            surge_store::audit::record(
                &mut *tx, "run.aborted", run_id, "human", project_id.as_deref(), now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok::<_, anyhow::Error>(moved)
    };
    match aborted.await {
        Ok(moved) => moved,
        Err(e) => {
            eprintln!("abort commit failed for {run_id}: {e}");
            false
        }
    }
}

type StderrTail = tokio::sync::oneshot::Receiver<String>;

/// What Surge writes onto a span a worker opened and never closed. The
/// runtime has no close-span capability by design (INV-AUTH-1's five), so the
/// only honest resolution is the supervisor's own observation that the run
/// ended (INV-EXEC-3) — see `store::observatory::resolve_dangling_spans` for
/// why the status becomes `error` rather than `ok` (smoke walk 3,
/// N4-residual).
const DANGLING_SPAN: &str =
    "span never reported completion — resolved when the supervisor observed the run end";

/// True when a run produced no spans of its own — the signature of a worker
/// that never reached Surge (NEW-2). Supervisor-written spans (`sp_end_*`,
/// `sp_fail_*`, `sp_orphan_*`) are not the worker's. A store read failure
/// returns false: a failed read must never manufacture a failure verdict.
async fn unobserved(state: &AppState, run_id: &str) -> bool {
    const SUPERVISOR_SPANS: [&str; 3] = ["sp_end_", "sp_fail_", "sp_orphan_"];
    surge_store::observatory::span_tree(&state.pool, run_id)
        .await
        .map(|spans| {
            !spans
                .iter()
                .any(|s| !SUPERVISOR_SPANS.iter().any(|p| s.id.starts_with(p)))
        })
        .unwrap_or(false)
}

/// Spawn a worker: prompt on stdin, stderr tailed into a channel so a failed
/// worker's actual error is a visible record, not a discarded pipe
/// (smoke 2026-08-25, F4; INV-ERR-1).
fn spawn_worker(
    cfg: &SupervisorConfig,
    dir: &std::path::Path,
    prompt: &str,
    extra_env: &[(&str, &str)],
) -> anyhow::Result<(tokio::process::Child, StderrTail)> {
    let plugin_dir = absolutize(&cfg.plugin_dir)?;
    // Fail loud, never spawn blind: without a real plugin tree the worker
    // gets no MCP tools and no hooks, so it cannot append spans, heartbeat,
    // or see an abort — and `claude -p` exits 0 regardless (NEW-1).
    crate::plugin_assets::verify(&plugin_dir)?;
    let mut cmd = tokio::process::Command::new(&cfg.worker_cmd[0]);
    cmd.args(&cfg.worker_cmd[1..])
        .current_dir(dir)
        .env("SURGE_API", &cfg.api_base)
        .env("SURGE_PLUGIN_DIR", &plugin_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = prompt.to_string();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(prompt.as_bytes()).await;
        });
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            // Keep only the last 4KB; keep draining so the child never blocks
            // on a full stderr pipe.
            let mut tail: Vec<u8> = Vec::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > 4096 {
                    tail.drain(..tail.len() - 4096);
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&tail).into_owned());
        });
    } else {
        let _ = tx.send(String::new());
    }
    Ok((child, rx))
}

/// The cleanup owed after a dispatch stops past the run row: run terminalized
/// with a reason span, lease released, credential revoked, worktree reaped,
/// audit written. Nothing leaks.
///
/// `issue_id` and `worktree` are `None` for a doc run (design §23-Fourteen):
/// it holds no lease and runs in the bound repo itself. That path used to
/// propagate spawn failure with `?` straight past this guard, leaving a
/// permanently `running` run, a live orphaned runtime token and no
/// `dispatch.failed` entry (smoke walk 3, N1). The audit row carries the
/// project, like `run.dispatched` does, so one project's dispatch lifecycle
/// is filterable end to end (N13).
/// Why a dispatch is being unwound past the run row. Both shapes owe the same
/// cleanup; they differ in what the record says happened.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unwind {
    /// Something broke (spawn, compile, worktree): the run is a failure and
    /// the issue is left `failed` for a human to retry.
    Failed,
    /// Surge declined to proceed (a stale materialization at the last check).
    /// Nothing was done to the issue, so it goes back to `eligible` — the
    /// same place the pre-lease refusal branches leave it.
    Refused,
}

#[allow(clippy::too_many_arguments)] // one cleanup path, every leak it must close
async fn unwind_dispatch(
    state: &AppState,
    unwind: Unwind,
    run_id: &str,
    project_id: &str,
    issue_id: Option<&str>,
    worktree: Option<WorktreeGuard>,
    runtime_token: Option<&str>,
    reason: &str,
) {
    let now = now_ms();
    let (run_status, issue_status, action) = match unwind {
        Unwind::Failed => (RunStatus::Failed, OrchestrationStatus::Failed, "dispatch.failed"),
        Unwind::Refused => {
            (RunStatus::Refused, OrchestrationStatus::Eligible, "dispatch.refused")
        }
    };
    let _ =
        surge_store::observatory::finish_run_if_running(&state.pool, run_id, run_status, now).await;
    let span = match unwind {
        Unwind::Failed => Span {
            id: format!("sp_fail_{run_id}"),
            run_id: run_id.to_string(),
            parent_span_id: None,
            node_id: None,
            role: SpanRole::Coordinator,
            started_at: now,
            duration_ms: Some(0),
            status: SpanStatus::Error,
            cost: 0.0,
            depth: 0,
            policy_decision: Some(reason.to_string()),
            body: Some(reason.to_string()),
        },
        // The same shape every other refusal run carries (N6).
        Unwind::Refused => Span {
            id: format!("sp_{run_id}"),
            run_id: run_id.to_string(),
            parent_span_id: None,
            node_id: None,
            role: SpanRole::Coordinator,
            started_at: now,
            duration_ms: Some(0),
            status: SpanStatus::Refused,
            cost: 0.0,
            depth: 0,
            policy_decision: Some(reason.to_string()),
            body: Some(reason.to_string()),
        },
    };
    log_span_failure(
        surge_store::observatory::append_span(&state.pool, &span).await,
        "dispatch unwind",
        run_id,
    );
    if let Some(issue_id) = issue_id {
        release_if_held(state, issue_id, run_id, issue_status).await;
    }
    if let Some(token) = runtime_token {
        let _ = surge_store::tokens::revoke(&state.pool, token, now).await;
    }
    if let Some(wt) = worktree {
        wt.reap();
    }
    let _ = surge_store::audit::record(&state.pool, action, reason, "supervisor", Some(project_id), now)
        .await;
}

pub struct WorktreeGuard {
    repo: PathBuf,
    dir: PathBuf,
}

impl WorktreeGuard {
    /// Reap at lease end (INV-EXEC-2).
    fn reap(&self) {
        if let Err(e) = git(
            self.repo.to_str().unwrap_or("."),
            &["worktree", "remove", "--force", self.dir.to_str().unwrap_or("")],
        ) {
            eprintln!("worktree reap failed for {:?}: {e}", self.dir);
        }
    }
}

fn git(repo: &str, args: &[&str]) -> anyhow::Result<()> {
    let out = std::process::Command::new("git").arg("-C").arg(repo).args(args).output()?;
    anyhow::ensure!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// Watch one worker: child exit → terminal status from the exit code; lease
/// silence past TTL → kill, reclaim, fail (§06-03). Abort is NOT enforced by
/// killing — it lands at the worker's next status poll (§06-06); the lease
/// clock is the backstop.
async fn monitor(
    state: AppState,
    run_id: String,
    project_id: String,
    issue_id: Option<String>,
    mut child: tokio::process::Child,
    stderr_tail: StderrTail,
    worktree: Option<WorktreeGuard>,
) {
    let poll = std::time::Duration::from_millis(state.supervisor.poll_ms);
    let (status, reason): (RunStatus, Option<String>) = loop {
        tokio::select! {
            exit = child.wait() => {
                match exit {
                    Ok(s) if s.success() => break (RunStatus::Succeeded, None),
                    Ok(s) => {
                        let code = s.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
                        let tail = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            stderr_tail,
                        )
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .unwrap_or_default();
                        let reason = if tail.trim().is_empty() {
                            format!("worker exited with {code}")
                        } else {
                            format!("worker exited with {code} — stderr tail: {}", tail.trim())
                        };
                        break (RunStatus::Failed, Some(reason));
                    }
                    Err(e) => break (RunStatus::Failed, Some(format!("wait failed: {e}"))),
                }
            }
            _ = tokio::time::sleep(poll) => {
                if let Some(issue_id) = &issue_id {
                    let lease_expired = surge_store::issues::get(&state.pool, issue_id).await
                        .ok()
                        .flatten()
                        .and_then(|i| i.lease)
                        .map(|l| now_ms() > l.expires_at)
                        .unwrap_or(false);
                    if lease_expired {
                        let _ = child.kill().await;
                        break (
                            RunStatus::Failed,
                            Some(format!(
                                "lease reclaimed — worker-1 stopped responding (TTL {}ms)",
                                state.supervisor.lease_ttl_ms
                            )),
                        );
                    }
                }
            }
        }
    };

    let now = now_ms();
    // Observability floor (NEW-2): a work-order run that exits 0 having
    // produced no spans and never heartbeated did not do the work — it never
    // reached Surge. Exit code alone would call that `verified` and flip the
    // issue, which is indistinguishable from success with nothing behind it.
    // Span count and heartbeat state are Surge-observed facts, so deriving
    // from them is INV-EXEC-3-clean.
    let (status, reason) = match (status, &issue_id) {
        (RunStatus::Succeeded, Some(_)) if unobserved(&state, &run_id).await => (
            RunStatus::Failed,
            Some(
                "worker exited 0 but appended no spans and never heartbeated — it could not \
                 reach Surge (check plugin registration in the compiled .claude/)"
                    .to_string(),
            ),
        ),
        (s, _) => (s, reason),
    };
    // If an abort already landed, it stands (finish_run_if_running is guarded).
    let moved = surge_store::observatory::finish_run_if_running(&state.pool, &run_id, status, now)
        .await
        .unwrap_or(false);
    // The worker process is gone, so its credential has no further legitimate
    // use — revoke unconditionally, including on the abort path where the run
    // was already terminal and `finish_run_if_running` was a no-op (S2).
    let _ = surge_store::tokens::revoke_for_run(&state.pool, &run_id, now).await;
    let final_status = if moved {
        status
    } else {
        // Someone else terminalized this run first: an abort landing in the
        // ledger (§06-06), or the lease sweeper / a shutdown drain (N2).
        // Read what actually happened rather than assuming — a stale guess
        // here would write the issue a status the run never had.
        surge_store::observatory::get_run(&state.pool, &run_id)
            .await
            .map(|r| r.status)
            .unwrap_or(RunStatus::Aborted)
    };
    if let Some(reason) = &reason {
        log_span_failure(
            surge_store::observatory::append_span(&state.pool, &Span {
                id: format!("sp_end_{run_id}"),
                run_id: run_id.clone(),
                parent_span_id: None,
                node_id: None,
                role: SpanRole::Coordinator,
                started_at: now,
                duration_ms: Some(0),
                status: SpanStatus::Error,
                cost: 0.0,
                depth: 0,
                policy_decision: Some(reason.clone()),
                body: Some(reason.clone()),
            })
            .await,
            "run end",
            &run_id,
        );
    }
    // The worker's own dangling spans are resolved from the same observed
    // fact that ended the run (N4-residual): walk 3 found spans left
    // `running` forever on runs that had already succeeded.
    let _ = surge_store::observatory::resolve_dangling_spans(&state.pool, &run_id, DANGLING_SPAN)
        .await;
    if let Some(issue_id) = &issue_id {
        let issue_status = match final_status {
            RunStatus::Succeeded => OrchestrationStatus::Verified,
            RunStatus::Aborted => OrchestrationStatus::Aborted,
            _ => OrchestrationStatus::Failed,
        };
        // Only if this run still holds it: the sweeper may have reclaimed the
        // lease already, and its verdict must not be overwritten (N2).
        release_if_held(&state, issue_id, &run_id, issue_status).await;
    }
    if let Some(wt) = worktree {
        wt.reap();
    }
    let _ = surge_store::audit::record(
        &state.pool,
        "run.finished",
        &format!("{run_id}:{}", final_status.as_str()),
        "supervisor",
        Some(&project_id),
        now,
    )
    .await;
}

/// End a lease only when it is still the named run's. Every lease writer in
/// this module goes through here: monitor, sweeper and reconcile can all
/// arrive at the same issue, and the loser must not restate a status the
/// winner already decided (N2). Returns whether it released.
///
/// The guard is the UPDATE's own `WHERE lease_run_id = ?` (see
/// `store::issues::release_lease`), not a read followed by a write: that
/// read-then-write was a TOCTOU window in which a human retry plus a
/// redispatch could slip, and the stale release then nulled out the NEW run's
/// live lease (concurrency review 2026-08-26).
async fn release_if_held(
    state: &AppState,
    issue_id: &str,
    run_id: &str,
    status: OrchestrationStatus,
) -> bool {
    surge_store::issues::release_lease(&state.pool, issue_id, run_id, status)
        .await
        .unwrap_or(false)
}

/// Human-triggered doc run (design §23-Fourteen): same supervisor, no issue,
/// no lease, no worktree — the worker runs in the bound repo itself, since a
/// doc run's whole point is producing a declared doc there.
pub async fn dispatch_doc_run(state: &AppState, project_id: &str) -> anyhow::Result<String> {
    let now = now_ms();
    let project = surge_store::projects::get(&state.pool, project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown project"))?;
    let mat = surge_store::materializations::fresh_for_project(&state.pool, project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no fresh materialization; compile first"))?;

    let run_id = format!("run_{}", &surge_store::tokens::hash(&format!("{project_id}{now}"))[..12]);
    let run = Run {
        id: run_id.clone(),
        project_id: project.id.clone(),
        issue_id: None,
        kind: RunKind::Doc,
        materialization_hash: mat.content_hash.clone(),
        work_order_hash: None,
        status: RunStatus::Running,
        started_at: now,
        ended_at: None,
        cost: 0.0,
    };
    surge_store::observatory::insert_run(&state.pool, &run).await?;
    let runtime_token = surge_store::tokens::mint_for_run(
        &state.pool,
        surge_store::tokens::TokenKind::Runtime,
        Some(&project.id),
        Some(&run_id),
        now,
    )
    .await?;
    let cfg = &state.supervisor;
    // Scoped to the doc node, deliberately. "Run the doc pipeline compiled
    // into .claude/" pointed the worker at a directory that carries EVERY
    // node kind's agents — the compiled tree is the whole materialization,
    // not a per-run subset — so the worker also attempted the agent node,
    // found no work order behind it, and self-reported a refusal. The
    // Observatory then rendered a green `succeeded` run containing a red
    // `refused` row (smoke walk 5, F6). A doc run has no issue and no lease
    // (design §23-Fourteen); its whole job is the declared doc.
    let prompt = "Run ONLY the doc node of the pipeline compiled into .claude/ for this project: \
                  produce the docs it declares, in this repo, and stop. The compiled .claude/ \
                  also carries the agents for work-order nodes — those belong to leased issues, \
                  not to this run; do not attempt them. Use the surge MCP tools as you work: \
                  surge_append_span for progress, surge_poll_run to check for aborts.\n";
    // The run row exists and the credential is minted, so a spawn failure
    // owes the same cleanup a work-order dispatch owes — issue-less variant.
    // Propagating with `?` here leaked a permanently `running` run, a live
    // runtime token and no `dispatch.failed` entry (smoke walk 3, N1).
    let (child, stderr_tail) = match spawn_worker(
        cfg,
        std::path::Path::new(&project.repo_path),
        prompt,
        &[("SURGE_RUN_ID", run_id.as_str()), ("SURGE_RUNTIME_TOKEN", runtime_token.as_str())],
    ) {
        Ok(cs) => cs,
        Err(e) => {
            unwind_dispatch(
                state,
                Unwind::Failed,
                &run_id,
                &project.id,
                None,
                None,
                Some(&runtime_token),
                &format!("worker spawn failed: {e}"),
            )
            .await;
            return Err(e);
        }
    };
    surge_store::audit::record(&state.pool, "run.dispatched", &run_id, "human", Some(&project.id), now)
        .await?;
    tokio::spawn(monitor(
        state.clone(),
        run_id.clone(),
        project.id.clone(),
        None,
        child,
        stderr_tail,
        None,
    ));
    Ok(run_id)
}

// ---------------------------------------------------------------------------
// Lease enforcement that outlives a single `monitor` task (smoke walk 3, N2).
//
// Before this, TTL was enforced *only* inside the per-run monitor, which dies
// with the process. SIGKILL the server mid-run and the wreckage was permanent:
// run stuck `running`, issue stuck `leased`, worktree residue on disk, and
// every later `surge dispatch <issue>` refused "not eligible or already
// leased" — recoverable only by hand-editing SQLite. So: a fresh process owns
// nothing it finds running, a standing sweeper enforces the clock whether or
// not a monitor exists, and shutdown drains rather than abandons.
// ---------------------------------------------------------------------------

/// Terminalize one run this supervisor is not watching: run → Failed with a
/// visible reason span (INV-ERR-1), the lease released if this run still
/// holds it, its worktree reaped (INV-EXEC-2), one audit row carrying the
/// project (INV-OBS-1, N13). The status transition is guarded, so concurrent
/// callers — and repeat passes — cannot double-write: only the caller that
/// actually moved the run writes the span and returns true.
async fn terminalize_orphan(state: &AppState, run: &Run, reason: &str) -> bool {
    let now = now_ms();
    let moved =
        surge_store::observatory::finish_run_if_running(&state.pool, &run.id, RunStatus::Failed, now)
            .await
            .unwrap_or(false);
    if !moved {
        return false;
    }
    log_span_failure(
        surge_store::observatory::append_span(&state.pool, &Span {
            id: format!("sp_orphan_{}", run.id),
            run_id: run.id.clone(),
            parent_span_id: None,
            node_id: None,
            role: SpanRole::Coordinator,
            started_at: now,
            duration_ms: Some(0),
            status: SpanStatus::Error,
            cost: 0.0,
            depth: 0,
            policy_decision: Some(reason.to_string()),
            body: Some(reason.to_string()),
        })
        .await,
        "orphan terminalization",
        &run.id,
    );
    // A run terminalized here has the same dangling-span problem a monitored
    // one does — more so, since its worker is gone (N4-residual).
    let _ = surge_store::observatory::resolve_dangling_spans(&state.pool, &run.id, DANGLING_SPAN)
        .await;
    if let Some(issue_id) = &run.issue_id {
        release_if_held(state, issue_id, &run.id, OrchestrationStatus::Failed).await;
        // Reap only what this run can still own: if ANY run holds the lease
        // when we look, the worktree under that issue is that run's, live.
        // The check used to be skipped whenever we had just released the
        // lease ourselves — but that is precisely the moment a queued
        // dispatch can claim and re-worktree the issue, and the reap would
        // then delete a live worker's tree (concurrency review 2026-08-26).
        let taken_over = surge_store::issues::get(&state.pool, issue_id)
            .await
            .ok()
            .flatten()
            .and_then(|i| i.lease)
            .is_some();
        if !taken_over {
            reap_orphan_worktree(state, &run.project_id, issue_id).await;
        }
    }
    let _ = surge_store::audit::record(
        &state.pool,
        "run.reconciled",
        &format!("{}:{}", run.id, RunStatus::Failed.as_str()),
        "supervisor",
        Some(&run.project_id),
        now,
    )
    .await;
    true
}

/// Reap a worktree with no live [`WorktreeGuard`] behind it. Reconcile and
/// the sweeper have to identify the directory by convention — INV-EXEC-2 is
/// one worktree per lease, and dispatch always puts it at
/// `<work_dir>/<project>/<issue>`. Absent is not an error: this is cleanup,
/// not a check.
async fn reap_orphan_worktree(state: &AppState, project_id: &str, issue_id: &str) {
    let Ok(work_root) = absolutize(&state.supervisor.work_dir) else {
        return;
    };
    let dir = work_root.join(project_id).join(issue_id);
    if !dir.exists() {
        return;
    }
    let Ok(Some(project)) = surge_store::projects::get(&state.pool, project_id).await else {
        eprintln!("worktree {dir:?} left in place: project {project_id} is gone");
        return;
    };
    WorktreeGuard { repo: project.repo_path.into(), dir }.reap();
}

/// Terminalize every run still marked `running`, with one reason. Shared by
/// boot reconcile and the shutdown drain; idempotent by construction — the
/// transition is guarded and the query only returns runs still running.
async fn terminalize_all_running(state: &AppState, reason: &str) -> anyhow::Result<usize> {
    let runs = surge_store::observatory::running_runs(&state.pool).await?;
    let mut n = 0;
    for run in &runs {
        if terminalize_orphan(state, run, reason).await {
            n += 1;
        }
    }
    Ok(n)
}

/// Boot-time reconciliation, called before the server starts serving: a fresh
/// process owns none of the runs it finds `running`, because whatever was
/// watching them is gone. Each becomes a visible failure, its lease is
/// released and its worktree reaped — so the operator's next dispatch is
/// refused for a reason that is true, or not refused at all (N2).
///
/// The issue lands `failed`, not back in `eligible`: a supervisor that died
/// is no evidence the work is safe to redo, and re-queue policy (retry
/// counts, waves) is Phase 2's. What this owes is coherent, visible state —
/// not a guess about the work.
///
/// Takes [`AppState`] rather than the pool alone: identifying a worktree
/// needs the supervisor's `work_dir`, and the reap needs the project's repo.
pub async fn reconcile_orphans(state: &AppState) -> anyhow::Result<usize> {
    terminalize_all_running(
        state,
        "supervisor restarted while this run was in flight — the worker it was watching is \
         unreachable, so the run is failed and the lease released (INV-ERR-1)",
    )
    .await
}

/// Shutdown drain: give live monitors a grace period to terminalize their own
/// runs, then reconcile whatever is left. A plain Ctrl-C thereby lands in the
/// same clean state a crash-plus-restart does, instead of leaving the N2
/// wreckage for the next boot to find.
pub async fn drain_on_shutdown(state: &AppState, grace: std::time::Duration) -> usize {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        match surge_store::observatory::running_runs(&state.pool).await {
            Ok(runs) if runs.is_empty() => return 0,
            Err(_) => break,
            _ => {}
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    terminalize_all_running(
        state,
        "supervisor shut down while this run was in flight — the run is failed and the lease \
         released rather than left held by a process that no longer exists (INV-ERR-1)",
    )
    .await
    .unwrap_or(0)
}

/// One pass of the standing lease sweeper: reclaim every lease whose clock
/// ran out with no monitor to notice. The per-run monitor is the fast path
/// (it polls `poll_ms` and can kill its child); the sweeper deliberately
/// waits a full `sweep_ms` past expiry so a live monitor wins the race, and
/// exists for the leases no monitor is watching at all. Returns how many it
/// reclaimed.
pub async fn sweep_expired_leases(state: &AppState) -> usize {
    let cfg = &state.supervisor;
    let grace = cfg.sweep_ms as i64;
    let now = now_ms();
    let Ok(leases) = surge_store::issues::held_leases(&state.pool).await else {
        return 0;
    };
    let mut n = 0;
    for lease in leases {
        if now <= lease.expires_at + grace {
            continue;
        }
        let reason = format!(
            "lease reclaimed by the sweeper — no heartbeat within TTL ({}ms) and no live monitor \
             held this run (INV-ERR-1)",
            cfg.lease_ttl_ms
        );
        let run = surge_store::observatory::get_run(&state.pool, &lease.run_id).await.ok();
        let swept = match &run {
            Some(run) => terminalize_orphan(state, run, &reason).await,
            None => false,
        };
        if swept {
            n += 1;
            continue;
        }
        // The run was already terminal but the lease outlived it — the half
        // of N2 that left an issue permanently undispatchable.
        // Release and audit commit together (INV-DATA-8): a crash between them
        // silently returned an issue to the pool with no record of why it was
        // reclaimed — the exact scenario the store review named.
        let reclaimed = async {
            let mut tx = state.pool.begin().await?;
            let released = surge_store::issues::release_lease(
                &mut *tx, &lease.issue_id, &lease.run_id, OrchestrationStatus::Failed,
            )
            .await?;
            if released {
                surge_store::audit::record(
                    &mut *tx,
                    "lease.reclaimed",
                    &format!("{}:{}", lease.issue_id, lease.run_id),
                    "supervisor",
                    Some(&lease.project_id),
                    now,
                )
                .await?;
            }
            tx.commit().await?;
            Ok::<_, anyhow::Error>(released)
        };
        match reclaimed.await {
            Ok(true) => {
                reap_orphan_worktree(state, &lease.project_id, &lease.issue_id).await;
                n += 1;
            }
            Ok(false) => {}
            Err(e) => eprintln!("lease reclaim commit failed for {}: {e}", lease.issue_id),
        }
    }
    n
}

/// One pass of the credential sweeper: revoke every token whose expiry has
/// passed. Authorization does not depend on it — `tokens::lookup_active`
/// refuses an expired token the moment the clock passes it — but the store
/// does: without this, an aged-out project runtime token sits there with
/// `revoked_at IS NULL` forever and "zero live runtime tokens that are not
/// backing a running run" stops being checkable by looking (smoke walk 5,
/// F1). Returns how many it revoked.
pub async fn sweep_expired_tokens(state: &AppState) -> u64 {
    let now = now_ms();
    let n = surge_store::tokens::revoke_expired(&state.pool, now).await.unwrap_or(0);
    if n > 0 {
        // Rotation is a privileged act and so is its automatic half (INV-OBS-1).
        let _ = surge_store::audit::record(
            &state.pool,
            "token.expired_revoked",
            &format!("{n} expired credential(s) revoked"),
            "supervisor",
            None,
            now,
        )
        .await;
    }
    n
}

/// Start the sweeper. One task per process, independent of any dispatch, so
/// TTL enforcement exists even when no `monitor` does (N2). It owns no child
/// processes: an expired lease whose worker is somehow still alive is
/// reclaimed in the store, and the worker's own abort poll (§06-06) is what
/// stops it. It sweeps expired credentials on the same beat — both are
/// clocks nothing else is watching.
pub fn spawn_lease_sweeper(state: AppState) -> tokio::task::JoinHandle<()> {
    let period = std::time::Duration::from_millis(state.supervisor.sweep_ms.max(1));
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(period).await;
            sweep_expired_leases(&state).await;
            sweep_expired_tokens(&state).await;
        }
    })
}
