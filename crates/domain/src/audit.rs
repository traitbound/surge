//! Audit entry — one trail, project-scoped view (design §03, INV-OBS-1).
//! Every privileged act writes one: action, subject, actor, when.

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuditEntry {
    pub id: i64,
    pub action: String,
    pub subject: String,
    pub actor: String,
    /// Project scope, when the act is project-scoped.
    pub project_id: Option<String>,
    pub at: Millis,
}
