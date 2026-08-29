//! Compile end to end: trust gate → 409 with names + audit; success → files
//! on disk, gitignore block, fresh materialization row, audit entry;
//! recompile stales the predecessor.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

async fn setup() -> (AppState, String, tempfile::TempDir) {
    let pool = surge_store::open_in_memory().await.unwrap();
    let state = AppState::new(pool.clone());
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();
    let repo = tempfile::tempdir().unwrap();

    let project = surge_domain::project::Project {
        id: "prj_fix".into(),
        name: "fixture".into(),
        repo_path: repo.path().to_string_lossy().into_owned(),
        assigned_pipeline: None,
        pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: surge_domain::project::TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: 1,
    };
    surge_store::projects::insert(&pool, &project).await.unwrap();
    let (n, e) = surge_domain::fixtures::two_node_graph();
    let p = surge_domain::fixtures::two_node_pipeline();
    surge_store::pipelines::insert_graph(&pool, &p, &n, &e).await.unwrap();
    for (kind, name, body) in [
        (surge_domain::library::LibraryItemKind::Subagent, "doc-writer", "Write the doc."),
        (surge_domain::library::LibraryItemKind::Skill, "write-summary", "# Summarize."),
        (surge_domain::library::LibraryItemKind::Subagent, "implementer", "Implement."),
    ] {
        surge_store::library::insert(&pool, &surge_domain::library::LibraryItem {
            id: format!("li_{name}"),
            kind,
            name: name.into(),
            version: 1,
            body: body.into(),
            trust: surge_domain::library::TrustState::Local,
            created_at: 1,
        })
        .await
        .unwrap();
    }
    (state, session, repo)
}

fn compile_req(session: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/projects/prj_fix/compile")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"pipeline_id":"pl_two_node_v1"}"#))
        .unwrap()
}

#[tokio::test]
async fn compile_writes_files_and_records_materialization() {
    let (state, session, repo) = setup().await;
    let router = app(state.clone());

    let r = router.clone().oneshot(compile_req(&session)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let hash1 = body["materialization"]["content_hash"].as_str().unwrap().to_string();
    assert!(hash1.starts_with("sha256:"));
    assert_eq!(body["capability_report"]["writes"][0], "docs/summary.md");

    // Files landed in the bound repo; gitignore block maintained (INV-DATA-7).
    assert!(repo.path().join(".claude/agents/implementer.md").is_file());
    assert!(repo.path().join("surge.yaml").is_file());
    let gi = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert_eq!(gi.matches("surge-managed").count(), 2); // start + end markers

    // Fresh materialization recorded; recompile replaces it atomically.
    let m1 = surge_store::materializations::fresh_for_project(&state.pool, "prj_fix")
        .await.unwrap().unwrap();
    assert_eq!(m1.content_hash, hash1);
    let r = router.clone().oneshot(compile_req(&session)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let m2 = surge_store::materializations::fresh_for_project(&state.pool, "prj_fix")
        .await.unwrap().unwrap();
    assert_eq!(m2.content_hash, hash1, "same graph, same identity (INV-ID-1)");
    assert_eq!(m1.id, m2.id, "recompile of identical content is a cache hit, not a new identity");

    let actions: Vec<String> = surge_store::audit::recent(&state.pool, 10).await.unwrap()
        .into_iter().map(|e| e.action).collect();
    assert_eq!(actions.iter().filter(|a| *a == "pipeline.compiled").count(), 2);
}

/// ESC-3 / INV-ID-1: the badge the Registry renders is the same fact dispatch
/// gates on, over the wire. Before this, `pipeline_status` came from a column
/// nothing ever wrote, so this read "published" on a project that had never
/// compiled and whose every dispatch would be refused. Compiling is the event
/// that makes dispatch legal, so it is the event that must flip the badge.
#[tokio::test]
async fn compiling_flips_the_reported_pipeline_status() {
    let (state, session, _repo) = setup().await;
    let router = app(state.clone());

    let status = |router: axum::Router, session: String| async move {
        let r = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/projects")
                    .header(header::AUTHORIZATION, format!("Bearer {session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        body[0]["pipeline_status"].as_str().unwrap().to_string()
    };

    assert_eq!(
        status(router.clone(), session.clone()).await,
        "not_compiled",
        "never compiled → dispatch would be refused (INV-ID-1), and the card must say so"
    );
    assert!(surge_store::materializations::fresh_for_project(&state.pool, "prj_fix")
        .await.unwrap().is_none(), "precondition: nothing fresh yet");

    let r = router.clone().oneshot(compile_req(&session)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    assert_eq!(
        status(router, session).await,
        "published",
        "a fresh materialization exists → the card must stop warning"
    );
}

#[tokio::test]
async fn untrusted_import_hard_blocks_compile() {
    let (state, session, repo) = setup().await;
    // Re-mark the skill untrusted (imports land untrusted, INV-AUTH-3).
    sqlx::query("UPDATE library_item SET trust = 'imported_untrusted' WHERE name = 'write-summary'")
        .execute(&state.pool)
        .await
        .unwrap();
    let router = app(state.clone());

    let r = router.clone().oneshot(compile_req(&session)).await.unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
    let body: serde_json::Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let msg = body["error"].as_str().unwrap();
    assert!(msg.contains("write-summary (skill)"), "refusal names the item: {msg}");

    // Nothing materialized, nothing written (hard block).
    assert!(surge_store::materializations::fresh_for_project(&state.pool, "prj_fix")
        .await.unwrap().is_none());
    assert!(!repo.path().join(".claude").exists());

    let actions: Vec<String> = surge_store::audit::recent(&state.pool, 10).await.unwrap()
        .into_iter().map(|e| e.action).collect();
    assert!(actions.contains(&"compile.refused".to_string()));
}
