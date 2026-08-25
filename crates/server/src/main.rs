use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use surge_server::{app, bootstrap_auth, AppState, BIND};

#[derive(Parser)]
#[command(name = "surge-server", version)]
struct Args {
    /// Path to the SQLite database file (created if absent).
    #[arg(long, default_value = "surge.db")]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let pool = surge_store::open(&args.db).await?;
    bootstrap_auth(&pool).await?;
    let router = app(AppState { pool });
    let addr: SocketAddr = BIND.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("surge-server listening on http://{BIND}");
    axum::serve(listener, router).await?;
    Ok(())
}
