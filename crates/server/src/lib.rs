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

use axum::{http::StatusCode, middleware, response::IntoResponse, routing::get, Json, Router};
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

/// Actor string for an audit row, derived from an identity: `human`,
/// `rt:<project>` (project-scoped runtime token), or `rt:<project>:<run>`
/// (run-bound runtime token, INV-AUTH-4). Shared between `auth`'s boundary
/// refusals and `runtime_api`'s scope refusals so every audit row derives
/// its actor from the same place — a copy here previously let one refusal
/// path format the actor without its run_id while the other kept it (walk-6
/// R3; the sharing itself guards the walk-3 N1/N6/N13 defect of copying this
/// kind of derivation).
pub(crate) fn actor_of(identity: &surge_store::tokens::Identity) -> String {
    use surge_store::tokens::Identity;
    match identity {
        Identity::Human => "human".to_string(),
        Identity::Runtime { project_id, run_id: None } => format!("rt:{project_id}"),
        Identity::Runtime { project_id, run_id: Some(run) } => format!("rt:{project_id}:{run}"),
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

/// Unknown path under `/api` or `/runtime`. Without it the SPA fallback
/// catches every nest miss and answers `200 text/html` — an API client is
/// told "fine" and handed the UI shell, and the 401-for-a-real-route vs
/// 200-HTML-for-a-typo split let an anonymous caller read off the route table
/// (walk-3 finding N11). Registered *after* the auth layer, so `layer`'s
/// "existing routes only" rule leaves it unauthenticated and the answer is
/// the same anonymously and authenticated.
async fn api_not_found() -> axum::response::Response {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "unknown endpoint" })))
        .into_response()
}

/// The full router. Three zones:
/// - public: `/healthz`, the one-time claim URL
/// - `/api/*`: human token only — a runtime token is refused loudly and
///   audited (INV-AUTH-2)
/// - `/runtime/*`: the five runtime capabilities; a human token also passes
///   (everything a machine may do is a subset of what a human may do, §04)
///
/// Each nested zone owns its own JSON 404, so the SPA fallback below serves
/// genuine UI paths only.
pub fn app(state: AppState) -> Router {
    let human = human_api::router()
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_human))
        .fallback(api_not_found);
    let runtime = runtime_api::router()
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_runtime_or_human))
        .fallback(api_not_found);
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
    //
    // The row's identity is not this seed's business any more: the fixture
    // derives its own `content_hash` from its own graph, and `insert_graph`
    // derives the value it stores from the graph it is handed (ESC-4). Every
    // future writer of a pipeline row inherits that for free — where ESC-1
    // left the obligation with each caller, it is now structural. INV-DATA-3
    // keeps a published version immutable, so the insert is the only moment
    // the hash can be set.
    let (nodes, edges) = surge_domain::fixtures::two_node_graph();
    let pipeline = surge_domain::fixtures::two_node_pipeline();
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
