//! Project — a bound repo (design §03).

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    /// Assigned pipeline, if any: the pinned (pipeline id, name, version, hash).
    pub assigned_pipeline: Option<AssignedPipeline>,
    /// **Derived, never stored** — the store computes it at read time from
    /// `materialization.fresh` (ESC-3). Set it on a value you are about to
    /// write and the write path ignores you; read it back to learn the truth.
    pub pipeline_status: PipelineAssignmentStatus,
    pub surge_yaml_written: bool,
    pub tracker: TrackerKind,
    /// e.g. `task/{issue}` — how task branches are named in this repo.
    pub branch_format: String,
    pub created_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssignedPipeline {
    pub pipeline_id: String,
    pub name: String,
    pub version: i64,
    pub content_hash: String,
}

/// What the project's compiled state is *today*, derived from
/// `materialization.fresh` — the single signal dispatch gates on (INV-ID-1).
///
/// Two variants, because two states are all the system can currently produce.
/// `Stale` used to be the second one, documented as "the pipeline moved since
/// the last compile"; nothing could ever produce it (`insert_fresh` is the only
/// writer of `materialization.fresh` and always inserts a successor in the same
/// transaction, and no route can edit a pipeline yet), while the Registry pill
/// asserted it. Two more states become producible later and each needs its own
/// variant *when something can produce it, not before* (ESC-3): unassigned,
/// once assignment exists; and a genuine `Stale`, once a write can leave a
/// project with materializations of which none is fresh (the planned revision
/// write does exactly that) — the store distinguishes it as `EXISTS(any) AND
/// NOT EXISTS(fresh)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PipelineAssignmentStatus {
    /// A fresh materialization exists — the project clears the INV-ID-1 check
    /// at dispatch.
    Published,
    /// No fresh materialization — dispatch is refused until Compile runs
    /// (INV-ID-1).
    NotCompiled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TrackerKind {
    Linear,
    Github,
    Builtin,
    None,
}
