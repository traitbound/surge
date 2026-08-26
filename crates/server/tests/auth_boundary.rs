//! The token boundary end to end (INV-AUTH-1/2/5, INV-ERR-1): claim flow,
//! capability gap, scope checks, and the audit trail behind every refusal.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use surge_server::{app, AppState};
use surge_store::tokens::TokenKind;
use tower::ServiceExt;

async fn test_state() -> AppState {
    AppState::new(surge_store::open_in_memory().await.unwrap())
}

/// Media type only — the charset parameter is not what any assertion means.
fn content_type(resp: &axum::response::Response) -> String {
    resp.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

fn req(method: &str, uri: &str, token: Option<&str>, body: Option<serde_json::Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn audit_actions(pool: &sqlx::SqlitePool) -> Vec<String> {
    surge_store::audit::recent(pool, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect()
}

#[tokio::test]
async fn claim_is_one_time_and_mints_a_session() {
    let state = test_state().await;
    let claim = surge_store::tokens::mint(&state.pool, TokenKind::Claim, None, 1).await.unwrap();
    let router = app(state.clone());

    // A guessed claim URL is refused and audited.
    let r = router.clone()
        .oneshot(req("GET", "/claim/cl_wrong", None, None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::GONE);

    // The real one mints a session cookie…
    let r = router.clone()
        .oneshot(req("GET", &format!("/claim/{claim}"), None, None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap().to_string();
    assert!(cookie.starts_with("surge_session=st_"));

    // …exactly once (INV-AUTH-5).
    let r = router.clone()
        .oneshot(req("GET", &format!("/claim/{claim}"), None, None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::GONE);

    // The cookie authenticates a privileged call.
    let session = cookie.split(';').next().unwrap().trim_start_matches("surge_session=").to_string();
    let r = router.clone()
        .oneshot(req("GET", "/api/audit", Some(&session), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    let actions = audit_actions(&state.pool).await;
    assert!(actions.contains(&"auth.claim_refused".to_string()));
    assert!(actions.contains(&"auth.session_claimed".to_string()));
}

#[tokio::test]
async fn the_capability_gap_is_enforced_and_audited() {
    let state = test_state().await;
    let session = surge_store::tokens::mint(&state.pool, TokenKind::Session, None, 1).await.unwrap();
    let router = app(state.clone());

    // No token → 401. Invalid token → 401 + audit.
    let r = router.clone().oneshot(req("GET", "/api/audit", None, None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = router.clone().oneshot(req("GET", "/api/audit", Some("st_forged"), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // Human creates a project and mints its runtime token.
    let r = router.clone()
        .oneshot(req("POST", "/api/projects", Some(&session),
            Some(serde_json::json!({"id": "prj_a", "name": "a", "repo_path": "/tmp/a"}))))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let r = router.clone()
        .oneshot(req("POST", "/api/projects/prj_a/runtime-token", Some(&session), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let rt = body_json(r).await["token"].as_str().unwrap().to_string();
    assert!(rt.starts_with("rt_"));

    // INV-AUTH-2: the runtime token at a privileged endpoint → 403, audited.
    let r = router.clone()
        .oneshot(req("POST", "/api/projects/prj_a/runtime-token", Some(&rt), None))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let actions = audit_actions(&state.pool).await;
    assert!(actions.contains(&"auth.runtime_refused_privileged".to_string()));
    assert!(actions.contains(&"auth.invalid_token".to_string()));
}

#[tokio::test]
async fn runtime_capabilities_are_project_scoped() {
    let state = test_state().await;
    let session = surge_store::tokens::mint(&state.pool, TokenKind::Session, None, 1).await.unwrap();
    let router = app(state.clone());

    for prj in ["prj_a", "prj_b"] {
        let r = router.clone()
            .oneshot(req("POST", "/api/projects", Some(&session),
                Some(serde_json::json!({"id": prj, "name": prj, "repo_path": "/tmp"}))))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
    }
    let mint = |p: &str| req("POST", &format!("/api/projects/{p}/runtime-token"), Some(&session), None);
    let rt_a = body_json(router.clone().oneshot(mint("prj_a")).await.unwrap()).await["token"]
        .as_str().unwrap().to_string();
    let rt_b = body_json(router.clone().oneshot(mint("prj_b")).await.unwrap()).await["token"]
        .as_str().unwrap().to_string();

    // A run in project a (seeded directly — dispatch is item 7).
    let run = surge_domain::observatory::Run {
        id: "run_1".into(),
        project_id: "prj_a".into(),
        issue_id: None,
        kind: surge_domain::observatory::RunKind::Doc,
        materialization_hash: "sha256:mat".into(),
        work_order_hash: None,
        status: surge_domain::observatory::RunStatus::Running,
        started_at: 1_000,
        ended_at: None,
        cost: 0.0,
    };
    surge_store::observatory::insert_run(&state.pool, &run).await.unwrap();

    // Capability 5: own-run status poll works for its own project…
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_1", Some(&rt_a), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // …and is refused loudly across projects.
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_1", Some(&rt_b), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    // Capability 4: append a span with the right token.
    let span = serde_json::json!({
        "id": "sp_1", "run_id": "run_1", "parent_span_id": null, "node_id": null,
        "role": "worker", "started_at": 2000, "duration_ms": 5, "status": "ok",
        "cost": 0.01, "depth": 0, "policy_decision": null, "body": "did work"
    });
    let r = router.clone()
        .oneshot(req("POST", "/runtime/runs/run_1/spans", Some(&rt_a), Some(span.clone())))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let r = router.clone()
        .oneshot(req("POST", "/runtime/runs/run_1/spans", Some(&rt_b), Some(span)))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    // The human token is a superset on runtime routes (§04).
    let r = router.clone().oneshot(req("GET", "/runtime/runs/run_1", Some(&session), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    let actions = audit_actions(&state.pool).await;
    assert_eq!(actions.iter().filter(|a| *a == "auth.runtime_refused_scope").count(), 2);
}

/// Walk-3 finding N11: an unknown path under either nested zone must answer a
/// JSON 404, not the SPA shell. Before this, the rust-embed fallback caught
/// every nest miss and returned `200 text/html` — an API client was told
/// "fine" and handed the UI, and the shape of the answer told an anonymous
/// caller which routes exist (401) and which do not (200).
#[tokio::test]
async fn unknown_api_and_runtime_paths_are_json_404s() {
    let state = test_state().await;
    let session = surge_store::tokens::mint(&state.pool, TokenKind::Session, None, 1).await.unwrap();
    let router = app(state.clone());

    for path in ["/api/nonexistent", "/runtime/nonexistent"] {
        // The 404 does not depend on authentication: the fallback is
        // registered after the auth layer, so it answers the same either way.
        for token in [None, Some(session.as_str())] {
            let r = router.clone().oneshot(req("GET", path, token, None)).await.unwrap();
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path} (token: {})", token.is_some());
            assert_eq!(content_type(&r), "application/json", "{path} must not answer HTML");
            assert_eq!(body_json(r).await["error"], "unknown endpoint");
        }
    }

    // A real route still answers from its own handler, not the fallback.
    let r = router.clone().oneshot(req("GET", "/api/audit", Some(&session), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // And a genuine UI path still belongs to the SPA fallback: the embedded
    // shell in a built tree, the dev-workflow pointer in a tree with no
    // `npm run build` — never the API's JSON.
    let r = router.oneshot(req("GET", "/projects", None, None)).await.unwrap();
    let (status, ct) = (r.status(), content_type(&r));
    assert_ne!(ct, "application/json", "a UI path was answered as an API path");
    if status == StatusCode::OK {
        assert!(ct.starts_with("text/html"), "embedded shell must be HTML, got {ct}");
    }
}

#[tokio::test]
async fn session_rotation_signs_everyone_out() {
    let state = test_state().await;
    let old = surge_store::tokens::mint(&state.pool, TokenKind::Session, None, 1).await.unwrap();
    let router = app(state.clone());

    let r = router.clone().oneshot(req("POST", "/api/session/rotate", Some(&old), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let new = body_json(r).await["token"].as_str().unwrap().to_string();

    let r = router.clone().oneshot(req("GET", "/api/audit", Some(&old), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = router.clone().oneshot(req("GET", "/api/audit", Some(&new), None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
