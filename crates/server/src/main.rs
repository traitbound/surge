use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use surge_server::{app, bootstrap_auth, bootstrap_seed, AppState, BIND};

/// How long shutdown waits for live monitors before failing what is left.
const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Parser)]
#[command(name = "surge-server", version)]
struct Args {
    /// Path to the SQLite database file (created if absent).
    #[arg(long, default_value = "surge.db")]
    db: PathBuf,
    /// Plugin tree handed to workers as SURGE_PLUGIN_DIR. Defaults to the
    /// copy embedded in this binary, extracted beside the database (ADR-4);
    /// point it at `integrations/claude-plugin/` to run a working tree.
    #[arg(long)]
    plugin_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let pool = surge_store::open(&args.db).await?;
    bootstrap_auth(&pool).await?;
    bootstrap_seed(&pool).await?;

    // The plugin ships inside the binary and is extracted beside the DB, so
    // SURGE_PLUGIN_DIR resolves wherever the operator started the process
    // (ADR-4/ADR-8; smoke re-walk NEW-1). An explicit --plugin-dir is
    // verified rather than extracted.
    let plugin_dir = match &args.plugin_dir {
        Some(dir) => {
            let dir = std::path::absolute(dir)?;
            surge_server::plugin_assets::verify(&dir)?;
            dir
        }
        None => surge_server::plugin_assets::extract_beside_db(&args.db)?,
    };
    eprintln!("worker plugin: {}", plugin_dir.display());
    // `plugin_dir` is an argument, not a defaultable field: a plausible wrong
    // value there is the NEW-1 P0 (smoke walk 3, N14).
    let supervisor = surge_server::supervisor::SupervisorConfig::new(plugin_dir);
    let state = AppState::with_supervisor(pool, supervisor);

    // Nothing survives a process boundary as `running`: whatever the previous
    // process was watching, this one is not (smoke walk 3, N2). Runs before
    // the first request is served, so no surface ever shows the wreckage.
    let reconciled = surge_server::supervisor::reconcile_orphans(&state).await?;
    if reconciled > 0 {
        eprintln!("reconciled {reconciled} run(s) left in flight by a previous process");
    }
    // TTL enforcement that does not depend on a per-run monitor existing (N2).
    surge_server::supervisor::spawn_lease_sweeper(state.clone());

    let router = app(state.clone());
    let addr: SocketAddr = BIND.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("surge-server listening on http://{BIND}");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Ctrl-C must not leave what a SIGKILL leaves: give in-flight monitors a
    // grace period to terminalize their own runs — long enough for one that
    // is already reaping, short enough that Ctrl-C still feels like Ctrl-C —
    // then fail whatever is still in flight (N2).
    let drained = surge_server::supervisor::drain_on_shutdown(&state, DRAIN_GRACE).await;
    if drained > 0 {
        eprintln!("failed {drained} run(s) still in flight at shutdown");
    }
    Ok(())
}

/// SIGINT (Ctrl-C) or SIGTERM — the two ways an operator or a supervisor
/// stops this process. Either resolves the graceful-shutdown future.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    eprintln!("shutdown signal received — draining in-flight runs");
}
