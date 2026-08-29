//! Materialization — pipeline × project → compiled files (design §03).
//! Identified by content hash (INV-ID-1); stale refuses dispatch.

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Materialization {
    pub id: String,
    /// Content hash — every run records the hash it executed under (INV-ID-1).
    pub content_hash: String,
    /// Cache key, e.g. `mk_a1b2..fleet`.
    pub cache_key: String,
    pub pipeline_id: String,
    pub project_id: String,
    /// Signed by the instance's human session identity (§04 capability report).
    pub signed_by: String,
    /// **Write-ignored** — `insert_fresh` hardcodes a fresh row, so setting
    /// this on a value you are about to write does nothing; read it back to
    /// learn the truth (ESC-5, mirroring the `Project::pipeline_status`
    /// precedent). A write that must leave a project with no fresh
    /// materialization — the `Stale` state `pipeline-revisions` needs — needs
    /// its own function, not a `false` passed to this one.
    pub fresh: bool,
    pub created_at: Millis,
}

/// The §04 capability report — computed from the live graph at compile, shown
/// to the human whose acceptance *is* the approval of what the pipeline can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CapabilityReport {
    /// Every output path across all doc nodes.
    pub writes: Vec<String>,
    /// Count of stage commands plus hook scripts.
    pub shell_count: u32,
    /// The first three, for the dialog line.
    pub shell_first: Vec<String>,
    /// Which subagents hold WebSearch (empty = "none").
    pub network: Vec<String>,
    /// The project egress allowlist line, e.g.
    /// "empty — all egress refused (loopback to Surge always allowed)".
    pub egress: String,
}
