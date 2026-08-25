//! The Surge object model — the twelve entities of design §03. Rust structs
//! here are the single source of truth (ADR-1); TypeScript is generated via
//! `ts-rs` into `ui/src/generated/` — regenerate with `cargo test -p surge-domain`.
//!
//! Two of the twelve entity headings are pairs (Issue & WorkOrder, Run & Span),
//! so twelve entities are fourteen structs. Closed vocabularies are enums —
//! they carry the invariants (INV-NAME-1); ids stay `String` in Phase 0.
//!
//! Timestamps are Unix milliseconds (`i64`, TS `number`). Semantic vs
//! presentation split (INV-ID-2) is structural: `Node.x`/`Node.y` and other
//! presentation fields live beside — never inside — hash-bearing content.

pub mod audit;
pub mod board;
pub mod doc;
pub mod fixtures;
pub mod library;
pub mod materialization;
pub mod observatory;
pub mod pipeline;
pub mod project;

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

/// Unix-millisecond timestamp. JSON number on the wire.
pub type Millis = i64;

#[cfg(test)]
mod ts_export {
    use ts_rs::TS;

    /// Generates the TypeScript projection for every exported type (deps included).
    #[test]
    fn export_typescript_bindings() {
        const DIR: &str = "../../ui/src/generated";
        super::Health::export_all_to(DIR).unwrap();
        super::project::Project::export_all_to(DIR).unwrap();
        super::pipeline::Pipeline::export_all_to(DIR).unwrap();
        super::pipeline::Node::export_all_to(DIR).unwrap();
        super::pipeline::Edge::export_all_to(DIR).unwrap();
        super::library::LibraryItem::export_all_to(DIR).unwrap();
        super::materialization::Materialization::export_all_to(DIR).unwrap();
        super::doc::Doc::export_all_to(DIR).unwrap();
        super::board::Issue::export_all_to(DIR).unwrap();
        super::board::WorkOrder::export_all_to(DIR).unwrap();
        super::board::PlanIssue::export_all_to(DIR).unwrap();
        super::observatory::Run::export_all_to(DIR).unwrap();
        super::observatory::Span::export_all_to(DIR).unwrap();
        super::observatory::Coe::export_all_to(DIR).unwrap();
        super::audit::AuditEntry::export_all_to(DIR).unwrap();
    }
}
