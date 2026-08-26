use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use surge_server::{app, bootstrap_auth, bootstrap_seed, AppState, BIND};

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
    let supervisor = surge_server::supervisor::SupervisorConfig {
        plugin_dir,
        ..Default::default()
    };
    let router = app(AppState::with_supervisor(pool, supervisor));
    let addr: SocketAddr = BIND.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("surge-server listening on http://{BIND}");
    axum::serve(listener, router).await?;
    Ok(())
}
