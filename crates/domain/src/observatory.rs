//! Run & Span, COE (design §03, §06). A dispatch emits a run; a run owns a
//! tree of spans. Span content is observability, never control flow
//! (INV-EXEC-3). Compaction may drop span bodies, never structure (INV-OBS-2).

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Run {
    pub id: String,
    pub project_id: String,
    /// Present for work-order runs, absent for doc runs (design §23-Fourteen).
    pub issue_id: Option<String>,
    pub kind: RunKind,
    /// The materialization hash this run executed under (INV-ID-1).
    pub materialization_hash: String,
    pub work_order_hash: Option<String>,
    pub status: RunStatus,
    pub started_at: Millis,
    pub ended_at: Option<Millis>,
    pub cost: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunKind {
    Doc,
    WorkOrder,
}

impl RunKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunKind::Doc => "doc",
            RunKind::WorkOrder => "work_order",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Aborted,
    /// e.g. stale-materialization refusal — a run with one span carrying the
    /// reason (INV-ERR-1).
    Refused,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::Aborted => "aborted",
            RunStatus::Refused => "refused",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Span {
    pub id: String,
    pub run_id: String,
    pub parent_span_id: Option<String>,
    /// The pipeline node that emitted this span, when attributable.
    pub node_id: Option<String>,
    pub role: SpanRole,
    pub started_at: Millis,
    pub duration_ms: Option<i64>,
    pub status: SpanStatus,
    pub cost: f64,
    pub depth: i64,
    pub policy_decision: Option<String>,
    /// Compactable (INV-OBS-2): `None` after compaction; structure fields
    /// above are kept forever.
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SpanRole {
    Coordinator,
    Worker,
    Verifier,
}

impl SpanRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanRole::Coordinator => "coordinator",
            SpanRole::Worker => "worker",
            SpanRole::Verifier => "verifier",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SpanStatus {
    Running,
    Ok,
    Error,
    Refused,
}

impl SpanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanStatus::Running => "running",
            SpanStatus::Ok => "ok",
            SpanStatus::Error => "error",
            SpanStatus::Refused => "refused",
        }
    }
}

/// Cause-of-error record + optional ratchet (design §03).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Coe {
    pub id: String,
    /// Attached to a run or an issue — at least one is set.
    pub run_id: Option<String>,
    pub issue_id: Option<String>,
    pub text: String,
    pub ratchet: Option<Ratchet>,
    pub created_at: Millis,
}

/// A concrete tightening applied against the next pipeline version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Ratchet {
    RoutingFallback { detail: String },
    VerifierCriterion { detail: String },
    GuardHook { detail: String },
    RequiredGate { edge_id: String },
}
