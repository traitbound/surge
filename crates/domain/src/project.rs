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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PipelineAssignmentStatus {
    Published,
    /// The pipeline moved since the last compile — dispatch refused (INV-ID-1).
    Stale,
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
