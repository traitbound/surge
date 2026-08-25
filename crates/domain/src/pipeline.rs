//! Pipeline, Node (six kinds), Edge (design §03).
//!
//! One `Pipeline` row per *published version* — a published version is
//! immutable (INV-DATA-3); a pipeline advances by publishing vN+1.
//! Hash discipline (INV-ID-2): semantic content only — node kind/config/
//! gates/fanout, edge endpoints/triggers/gate flags, pinned library refs.
//! Presentation (`x`, `y`, collapse state) never enters the hash.

use crate::Millis;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    pub version: i64,
    pub content_hash: String,
    pub blessed: bool,
    /// Fork provenance: the pipeline-version row this was forked from.
    pub forked_from: Option<String>,
    pub created_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Node {
    pub id: String,
    pub pipeline_id: String,
    pub label: String,
    /// Presentation only — never hashed (INV-ID-2).
    pub x: f64,
    /// Presentation only — never hashed (INV-ID-2).
    pub y: f64,
    pub human_gate: bool,
    pub emits_span: bool,
    pub metric_binding: Option<String>,
    /// Why this node is measured.
    pub metric_note: Option<String>,
    pub config: NodeConfig,
}

/// Kind-specific behaviour (design §03 node table). Tagged by `kind` so the
/// TS side discriminates on the same field the DB stores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum NodeConfig {
    /// Writes one file. Compiles to a SKILL.md.
    Doc {
        subagent: LibraryRef,
        output_path: String,
        skill: LibraryRef,
    },
    /// Delegates to a subagent. Compiles to an agent `.md`.
    Agent {
        subagent: LibraryRef,
        /// Parallel fanout (§06: implement fans out ×3 by default).
        fanout: Option<u32>,
    },
    /// Binds a library hook to an event, matcher and scope.
    Hook {
        hook: LibraryRef,
        event: String,
        matcher: Option<String>,
        scope: HookScope,
    },
    /// Invokes a library skill by name. Compiles to a SKILL.md.
    Skill { skill: LibraryRef },
    /// A deterministic shell command. Compiles to a surge.yaml step.
    Stage { command: String },
    /// A composite of member nodes with exposed per-instance parameters.
    Block {
        members: Vec<String>,
        exposed_params: Vec<String>,
        collapsed: bool,
    },
}

impl NodeConfig {
    /// The `kind` discriminant as stored in the DB and serialized to JSON.
    pub fn kind(&self) -> &'static str {
        match self {
            NodeConfig::Doc { .. } => "doc",
            NodeConfig::Agent { .. } => "agent",
            NodeConfig::Hook { .. } => "hook",
            NodeConfig::Skill { .. } => "skill",
            NodeConfig::Stage { .. } => "stage",
            NodeConfig::Block { .. } => "block",
        }
    }
}

/// A pinned library reference — name + version, never floating (INV-DATA-2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryRef {
    pub name: String,
    pub version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HookScope {
    Session,
    Step,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Edge {
    pub id: String,
    pub pipeline_id: String,
    pub from_node: String,
    pub to_node: String,
    pub trigger: EdgeTrigger,
    /// A gated edge requires a human unlock — a versioned, logged act.
    pub gate_required: bool,
}

/// Edge trigger vocabulary (design §03). `Custom` carries the raw string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EdgeTrigger {
    DocWritten,
    InvariantsApproved,
    TaskgraphApproved,
    Leased,
    Submitted,
    Passed,
    Failed,
    Scope,
    Custom(String),
}

impl EdgeTrigger {
    pub fn as_str(&self) -> &str {
        match self {
            EdgeTrigger::DocWritten => "doc_written",
            EdgeTrigger::InvariantsApproved => "invariants_approved",
            EdgeTrigger::TaskgraphApproved => "taskgraph_approved",
            EdgeTrigger::Leased => "leased",
            EdgeTrigger::Submitted => "submitted",
            EdgeTrigger::Passed => "passed",
            EdgeTrigger::Failed => "failed",
            EdgeTrigger::Scope => "scope",
            EdgeTrigger::Custom(s) => s,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "doc_written" => EdgeTrigger::DocWritten,
            "invariants_approved" => EdgeTrigger::InvariantsApproved,
            "taskgraph_approved" => EdgeTrigger::TaskgraphApproved,
            "leased" => EdgeTrigger::Leased,
            "submitted" => EdgeTrigger::Submitted,
            "passed" => EdgeTrigger::Passed,
            "failed" => EdgeTrigger::Failed,
            "scope" => EdgeTrigger::Scope,
            other => EdgeTrigger::Custom(other.to_string()),
        }
    }
}
