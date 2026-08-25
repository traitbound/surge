//! Issue & WorkOrder (Board·Ops) and Plan issue (Board·Plan) — design §03.
//! Human-owned fields (disposition, priority) live *beside* orchestration
//! status, never inside it. Plan issues are mirrored, never written back
//! (INV-DATA-5).

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Issue {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub wave: i64,
    pub phase: String,
    /// Orchestration status — derives only from Surge-observed facts (INV-EXEC-3).
    pub status: OrchestrationStatus,
    /// Hash of the rendered work-order file; mismatch → refusal (design §05).
    pub work_order_hash: String,
    pub gate2: Gate2State,
    pub lease: Option<Lease>,
    pub retry_count: i64,
    /// Human-owned, beside status.
    pub disposition: Option<String>,
    /// Human-owned, beside status.
    pub priority: i64,
    /// Wave integration issues (SUR-W1-INT) are ordinary issues that assemble a wave.
    pub is_wave_integration: bool,
    pub created_at: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OrchestrationStatus {
    Draft,
    Eligible,
    Dispatched,
    Leased,
    Verifying,
    Verified,
    Failed,
    Aborted,
    /// Removed by a taskgraph amendment (INV-DATA-4).
    Cut,
}

impl OrchestrationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrchestrationStatus::Draft => "draft",
            OrchestrationStatus::Eligible => "eligible",
            OrchestrationStatus::Dispatched => "dispatched",
            OrchestrationStatus::Leased => "leased",
            OrchestrationStatus::Verifying => "verifying",
            OrchestrationStatus::Verified => "verified",
            OrchestrationStatus::Failed => "failed",
            OrchestrationStatus::Aborted => "aborted",
            OrchestrationStatus::Cut => "cut",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export)]
pub enum Gate2State {
    Pending,
    Reviewed { by: String, at: Millis },
}

/// A live lease (§06: TTL 10 minutes, heartbeats against it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Lease {
    pub owner: String,
    pub run_id: String,
    pub expires_at: Millis,
    pub last_heartbeat_at: Millis,
}

/// The rendered work-order file behind an issue. Revisions clear their
/// Gate-2 review (design §05).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkOrder {
    pub id: String,
    pub issue_id: String,
    /// Path under `work_orders/` in the bound repo (INV-DATA-1).
    pub path: String,
    pub revision: i64,
    pub content_hash: String,
    pub created_at: Millis,
}

/// Read-only projection of a tracker issue (INV-DATA-5). `sprint` and
/// `planning_status` are Surge's own and never sync outward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanIssue {
    pub id: String,
    pub project_id: String,
    pub number: String,
    pub title: String,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    pub assignee: Option<String>,
    pub pr_state: Option<String>,
    pub commit_count: i64,
    pub sprint: Option<String>,
    pub planning_status: Option<String>,
    /// Optional link to the Surge work order — closes the loop between the
    /// two halves of the board.
    pub linked_issue_id: Option<String>,
    pub mirrored_at: Millis,
}
