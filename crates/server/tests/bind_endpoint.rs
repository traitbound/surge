//! Project binding (phase 0 item 4, INV-DATA-1): bind writes the surge.yaml
//! base file into the bound repo and nothing else; refusals are visible
//! records with audit entries (INV-ERR-1).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

async fn setup(repo_path: &str) -> (AppState, String) {
    let pool = surge_store::open_in_memory().await.unwrap();
    let state = AppState::new(pool.clone());
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();
    let project = surge_domain::project::Project {
        id: "prj_bind".into(),
        name: "bindable".into(),
        repo_path: repo_path.into(),
        assigned_pipeline: None,
        pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: surge_domain::project::TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: 1,
    };
    surge_store::projects::insert(&pool, &project).await.unwrap();
    (state, session)
}

fn bind_req(session: &str, project_id: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/projects/{project_id}/bind"))
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn bind_writes_surge_yaml_and_nothing_else() {
    let repo = tempfile::tempdir().unwrap();
    let (state, session) = setup(&repo.path().to_string_lossy()).await;

    let r = app(state.clone()).oneshot(bind_req(&session, "prj_bind")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["surge_yaml_written"], true);

    // Exactly one file landed: surge.yaml, and it is the base header —
    // project identity plus the compiler-managed notice, no step blocks.
    let entries: Vec<String> = std::fs::read_dir(repo.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["surge.yaml".to_string()]);
    let yaml = std::fs::read_to_string(repo.path().join("surge.yaml")).unwrap();
    assert!(yaml.contains("project: prj_bind"), "{yaml}");
    assert!(yaml.contains("compiler-managed"), "{yaml}");
    assert!(!yaml.contains("\nsteps:"), "bind writes the base file only: {yaml}");

    // Flag persisted; audit recorded (INV-OBS-1).
    let p = surge_store::projects::get(&state.pool, "prj_bind").await.unwrap().unwrap();
    assert!(p.surge_yaml_written);
    let actions: Vec<String> = surge_store::audit::recent(&state.pool, 10).await.unwrap()
        .into_iter().map(|e| e.action).collect();
    assert!(actions.contains(&"project.bound".to_string()));
}

#[tokio::test]
async fn bind_header_matches_the_compiled_surge_yaml() {
    // The compiled surge.yaml extends the exact base bind wrote — the two
    // renderings can never disagree because they share surge_yaml_base.
    let repo = tempfile::tempdir().unwrap();
    let (state, session) = setup(&repo.path().to_string_lossy()).await;
    let r = app(state.clone()).oneshot(bind_req(&session, "prj_bind")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bound = std::fs::read_to_string(repo.path().join("surge.yaml")).unwrap();

    let p = surge_store::projects::get(&state.pool, "prj_bind").await.unwrap().unwrap();
    assert_eq!(surge_compiler::surge_yaml_base(&p), bound);
}

#[tokio::test]
async fn bind_refuses_missing_repo_dir_with_audit() {
    let (state, session) = setup("/nonexistent/surge-bind-test").await;

    let r = app(state.clone()).oneshot(bind_req(&session, "prj_bind")).await.unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
    let body: serde_json::Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body["error"].as_str().unwrap().contains("not a directory"));

    let p = surge_store::projects::get(&state.pool, "prj_bind").await.unwrap().unwrap();
    assert!(!p.surge_yaml_written, "refusal must not set the bind flag");
    let actions: Vec<String> = surge_store::audit::recent(&state.pool, 10).await.unwrap()
        .into_iter().map(|e| e.action).collect();
    assert!(actions.contains(&"project.bind_refused".to_string()));
}

#[tokio::test]
async fn bind_refuses_unknown_project_with_audit() {
    let repo = tempfile::tempdir().unwrap();
    let (state, session) = setup(&repo.path().to_string_lossy()).await;

    let r = app(state.clone()).oneshot(bind_req(&session, "prj_absent")).await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let entries: Vec<_> = std::fs::read_dir(repo.path()).unwrap().collect();
    assert!(entries.is_empty(), "a refused bind writes nothing");
    let actions: Vec<String> = surge_store::audit::recent(&state.pool, 10).await.unwrap()
        .into_iter().map(|e| e.action).collect();
    assert!(actions.contains(&"project.bind_refused".to_string()));
}
