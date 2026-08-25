//! The Surge object model. Rust structs here are the single source of truth;
//! TypeScript types are generated from them via `ts-rs` (ADR-1) into
//! `ui/src/generated/` — regenerate with `cargo test -p surge-domain`.
//!
//! The twelve entities (design §03) land here in Phase 0 item 2. This module
//! currently carries only the types the scaffolded API surface needs.

use serde::Serialize;
use ts_rs::TS;

/// Response body of `GET /healthz` — the first type to cross the `ts-rs` seam.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Health {
    /// Crate version of the running binary.
    pub version: String,
    /// Schema version: latest applied `sqlx` migration (ADR-9).
    /// `i64` crosses the wire as a JSON number, so the TS side is `number`,
    /// not ts-rs's default `bigint` (phase-0 scoping assumption on numeric reprs).
    #[ts(type = "number")]
    pub schema_version: i64,
}

#[cfg(test)]
mod ts_export {
    use super::*;
    use ts_rs::TS;

    /// Generates the TypeScript projection. `#[ts(export)]` types export when
    /// this test target runs; the explicit call pins the output directory.
    #[test]
    fn export_typescript_bindings() {
        Health::export_all_to("../../ui/src/generated").expect("ts-rs export failed");
    }
}
