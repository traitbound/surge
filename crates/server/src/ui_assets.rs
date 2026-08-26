//! Embedded UI serving (ADR-4): the built Vite output is compiled into the
//! binary via rust-embed and served from `/` — one file to copy, no Node at
//! runtime. Unknown paths fall back to index.html (SPA routing). The API,
//! runtime, claim and health routes are registered before this fallback and
//! always win — and `/api` and `/runtime` carry their own JSON 404s, so an
//! unknown path under either never reaches here and never answers
//! `200 text/html` (walk-3 finding N11).
//!
//! `ui/dist/` is a build artefact: run `npm run build` in `ui/` before a
//! release build. A dev tree without it still compiles (the directory holds a
//! committed .gitkeep) and serves a pointer to the dev workflow instead.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist/"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() { "index.html" } else { path };
    let file = Assets::get(candidate).or_else(|| Assets::get("index.html"));
    match file {
        Some(f) => {
            let mime = mime_guess::from_path(if Assets::get(candidate).is_some() {
                candidate
            } else {
                "index.html"
            })
            .first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], f.data).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "UI assets not embedded in this build — run `npm run build` in ui/ and rebuild, \
             or use the Vite dev server (`npm run dev`).",
        )
            .into_response(),
    }
}
