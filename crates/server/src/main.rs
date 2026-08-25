//! Surge server: one process, loopback only (INV-DEPLOY-1). Token middleware,
//! the runtime API, compiler, dispatcher and supervisor land in later Phase 0
//! tasks; this scaffold serves `/healthz` over the real store.

use axum::{extract::State, routing::get, Json, Router};
use clap::Parser;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::path::PathBuf;

const BIND: &str = "127.0.0.1:7420";

#[derive(Parser)]
#[command(name = "surge-server", version)]
struct Args {
    /// Path to the SQLite database file (created if absent).
    #[arg(long, default_value = "surge.db")]
    db: PathBuf,
}

#[derive(Clone)]
struct App {
    pool: SqlitePool,
}

async fn healthz(State(app): State<App>) -> Json<surge_domain::Health> {
    let schema_version = surge_store::schema_version(&app.pool).await.unwrap_or(0);
    Json(surge_domain::Health {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let pool = surge_store::open(&args.db).await?;
    let app = Router::new()
        .route("/healthz", get(healthz))
        .with_state(App { pool });
    let addr: SocketAddr = BIND.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("surge-server listening on http://{BIND}");
    axum::serve(listener, app).await?;
    Ok(())
}
