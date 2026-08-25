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
    pub fresh: bool,
    pub created_at: Millis,
}
