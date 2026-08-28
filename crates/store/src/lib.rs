//! The single store (ADR-2): embedded SQLite via `sqlx`, one file, WAL.
//! Every query lives here behind a typed repository function and is
//! compile-checked; offline metadata is committed under `.sqlx/`
//! (`cargo sqlx prepare --workspace`). Repository functions that write will
//! also fire the commit broadcast once the event bus lands (ADR-3).

pub mod audit;
pub mod issues;
pub mod library;
pub mod materializations;
pub mod observatory;
pub mod pipelines;
pub mod projects;
pub mod tokens;
pub mod work_orders;

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
    use surge_compiler::pipeline_content_hash;
    use surge_domain::board::WorkOrder;
    use surge_domain::fixtures;
    use surge_domain::observatory::{Run, RunKind, RunStatus, Span, SpanRole, SpanStatus};

    #[tokio::test]
    async fn pipeline_graph_roundtrips_and_traverses() {
        let pool = crate::open_in_memory().await.unwrap();
        let (nodes, edges) = fixtures::two_node_graph();
        let pipeline = fixtures::two_node_pipeline(pipeline_content_hash(&nodes, &edges));
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
        let (nodes, mut edges) = fixtures::two_node_graph();
        edges[0].to_node = "nd_missing".into();
        // Hashed after the mutation: the row the engine is asked to reject is
        // internally consistent, so the FK is the only reason it can fail.
        let pipeline = fixtures::two_node_pipeline(pipeline_content_hash(&nodes, &edges));
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
    async fn bind_flag_and_pipeline_existence_roundtrip() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;

        assert!(crate::pipelines::exists(&pool, "pl_two_node_v1").await.unwrap());
        assert!(!crate::pipelines::exists(&pool, "pl_absent").await.unwrap());

        assert!(crate::projects::mark_surge_yaml_written(&pool, "prj_fixture").await.unwrap());
        let p = crate::projects::get(&pool, "prj_fixture").await.unwrap().unwrap();
        assert!(p.surge_yaml_written);
        assert!(!crate::projects::mark_surge_yaml_written(&pool, "prj_absent").await.unwrap());
    }

    #[tokio::test]
    async fn projects_list_reads_back_newest_first() {
        let pool = crate::open_in_memory().await.unwrap();
        for (id, t) in [("prj_a", 1_000), ("prj_b", 2_000)] {
            sqlx::query!(
                "INSERT INTO project (id, name, repo_path, created_at)
                 VALUES (?1, ?1, '/tmp/p', ?2)",
                id,
                t
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        let projects = crate::projects::list(&pool).await.unwrap();
        let ids: Vec<&str> = projects.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["prj_b", "prj_a"]);
        // The list read agrees with the single-row read.
        assert_eq!(projects[0], crate::projects::get(&pool, "prj_b").await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn runs_list_scopes_by_project_and_orders_newest_first() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        sqlx::query!(
            "INSERT INTO project (id, name, repo_path, created_at)
             VALUES ('prj_other', 'other', '/tmp/other', 1)"
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, project, t) in
            [("run_a", "prj_fixture", 1_000), ("run_b", "prj_fixture", 2_000), ("run_c", "prj_other", 3_000)]
        {
            crate::observatory::insert_run(&pool, &Run {
                id: id.into(),
                project_id: project.into(),
                issue_id: None,
                kind: RunKind::Doc,
                materialization_hash: "sha256:mat".into(),
                work_order_hash: None,
                status: RunStatus::Running,
                started_at: t,
                ended_at: None,
                cost: 0.0,
            })
            .await
            .unwrap();
        }
        let all = crate::observatory::list_runs(&pool, None).await.unwrap();
        let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["run_c", "run_b", "run_a"]);
        let scoped = crate::observatory::list_runs(&pool, Some("prj_fixture")).await.unwrap();
        let ids: Vec<&str> = scoped.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["run_b", "run_a"]);
        assert!(crate::observatory::list_runs(&pool, Some("prj_absent")).await.unwrap().is_empty());

    }

    /// Walk-3 finding N10: `work_orders::latest_for_issue` had no caller and
    /// no test. It is kept because it is the read half of the hash-mismatch
    /// check — INV-DATA-6 admits `work_orders/` reads for exactly that, and
    /// the row exists to be compared against the file on disk (design §05) —
    /// so what needs proving is the "latest" part: revisions accumulate and
    /// the highest one wins, whatever order they were written in.
    #[tokio::test]
    async fn work_order_read_returns_the_highest_revision() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        seed_issue(&pool, "iss_1").await;
        seed_issue(&pool, "iss_2").await;

        assert_eq!(crate::work_orders::latest_for_issue(&pool, "iss_1").await.unwrap(), None);

        // Revision 2 is written before revision 1: "latest" is the revision
        // number, not the insertion order.
        for (id, issue, rev, t) in [
            ("wo_1_r2", "iss_1", 2, 2_000i64),
            ("wo_1_r1", "iss_1", 1, 1_000),
            ("wo_2_r1", "iss_2", 1, 3_000),
        ] {
            crate::work_orders::insert(&pool, &WorkOrder {
                id: id.into(),
                issue_id: issue.into(),
                path: format!("work_orders/{issue}.md"),
                revision: rev,
                content_hash: format!("sha256:{id}"),
                created_at: t,
            })
            .await
            .unwrap();
        }

        let wo = crate::work_orders::latest_for_issue(&pool, "iss_1").await.unwrap().unwrap();
        assert_eq!(wo.id, "wo_1_r2");
        assert_eq!(wo.revision, 2);
        assert_eq!(wo.content_hash, "sha256:wo_1_r2");
        assert_eq!(wo.path, "work_orders/iss_1.md");
        // Scoped to its issue, and absent issues read as None rather than error.
        assert_eq!(
            crate::work_orders::latest_for_issue(&pool, "iss_2").await.unwrap().unwrap().id,
            "wo_2_r1"
        );
        assert_eq!(crate::work_orders::latest_for_issue(&pool, "iss_absent").await.unwrap(), None);
    }

    /// `claim_lease` is `role:critical` and its doc comment says "exactly one
    /// claimant wins", but no test ever issued two CONCURRENT claims: every
    /// existing one claims twice in sequence, which the `WHERE lease_owner IS
    /// NULL` predicate would satisfy even if it were not atomic (store review
    /// 2026-08-26, WARN). So: a real contention test — a file-backed pool
    /// (the in-memory one is a single connection, which would serialize the
    /// race away), N tasks released at once against one eligible issue.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_claims_have_exactly_one_winner() {
        let dir = std::env::temp_dir().join(format!("surge-claim-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("race.db");
        let pool = crate::open(&db).await.unwrap();
        seed_project_and_pipeline(&pool).await;
        seed_issue(&pool, "iss_race").await;

        const CLAIMANTS: usize = 8;
        // A shared start instant rather than a barrier: every task is parked
        // until the same moment, then they all hit one issue at once.
        let go = tokio::time::Instant::now() + std::time::Duration::from_millis(100);
        let mut tasks = Vec::new();
        for n in 0..CLAIMANTS {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                tokio::time::sleep_until(go).await;
                crate::issues::claim_lease(
                    &pool,
                    "iss_race",
                    &format!("worker-{n}"),
                    &format!("run_{n}"),
                    1_000,
                    600_000,
                )
                .await
                .unwrap()
            }));
        }
        let mut won = 0;
        for t in tasks {
            if t.await.unwrap() {
                won += 1;
            }
        }
        assert_eq!(won, 1, "exactly one claimant wins (INV-EXEC-1)");

        // And the store holds one coherent, all-or-nothing lease.
        let lease = crate::issues::get(&pool, "iss_race").await.unwrap().unwrap().lease.unwrap();
        assert!(lease.owner.starts_with("worker-"));
        assert_eq!(lease.run_id, lease.owner.replace("worker-", "run_"));
        assert_eq!(crate::issues::held_leases(&pool).await.unwrap().len(), 1);

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The lease release is guarded on the run that holds it, like every
    /// other lease write. Unguarded (`WHERE id = ?` plus a client-side
    /// read-then-write in the supervisor) a stale releaser could null out a
    /// NEWER run's live lease — a retry plus a redispatch landing in the
    /// window between the read and the write (concurrency review 2026-08-26).
    #[tokio::test]
    async fn releasing_a_lease_only_works_for_the_run_that_holds_it() {
        use surge_domain::board::OrchestrationStatus;
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        seed_issue(&pool, "iss_1").await;
        assert!(crate::issues::claim_lease(&pool, "iss_1", "w", "run_new", 1, 600_000)
            .await
            .unwrap());

        // The stale releaser (an older run, or a sweeper that lost the race).
        assert!(
            !crate::issues::release_lease(&pool, "iss_1", "run_old", OrchestrationStatus::Failed)
                .await
                .unwrap()
        );
        let issue = crate::issues::get(&pool, "iss_1").await.unwrap().unwrap();
        assert_eq!(issue.lease.unwrap().run_id, "run_new", "the live lease survived");
        assert_eq!(issue.status, OrchestrationStatus::Leased, "and its status was not restated");

        // The holder releases it.
        assert!(
            crate::issues::release_lease(&pool, "iss_1", "run_new", OrchestrationStatus::Verified)
                .await
                .unwrap()
        );
        let issue = crate::issues::get(&pool, "iss_1").await.unwrap().unwrap();
        assert!(issue.lease.is_none());
        assert_eq!(issue.status, OrchestrationStatus::Verified);
    }

    /// Same guard on the beat: a worker's heartbeat names its run, so it can
    /// only extend its own lease — the store half of the heartbeat hijack fix
    /// (auth review 2026-08-26). The interactive token passes `None` and
    /// extends whatever lease is there.
    #[tokio::test]
    async fn a_heartbeat_naming_another_run_moves_no_clock() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        seed_issue(&pool, "iss_1").await;
        assert!(crate::issues::claim_lease(&pool, "iss_1", "w", "run_mine", 1, 1_000)
            .await
            .unwrap());

        assert!(!crate::issues::heartbeat(&pool, "iss_1", Some("run_other"), 5_000, 1_000)
            .await
            .unwrap());
        assert_eq!(
            crate::issues::get(&pool, "iss_1").await.unwrap().unwrap().lease.unwrap().expires_at,
            1_001,
            "another run's beat never moved the clock"
        );
        assert!(crate::issues::heartbeat(&pool, "iss_1", Some("run_mine"), 5_000, 1_000)
            .await
            .unwrap());
        assert_eq!(
            crate::issues::get(&pool, "iss_1").await.unwrap().unwrap().lease.unwrap().expires_at,
            6_000
        );
        // The unbound (interactive) caller has no run to be checked against.
        assert!(crate::issues::heartbeat(&pool, "iss_1", None, 7_000, 1_000).await.unwrap());
    }

    /// INV-DATA-8, proven rather than asserted: a state change and its audit
    /// entry share a transaction, so if the audit write fails the state change
    /// is not visible either. Before this, every server handler committed the
    /// two separately and a crash in between produced a privileged act with no
    /// record — INV-OBS-1's guarantee failing through INV-DATA-8's crack.
    #[tokio::test]
    async fn state_change_and_audit_roll_back_together() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await;
        let mat = surge_domain::materialization::Materialization {
            id: "mk_tx".into(),
            content_hash: "sha256:tx".into(),
            cache_key: "mk_tx..fixture".into(),
            pipeline_id: "pl_two_node_v1".into(),
            project_id: "prj_fixture".into(),
            signed_by: "st_test".into(),
            fresh: true,
            created_at: 1,
        };

        // A transaction that writes the state change, then fails its audit
        // entry (an over-long action violates the schema CHECK), then rolls back.
        let mut tx = pool.begin().await.unwrap();
        crate::materializations::insert_fresh(&mut tx, &mat).await.unwrap();
        let bad_audit = crate::audit::record(
            &mut *tx, "", "subject", "actor", Some("prj_absent"), 1,
        )
        .await;
        assert!(bad_audit.is_err(), "the audit write must fail for this test to mean anything");
        drop(tx); // no commit — the whole unit unwinds

        assert!(
            crate::materializations::fresh_for_project(&pool, "prj_fixture").await.unwrap().is_none(),
            "the materialization must not survive an audit failure"
        );

        // And the happy path commits both.
        let mut tx = pool.begin().await.unwrap();
        crate::materializations::insert_fresh(&mut tx, &mat).await.unwrap();
        crate::audit::record(&mut *tx, "pipeline.compiled", "sha256:tx", "human", Some("prj_fixture"), 2)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(crate::materializations::fresh_for_project(&pool, "prj_fixture").await.unwrap().is_some());
        assert!(crate::audit::recent(&pool, 10).await.unwrap().iter().any(|a| a.action == "pipeline.compiled"));
    }

    /// ESC-3 / INV-ID-1: `Project::pipeline_status` is a *derived* view of the
    /// one signal dispatch actually checks — `materialization.fresh` — not a
    /// stored column. The stored column had no writer anywhere in the tree, so
    /// every project read back `Published` and the Registry pill described a
    /// state the system could not produce.
    #[tokio::test]
    async fn pipeline_status_is_derived_from_materialization_freshness() {
        let pool = crate::open_in_memory().await.unwrap();
        seed_project_and_pipeline(&pool).await; // a project, never compiled

        let uncompiled = crate::projects::get(&pool, "prj_fixture").await.unwrap().unwrap();
        assert_eq!(
            uncompiled.pipeline_status,
            surge_domain::project::PipelineAssignmentStatus::NotCompiled,
            "a project with no fresh materialization must not report a compiled \
             status — dispatch refuses it (INV-ID-1)"
        );
        let listed = crate::projects::list(&pool).await.unwrap();
        assert_eq!(
            listed[0].pipeline_status, uncompiled.pipeline_status,
            "list and get must not disagree about the same project"
        );

        // Compile: one fresh materialization for this project.
        crate::materializations::insert_fresh_committed(&pool, &surge_domain::materialization::Materialization {
            id: "mk_esc3".into(),
            content_hash: "sha256:esc3".into(),
            cache_key: "mk_esc3..fixture".into(),
            pipeline_id: "pl_two_node_v1".into(),
            project_id: "prj_fixture".into(),
            signed_by: "st_test".into(),
            fresh: true,
            created_at: 1,
        })
        .await
        .unwrap();

        let compiled = crate::projects::get(&pool, "prj_fixture").await.unwrap().unwrap();
        assert_eq!(
            compiled.pipeline_status,
            surge_domain::project::PipelineAssignmentStatus::Published,
            "a fresh materialization exists — the project passes the INV-ID-1 check"
        );
        assert_eq!(
            crate::projects::list(&pool).await.unwrap()[0].pipeline_status,
            surge_domain::project::PipelineAssignmentStatus::Published
        );
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
    use surge_compiler::pipeline_content_hash;
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
        let (n, e) = fixtures::two_node_graph();
        let p = fixtures::two_node_pipeline(pipeline_content_hash(&n, &e));
        crate::pipelines::insert_graph(pool, &p, &n, &e).await.unwrap();
    }

    /// An eligible issue in the fixture project — the FK target a work-order
    /// row needs.
    pub async fn seed_issue(pool: &SqlitePool, id: &str) {
        crate::issues::insert(pool, &surge_domain::board::Issue {
            id: id.into(),
            project_id: "prj_fixture".into(),
            title: format!("issue {id}"),
            wave: 1,
            phase: "phase-0".into(),
            status: surge_domain::board::OrchestrationStatus::Eligible,
            work_order_hash: format!("sha256:wo_{id}"),
            gate2: surge_domain::board::Gate2State::Pending,
            lease: None,
            retry_count: 0,
            disposition: None,
            priority: 0,
            is_wave_integration: false,
            created_at: 1,
        })
        .await
        .unwrap();
    }
}
