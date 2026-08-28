//! The token boundary (design §04). Refusals are loud and audited:
//! a runtime token at a privileged endpoint is INV-AUTH-2's case; any
//! presented-but-invalid token is INV-ERR-1's "token rejection".

use crate::{now_ms, AppState};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use surge_store::tokens::{Identity, TokenLookup};

/// Bearer header first, then the session cookie (browser).
fn presented_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Some(t.trim().to_string());
            }
        }
    }
    let cookies = req.headers().get(axum::http::header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|c| {
        c.trim()
            .strip_prefix("surge_session=")
            .map(|v| v.to_string())
    })
}

fn refusal(status: StatusCode, reason: &str) -> Response {
    (status, Json(serde_json::json!({ "error": reason }))).into_response()
}

async fn audit_refusal(state: &AppState, action: &str, path: &str, actor: &str, project_id: Option<&str>) {
    // The refusal must still land even if auditing fails; log loudly instead.
    if let Err(e) =
        surge_store::audit::record(&state.pool, action, path, actor, project_id, now_ms()).await
    {
        eprintln!("AUDIT WRITE FAILED for {action} on {path}: {e}");
    }
}

#[allow(clippy::result_large_err)] // the Err is a ready-to-send Response by design
/// `Request`'s body is `!Sync`, so everything auth needs is copied out of the
/// request before the first await — the middleware future must stay `Send`.
async fn identify(state: &AppState, token: Option<String>, path: &str) -> Result<Option<Identity>, Response> {
    let Some(token) = token else {
        return Ok(None);
    };
    match surge_store::tokens::lookup_active(&state.pool, &token, now_ms()).await {
        Ok(TokenLookup::Active(id)) => Ok(Some(id)),
        Ok(TokenLookup::Expired) => {
            // Named apart from "invalid" so an operator whose project runtime
            // token aged out is told what actually happened (INV-ERR-1; the
            // expiry itself is F1's fix).
            audit_refusal(state, "auth.expired_token", path, "unknown", None).await;
            Err(refusal(
                StatusCode::UNAUTHORIZED,
                "token expired — mint a fresh project runtime token (Settings → API TOKENS)",
            ))
        }
        Ok(TokenLookup::Unknown) => {
            audit_refusal(state, "auth.invalid_token", path, "unknown", None).await;
            Err(refusal(StatusCode::UNAUTHORIZED, "invalid or revoked token"))
        }
        Err(e) => {
            eprintln!("token lookup failed: {e}");
            Err(refusal(StatusCode::INTERNAL_SERVER_ERROR, "auth backend error"))
        }
    }
}

/// `/api/*`: human session token only.
pub async fn require_human(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let (token, path) = (presented_token(&req), req.uri().path().to_string());
    match identify(&state, token, &path).await {
        Err(resp) => resp,
        Ok(None) => refusal(StatusCode::UNAUTHORIZED, "authentication required"),
        Ok(Some(Identity::Runtime { project_id, .. })) => {
            // INV-AUTH-2: refused loudly, never silently dropped.
            let actor = format!("rt:{project_id}");
            audit_refusal(
                &state,
                "auth.runtime_refused_privileged",
                &path,
                &actor,
                Some(&project_id),
            )
            .await;
            refusal(
                StatusCode::FORBIDDEN,
                "runtime token refused at privileged endpoint (audited)",
            )
        }
        Ok(Some(Identity::Human)) => {
            req.extensions_mut().insert(Identity::Human);
            next.run(req).await
        }
    }
}

/// `/runtime/*`: runtime token (project-scoped) or human token (superset).
pub async fn require_runtime_or_human(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let (token, path) = (presented_token(&req), req.uri().path().to_string());
    match identify(&state, token, &path).await {
        Err(resp) => resp,
        Ok(None) => refusal(StatusCode::UNAUTHORIZED, "authentication required"),
        Ok(Some(identity)) => {
            req.extensions_mut().insert(identity);
            next.run(req).await
        }
    }
}
