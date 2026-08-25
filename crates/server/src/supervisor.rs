//! The runtime supervisor — Surge owns the actuator (ADR-5, INV-EXEC-1/2/3).
//! Dispatch: fresh-materialization check → work-order hash check → lease →
//! git worktree on the task branch (outside the bound repo: worktrees live in
//! Surge's work dir, since INV-DATA-1 closes the repo write list) → compile
//! into it → spawn one headless worker with the runtime token injected as env
//! (INV-AUTH-4) → watch. Terminal state derives from exit codes and the lease
//! clock only — never span content (INV-EXEC-3).

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
    pub plugin_dir: PathBuf,
    /// How often the watchdog looks at the lease clock and the child.
    pub poll_ms: u64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
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
            plugin_dir: PathBuf::from("integrations/claude-plugin"),
            poll_ms: 500,
        }
    }
}

pub enum DispatchOutcome {
    /// The run id of the spawned worker's run.
    Spawned { run_id: String },
    /// Refused — and the refusal itself produced a run with one span carrying
    /// the reason (INV-ERR-1, design §06).
    Refused { run_id: String, reason: String },
}

/// Path config may be given relative; consumers resolve it against the
/// server's cwd, ONCE, here — `git -C <repo>` would otherwise resolve the
/// same string against the bound repo and write a worktree inside it
/// (smoke 2026-08-25, F1/F3; INV-DATA-1, INV-EXEC-2).
fn absolutize(p: &std::path::Path) -> anyhow::Result<PathBuf> {
    Ok(std::path::absolute(p)?)
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
    surge_store::observatory::append_span(&state.pool, &Span {
        id: format!("sp_{run_id}"),
        run_id: run_id.clone(),
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
    .await?;
    surge_store::audit::record(&state.pool, "dispatch.refused", reason, "human", Some(project_id), now)
        .await?;
    Ok(run_id)
}

/// Dispatch one issue (phase 0: single-task, no queue). See module docs for
/// the sequence; every spawn records run id, materialization hash and
/// work-order hash (INV-EXEC-1) via the run row itself.
pub async fn dispatch_issue(state: &AppState, issue_id: &str) -> anyhow::Result<DispatchOutcome> {
    let now = now_ms();
    let issue = surge_store::issues::get(&state.pool, issue_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown issue"))?;
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
            fail_dispatch(state, &run_id, &issue.id, None, &format!("worktree creation failed: {e}")).await;
            return Err(e);
        }
    };

    // 5+6. Compile into the worktree, render the work order, spawn — any
    // failure past the lease claim must not leak the lease, the running run,
    // or the worktree (smoke 2026-08-25, F4).
    let spawned: anyhow::Result<(tokio::process::Child, StderrTail)> = async {
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

        let runtime_token = surge_store::tokens::mint(
            &state.pool,
            surge_store::tokens::TokenKind::Runtime,
            Some(&project.id),
            now,
        )
        .await?;
        // The work order arrives on stdin (see worker_cmd docs) with a
        // pointer at the surge MCP tools.
        let prompt = format!(
            "{rendered}\nUse the surge MCP tools as you work: surge_append_span for progress, \
             surge_heartbeat regularly, surge_poll_run to check for aborts.\n"
        );
        spawn_worker(cfg, &worktree, &prompt, &[
            ("SURGE_RUN_ID", run_id.as_str()),
            ("SURGE_ISSUE_ID", issue.id.as_str()),
            ("SURGE_RUNTIME_TOKEN", runtime_token.as_str()),
        ])
    }
    .await;
    let (child, stderr_tail) = match spawned {
        Ok(cs) => cs,
        Err(e) => {
            fail_dispatch(state, &run_id, &issue.id, Some(guard), &format!("worker spawn failed: {e}")).await;
            return Err(e);
        }
    };

    surge_store::audit::record(&state.pool, "run.dispatched", &run_id, "human", Some(&project.id), now)
        .await?;

    tokio::spawn(monitor(
        state.clone(),
        run_id.clone(),
        Some(issue.id.clone()),
        child,
        stderr_tail,
        Some(guard),
    ));
    Ok(DispatchOutcome::Spawned { run_id })
}

type StderrTail = tokio::sync::oneshot::Receiver<String>;

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

/// The cleanup owed after a post-lease dispatch failure: run → failed with a
/// reason span, lease released, worktree reaped. Nothing leaks.
async fn fail_dispatch(
    state: &AppState,
    run_id: &str,
    issue_id: &str,
    worktree: Option<WorktreeGuard>,
    reason: &str,
) {
    let now = now_ms();
    let _ = surge_store::observatory::finish_run_if_running(&state.pool, run_id, RunStatus::Failed, now)
        .await;
    let _ = surge_store::observatory::append_span(&state.pool, &Span {
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
    })
    .await;
    let _ = surge_store::issues::release_lease(&state.pool, issue_id, OrchestrationStatus::Failed).await;
    if let Some(wt) = worktree {
        wt.reap();
    }
    let _ = surge_store::audit::record(&state.pool, "dispatch.failed", reason, "supervisor", None, now)
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
    // If an abort already landed, it stands (finish_run_if_running is guarded).
    let moved = surge_store::observatory::finish_run_if_running(&state.pool, &run_id, status, now)
        .await
        .unwrap_or(false);
    let final_status = if moved {
        status
    } else {
        RunStatus::Aborted // the only guarded path a running run leaves early
    };
    if let Some(reason) = &reason {
        let _ = surge_store::observatory::append_span(&state.pool, &Span {
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
        .await;
    }
    if let Some(issue_id) = &issue_id {
        let issue_status = match final_status {
            RunStatus::Succeeded => OrchestrationStatus::Verified,
            RunStatus::Aborted => OrchestrationStatus::Aborted,
            _ => OrchestrationStatus::Failed,
        };
        let _ = surge_store::issues::release_lease(&state.pool, issue_id, issue_status).await;
    }
    if let Some(wt) = worktree {
        wt.reap();
    }
    let _ = surge_store::audit::record(
        &state.pool,
        "run.finished",
        &format!("{run_id}:{}", final_status.as_str()),
        "supervisor",
        None,
        now,
    )
    .await;
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
    let runtime_token = surge_store::tokens::mint(
        &state.pool,
        surge_store::tokens::TokenKind::Runtime,
        Some(&project.id),
        now,
    )
    .await?;
    let cfg = &state.supervisor;
    let prompt = "Run the doc pipeline compiled into .claude/ for this project. Use the surge MCP \
                  tools as you work: surge_append_span for progress, surge_poll_run to check for aborts.\n";
    let (child, stderr_tail) = spawn_worker(
        cfg,
        std::path::Path::new(&project.repo_path),
        prompt,
        &[("SURGE_RUN_ID", run_id.as_str()), ("SURGE_RUNTIME_TOKEN", runtime_token.as_str())],
    )?;
    surge_store::audit::record(&state.pool, "run.dispatched", &run_id, "human", Some(&project.id), now)
        .await?;
    tokio::spawn(monitor(state.clone(), run_id.clone(), None, child, stderr_tail, None));
    Ok(run_id)
}
