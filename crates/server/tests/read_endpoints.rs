//! The three phase-0 UI read endpoints: GET /api/projects, GET /api/runs
//! (optionally project-scoped), GET /api/runs/{id}/spans. Human token only —
//! the auth boundary applies to reads exactly as to writes (INV-AUTH-1/2).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use surge_domain::observatory::{Run, RunKind, RunStatus, Span, SpanRole, SpanStatus};
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

async fn setup() -> (AppState, String) {
    let pool = surge_store::open_in_memory().await.unwrap();
    let state = AppState::new(pool.clone());
    let session = surge_store::tokens::mint(&pool, TokenKind::Session, None, 1).await.unwrap();

    for (id, name, t) in [("prj_a", "alpha", 1_000i64), ("prj_b", "beta", 2_000)] {
        surge_store::projects::insert(&pool, &surge_domain::project::Project {
            id: id.into(),
            name: name.into(),
            repo_path: format!("/tmp/{name}"),
            assigned_pipeline: None,
            pipeline_status: surge_domain::project::PipelineAssignmentStatus::Published,
            surge_yaml_written: false,
            tracker: surge_domain::project::TrackerKind::None,
            branch_format: "task/{issue}".into(),
            created_at: t,
        })
        .await
        .unwrap();
    }
    for (id, project, t) in [("run_1", "prj_a", 1_000i64), ("run_2", "prj_b", 2_000)] {
        surge_store::observatory::insert_run(&pool, &Run {
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
    for (id, parent, t, depth) in
        [("sp_root", None, 1_000i64, 0i64), ("sp_child", Some("sp_root"), 2_000, 1)]
    {
        surge_store::observatory::append_span(&pool, &Span {
            id: id.into(),
            run_id: "run_1".into(),
            parent_span_id: parent.map(Into::into),
            node_id: None,
            role: SpanRole::Worker,
            started_at: t,
            duration_ms: Some(5),
            status: SpanStatus::Ok,
            cost: 0.01,
            depth,
            policy_decision: None,
            body: None,
        })
        .await
        .unwrap();
    }
    (state, session)
}

fn get_req(uri: &str, session: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(s) = session {
        b = b.header(header::AUTHORIZATION, format!("Bearer {s}"));
    }
    b.body(Body::empty()).unwrap()
}

async fn json_body(r: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn projects_list_returns_every_project_newest_first() {
    let (state, session) = setup().await;
    let r = app(state).oneshot(get_req("/api/projects", Some(&session))).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = json_body(r).await;
    let ids: Vec<&str> = body.as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["prj_b", "prj_a"]);
    assert_eq!(body[0]["name"], "beta");
}

#[tokio::test]
async fn runs_list_scopes_by_project_id_query() {
    let (state, session) = setup().await;
    let router = app(state);
    let r = router.clone().oneshot(get_req("/api/runs", Some(&session))).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json_body(r).await.as_array().unwrap().len(), 2);

    let r = router
        .oneshot(get_req("/api/runs?project_id=prj_a", Some(&session)))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = json_body(r).await;
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["id"], "run_1");
    assert_eq!(body[0]["status"], "running");
}

#[tokio::test]
async fn run_spans_returns_the_depth_first_tree() {
    let (state, session) = setup().await;
    let router = app(state);
    let r = router
        .clone()
        .oneshot(get_req("/api/runs/run_1/spans", Some(&session)))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = json_body(r).await;
    let ids: Vec<&str> = body.as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["sp_root", "sp_child"]);
    assert_eq!(body[1]["depth"], 1);

    // A run with no spans reads back an empty tree, not an error.
    let r = router.oneshot(get_req("/api/runs/run_2/spans", Some(&session))).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(json_body(r).await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn reads_require_a_human_token() {
    let (state, _session) = setup().await;
    let router = app(state.clone());
    for uri in ["/api/projects", "/api/runs", "/api/runs/run_1/spans"] {
        let r = router.clone().oneshot(get_req(uri, None)).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{uri} must refuse anonymous reads");
    }
    // A runtime token is refused loudly at privileged reads too (INV-AUTH-2).
    let rt = surge_store::tokens::mint(&state.pool, TokenKind::Runtime, Some("prj_a"), 1)
        .await
        .unwrap();
    let r = app(state).oneshot(get_req("/api/projects", Some(&rt))).await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}
