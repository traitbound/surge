//! Library item — Hook · Subagent · Skill (design §03, §04).
//! Immutable per version (INV-DATA-2); trust-gated on import (INV-AUTH-3).

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryItem {
    pub id: String,
    pub kind: LibraryItemKind,
    pub name: String,
    pub version: i64,
    /// The item body (hook script, subagent .md, SKILL.md content).
    pub body: String,
    pub trust: TrustState,
    pub created_at: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LibraryItemKind {
    Hook,
    Subagent,
    Skill,
}

impl LibraryItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LibraryItemKind::Hook => "hook",
            LibraryItemKind::Subagent => "subagent",
            LibraryItemKind::Skill => "skill",
        }
    }
}

/// Untrusted items never materialize; compile is hard-blocked while any
/// referenced item is untrusted (INV-AUTH-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export)]
pub enum TrustState {
    Local,
    ImportedUntrusted,
    ImportedReviewed { by: String, at: Millis },
}
