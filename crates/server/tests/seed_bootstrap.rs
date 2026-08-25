//! Default library seed (phase 0 item 10, design §03): the shipped library
//! items and the two-node fixture pipeline land at every boot, idempotently —
//! existing rows are never touched (INV-DATA-2), and `library.seeded` is
//! audited only on the boot that first inserts anything.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use surge_domain::library::{LibraryItemKind, TrustState};
use surge_server::{app, bootstrap_seed, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

#[tokio::test]
async fn seed_is_idempotent_across_boots() {
    let pool = surge_store::open_in_memory().await.unwrap();
    bootstrap_seed(&pool).await.unwrap();

    // The three items the two-node pipeline references, trusted Local, with
    // real product bodies that carry the span discipline (ADR-8 MCP tools).
    for (kind, name, marker) in [
        (LibraryItemKind::Subagent, "doc-writer", "name: doc-writer"),
        (LibraryItemKind::Skill, "write-summary", "# write-summary"),
        (LibraryItemKind::Subagent, "implementer", "name: implementer"),
    ] {
        let item = surge_store::library::get(&pool, kind, name, 1).await.unwrap().unwrap();
        assert_eq!(item.trust, TrustState::Local);
        assert!(item.body.contains(marker), "{name}: {}", item.body);
        assert!(item.body.contains("surge_append_span"), "{name} must teach span emission");
    }
    let created = surge_store::library::get(&pool, LibraryItemKind::Subagent, "doc-writer", 1)
        .await.unwrap().unwrap().created_at;

    // The fixture pipeline graph is data a fresh instance can compile.
    let (p, n, e) = surge_domain::fixtures::two_node_pipeline();
    let (p2, n2, e2) = surge_store::pipelines::load_graph(&pool, &p.id).await.unwrap();
    assert_eq!(p2, p);
    assert_eq!(n2.len(), n.len());
    assert_eq!(e2, e);

    // Second boot: nothing inserted, nothing updated, no second audit entry.
    bootstrap_seed(&pool).await.unwrap();
    let again = surge_store::library::get(&pool, LibraryItemKind::Subagent, "doc-writer", 1)
        .await.unwrap().unwrap();
    assert_eq!(again.created_at, created, "existing rows are left untouched (INV-DATA-2)");
    let seeded: Vec<_> = surge_store::audit::recent(&pool, 20).await.unwrap()
        .into_iter().filter(|e| e.action == "library.seeded").collect();
    assert_eq!(seeded.len(), 1, "library.seeded is audited only on first seed");
}

#[tokio::test]
async fn seeded_pipeline_compiles_cleanly() {
    let pool = surge_store::open_in_memory().await.unwrap();
    bootstrap_seed(&pool).await.unwrap();
    let state = AppState::new(pool.clone());
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();

    let repo = tempfile::tempdir().unwrap();
    surge_store::projects::insert(&pool, &surge_domain::project::Project {
        id: "prj_seeded".into(),
        name: "seeded".into(),
        repo_path: repo.path().to_string_lossy().into_owned(),
        assigned_pipeline: None,
        pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: surge_domain::project::TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: 1,
    })
    .await
    .unwrap();

    let r = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/projects/prj_seeded/compile")
                .header(header::AUTHORIZATION, format!("Bearer {session}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pipeline_id":"pl_two_node_v1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK, "a fresh seeded instance compiles out of the box");
    assert!(repo.path().join(".claude/agents/doc-writer.md").is_file());
    assert!(repo.path().join(".claude/skills/write-summary/SKILL.md").is_file());
}
