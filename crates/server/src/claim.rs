//! One-time session claim (INV-AUTH-5): only the browser that visits the
//! printed URL holds a session.

use crate::{now_ms, AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use surge_store::tokens::TokenKind;

pub async fn claim_session(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Response {
    let now = now_ms();
    match surge_store::tokens::consume_claim(&state.pool, &token, now).await {
        Ok(true) => {}
        Ok(false) => {
            // A guessed or reused claim URL is a refusal worth seeing (INV-ERR-1).
            let _ = surge_store::audit::record(
                &state.pool,
                "auth.claim_refused",
                "/claim",
                "unknown",
                None,
                now,
            )
            .await;
            return (StatusCode::GONE, "claim link invalid or already used").into_response();
        }
        Err(e) => {
            eprintln!("claim lookup failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "claim backend error").into_response();
        }
    }
    let session = match surge_store::tokens::mint(&state.pool, TokenKind::Session, None, now).await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("session mint failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "claim backend error").into_response();
        }
    };
    let _ = surge_store::audit::record(
        &state.pool,
        "auth.session_claimed",
        "/claim",
        "human",
        None,
        now,
    )
    .await;
    (
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            format!("surge_session={session}; HttpOnly; SameSite=Strict; Path=/"),
        )],
        "session claimed — you can close this tab and open Surge",
    )
        .into_response()
}
