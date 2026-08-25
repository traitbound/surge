//! The single store (ADR-2): embedded SQLite via `sqlx`, one file, WAL.
//! Every query lives here behind a typed repository function and is
//! compile-checked; offline metadata is committed under `.sqlx/`
//! (`cargo sqlx prepare --workspace`). Repository functions that write will
//! also fire the commit broadcast once the event bus lands (ADR-3).

pub mod audit;
pub mod library;
pub mod materializations;
pub mod observatory;
pub mod pipelines;
pub mod projects;
pub mod tokens;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;

/// Embedded migrations, applied at process start (ADR-9). sqlx's migrations
/// table is the schema-version stamp.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Open (creating if absent) the database file and apply pending migrations.
/// WAL for concurrent readers, foreign keys enforced, busy_timeout for the
/// single-writer discipline (ADR-2 consequences).
pub async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().connect_with(opts).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// In-memory database for integration tests — same migrations, same queries.
pub async fn open_in_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(true);
    // A single connection: each in-memory connection is its own database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Latest applied migration version — surfaced on `/healthz`.
pub async fn schema_version(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row = sqlx::query!("SELECT MAX(version) AS version FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(row.version.unwrap_or(0))
}

pub mod instance_meta {
    use sqlx::SqlitePool;

    pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query!(
            "INSERT INTO instance_meta (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query!("SELECT value FROM instance_meta WHERE key = ?", key)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|r| r.value))
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn migrations_apply_and_meta_roundtrips() {
        let pool = super::open_in_memory().await.unwrap();
        assert!(super::schema_version(&pool).await.unwrap() >= 1);
        super::instance_meta::set(&pool, "instance_id", "test").await.unwrap();
        assert_eq!(
            super::instance_meta::get(&pool, "instance_id").await.unwrap().as_deref(),
            Some("test")
        );
        assert_eq!(super::instance_meta::get(&pool, "absent").await.unwrap(), None);
    }
}

#[cfg(test)]
mod object_model_tests {
    use super::tests_support::*;
    use surge_domain::fixtures;
    use surge_domain::observatory::{Run, RunKind, RunStatus, Span, SpanRole, SpanStatus};

    #[tokio::test]
    async fn pipeline_graph_roundtrips_and_traverses() {
        let pool = crate::open_in_memory().await.unwrap();
        let (pipeline, nodes, edges) = fixtures::two_node_pipeline();
        crate::pipelines::insert_graph(&pool, &pipeline, &nodes, &edges).await.unwrap();

        let (p2, mut n2, e2) = crate::pipelines::load_graph(&pool, &pipeline.id).await.unwrap();
        assert_eq!(p2, pipeline);
        // load_graph orders by id; the fixture lists nodes in flow order.
        n2.sort_by(|a, b| {
            let pos = |n: &surge_domain::pipeline::Node| nodes.iter().position(|x| x.id == n.id);
            pos(a).cmp(&pos(b))
        });
        assert_eq!(n2, nodes);
        assert_eq!(e2, edges);

        // DAG traversal (recursive CTE): everything downstream of the doc node.
        let reach = crate::pipelines::reachable_nodes(&pool, &pipeline.id, "nd_write_summary")
            .await
            .unwrap();
        assert_eq!(reach, vec!["nd_implement".to_string(), "nd_write_summary".to_string()]);
    }

    #[tokio::test]
    async fn dangling_edge_is_refused_by_the_engine() {
        let pool = crate::open_in_memory().await.unwrap();
        let (pipeline, nodes, mut edges) = fixtures::two_node_pipeline();
        edges[0].to_node = "nd_missing".into();
        let err = crate::pipelines::insert_graph(&pool, &pipeline, &nodes, &edges).await;
        assert!(err.is_err(), "FK on (pipeline_id, to_node) must reject a dangling edge");
        // And the transaction rolled back whole (INV-DATA-8): no orphan pipeline row.
        assert!(crate::pipelines::load_graph(&pool, &pipeline.id).await.is_err());
    }

    #[tokio::test]
    async fn span_tree_reads_depth_first() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        let run = Run {
            id: "run_1".into(),
            project_id: "prj_fixture".into(),
            issue_id: None,
            kind: RunKind::Doc,
            materialization_hash: "sha256:mat".into(),
            work_order_hash: None,
            status: RunStatus::Running,
            started_at: 1_000,
            ended_at: None,
            cost: 0.0,
        };
        crate::observatory::insert_run(&pool, &run).await.unwrap();
        // root(coordinator) -> a(worker, t=2000) -> a1(verifier, t=4000)
        //                   -> b(worker, t=3000)
        for (id, parent, role, t, depth) in [
            ("sp_root", None, SpanRole::Coordinator, 1_000, 0),
            ("sp_a", Some("sp_root"), SpanRole::Worker, 2_000, 1),
            ("sp_b", Some("sp_root"), SpanRole::Worker, 3_000, 1),
            ("sp_a1", Some("sp_a"), SpanRole::Verifier, 4_000, 2),
        ] {
            crate::observatory::append_span(&pool, &Span {
                id: id.into(),
                run_id: "run_1".into(),
                parent_span_id: parent.map(Into::into),
                node_id: None,
                role,
                started_at: t,
                duration_ms: Some(10),
                status: SpanStatus::Ok,
                cost: 0.01,
                depth,
                policy_decision: None,
                body: Some("output".into()),
            })
            .await
            .unwrap();
        }
        let tree = crate::observatory::span_tree(&pool, "run_1").await.unwrap();
        let order: Vec<&str> = tree.iter().map(|s| s.id.as_str()).collect();
        // Depth-first: a's subtree completes before sibling b.
        assert_eq!(order, vec!["sp_root", "sp_a", "sp_a1", "sp_b"]);
        assert_eq!(tree[0].role, SpanRole::Coordinator);

        let run2 = crate::observatory::get_run(&pool, "run_1").await.unwrap();
        assert_eq!(run2, run);
    }

    #[tokio::test]
    async fn work_order_run_without_issue_is_refused() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        let bad = Run {
            id: "run_bad".into(),
            project_id: "prj_fixture".into(),
            issue_id: None, // work_order kind requires an issue (design §23-Fourteen)
            kind: RunKind::WorkOrder,
            materialization_hash: "sha256:mat".into(),
            work_order_hash: Some("sha256:wo".into()),
            status: RunStatus::Running,
            started_at: 1_000,
            ended_at: None,
            cost: 0.0,
        };
        assert!(crate::observatory::insert_run(&pool, &bad).await.is_err());
    }

    #[tokio::test]
    async fn audit_appends_and_reads_back() {
        let pool = crate::open_in_memory().await.unwrap();
        let id = crate::audit::record(&pool, "compile", "pl_two_node_v1", "st_test", None, 5_000)
            .await
            .unwrap();
        assert!(id >= 1);
        let entries = crate::audit::recent(&pool, 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "compile");
        assert_eq!(entries[0].project_id, None);
    }
}

#[cfg(test)]
mod tests_support {
    use sqlx::SqlitePool;
    use surge_domain::fixtures;

    /// Seed the FK targets run/span rows need: one project, the fixture pipeline.
    pub async fn seed_project_and_pipeline(pool: &SqlitePool) {
        sqlx::query!(
            "INSERT INTO project (id, name, repo_path, created_at)
             VALUES ('prj_fixture', 'fixture', '/tmp/fixture', 1)"
        )
        .execute(pool)
        .await
        .unwrap();
        let (p, n, e) = fixtures::two_node_pipeline();
        crate::pipelines::insert_graph(pool, &p, &n, &e).await.unwrap();
    }
}
