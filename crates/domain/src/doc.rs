//! Doc — output of a doc node, chained (design §03, §05).
//! Parent-change badges, never cascade invalidation.

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Doc {
    pub id: String,
    pub project_id: String,
    /// The doc node that writes this doc.
    pub node_id: String,
    /// Repo path (declared by the doc node — the repo file is canonical,
    /// Surge's copy the projection, INV-DATA-6).
    pub path: String,
    /// Previous doc in the chain, if any.
    pub parent_doc_id: Option<String>,
    /// Content hash of the ingested repo file.
    pub content_hash: Option<String>,
    pub gate: DocGateState,
    /// Parent's hash at the moment this doc was approved — a differing live
    /// parent hash is the "parent changed" badge (design §05).
    pub parent_hash_at_approval: Option<String>,
    pub created_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export)]
pub enum DocGateState {
    Pending,
    Approved { by: String, at: Millis },
}
