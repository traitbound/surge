//! Surge server: one process, loopback only (INV-DEPLOY-1). The human-token /
//! runtime-token boundary is enforced here, in middleware — never in the UI
//! (INV-AUTH-1).

pub mod auth;
pub mod plugin_assets;
mod claim;
mod compile_api;
mod human_api;
mod runtime_api;
pub mod supervisor;
mod ui_assets;

use axum::{middleware, routing::get, Json, Router};
use sqlx::SqlitePool;

pub const BIND: &str = "127.0.0.1:7420";

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub supervisor: std::sync::Arc<supervisor::SupervisorConfig>,
}

impl AppState {
    /// State with a supervisor that cannot dispatch: there is no safe default
    /// for the plugin dir, so surfaces that never spawn say so explicitly
    /// (smoke walk 3, N14).
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, supervisor: std::sync::Arc::new(supervisor::SupervisorConfig::unconfigured()) }
    }

    pub fn with_supervisor(pool: SqlitePool, cfg: supervisor::SupervisorConfig) -> Self {
        Self { pool, supervisor: std::sync::Arc::new(cfg) }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_millis() as i64
}

async fn healthz(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<surge_domain::Health> {
    let schema_version = surge_store::schema_version(&state.pool).await.unwrap_or(0);
    Json(surge_domain::Health {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version,
    })
}

/// The full router. Three zones:
/// - public: `/healthz`, the one-time claim URL
/// - `/api/*`: human token only — a runtime token is refused loudly and
///   audited (INV-AUTH-2)
/// - `/runtime/*`: the five runtime capabilities; a human token also passes
///   (everything a machine may do is a subset of what a human may do, §04)
pub fn app(state: AppState) -> Router {
    let human = human_api::router().layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_human,
    ));
    let runtime = runtime_api::router().layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_runtime_or_human,
    ));
    Router::new()
        .route("/healthz", get(healthz))
        .route("/claim/{token}", get(claim::claim_session))
        .nest("/api", human)
        .nest("/runtime", runtime)
        .fallback(ui_assets::serve)
        .with_state(state)
}

/// The shipped default library (design §03: normative product content, not
/// fixture data) plus the two-node fixture pipeline — everything a fresh
/// instance needs to compile and dispatch. Runs at every boot, idempotently:
/// existing rows are never touched (library items are immutable per version,
/// INV-DATA-2); the `library.seeded` audit entry is written only on the boot
/// that first inserts anything (INV-OBS-1).
pub async fn bootstrap_seed(pool: &SqlitePool) -> anyhow::Result<()> {
    use surge_domain::library::{LibraryItem, LibraryItemKind, TrustState};

    let now = now_ms();
    let mut seeded = Vec::new();

    let items: [(LibraryItemKind, &str, &str); 3] = [
        (LibraryItemKind::Subagent, "doc-writer", include_str!("../seed/doc-writer.md")),
        (LibraryItemKind::Skill, "write-summary", include_str!("../seed/write-summary.SKILL.md")),
        (LibraryItemKind::Subagent, "implementer", include_str!("../seed/implementer.md")),
    ];
    for (kind, name, body) in items {
        if surge_store::library::get(pool, kind, name, 1).await?.is_some() {
            continue;
        }
        surge_store::library::insert(
            pool,
            &LibraryItem {
                id: format!("li_{}_v1", name.replace('-', "_")),
                kind,
                name: name.into(),
                version: 1,
                body: body.into(),
                trust: TrustState::Local,
                created_at: now,
            },
        )
        .await?;
        seeded.push(format!("{} v1 ({})", name, kind.as_str()));
    }

    // Phase 0 has no editor; pipelines are data. Seed the two-node pipeline
    // so a fresh instance has something to compile and dispatch.
    let (pipeline, nodes, edges) = surge_domain::fixtures::two_node_pipeline();
    if !surge_store::pipelines::exists(pool, &pipeline.id).await? {
        surge_store::pipelines::insert_graph(pool, &pipeline, &nodes, &edges).await?;
        seeded.push(format!("{} ({})", pipeline.name, pipeline.id));
    }

    if !seeded.is_empty() {
        surge_store::audit::record(pool, "library.seeded", &seeded.join(", "), "system", None, now)
            .await?;
    }
    Ok(())
}

/// First launch / restore / rotation-to-zero: no active session → mint a
/// one-time claim token and print the claim URL to the terminal. Reachability
/// is never authorization (INV-AUTH-5).
pub async fn bootstrap_auth(pool: &SqlitePool) -> anyhow::Result<()> {
    if surge_store::tokens::has_active_session(pool).await? {
        return Ok(());
    }
    // Invalidate any dangling claim from a previous unclaimed boot.
    surge_store::tokens::revoke_all(pool, surge_store::tokens::TokenKind::Claim, now_ms()).await?;
    let claim = surge_store::tokens::mint(
        pool,
        surge_store::tokens::TokenKind::Claim,
        None,
        now_ms(),
    )
    .await?;
    eprintln!("no active session — claim this instance (one-time URL):");
    eprintln!("    http://{BIND}/claim/{claim}");
    Ok(())
}
