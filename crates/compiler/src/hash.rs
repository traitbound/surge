//! INV-ID-2, executable: the pipeline content hash covers semantic content
//! only — nodes (**id**, kind, config, gates, fanout), edges (**id**,
//! endpoints, triggers, gate flags), pinned library references (inside
//! config). Presentation state (positions, labels, collapse state, and the
//! observability fields `emits_span`/`metric_binding`/`metric_note`) never
//! enters the hash. Two graphs that execute identically *and name the same
//! nodes* must hash identically — ids are hash inputs, so a consistently
//! renamed graph is a different pipeline by design (INV-ID-2 amendment,
//! 2026-08-28). Prompt bodies are NOT covered here: they reach identity
//! through the pinned `LibraryRef` plus INV-DATA-2 immutability, and
//! directly in [`materialization_hash`], which covers emitted bytes.

use serde::Serialize;
use sha2::{Digest, Sha256};
use surge_domain::pipeline::{Edge, HookScope, LibraryRef, Node, NodeConfig};

/// The semantic projection of a node's config — an explicit allowlist, not a
/// serialization of `NodeConfig` itself.
///
/// This shape is the enforcement of INV-ID-2, and it is deliberately verbose.
/// Hashing `NodeConfig` wholesale meant presentation state travelled with it:
/// `Block { collapsed }` entered the hash, so toggling a group open in the
/// canvas changed the pipeline's identity — exactly what the invariant forbids
/// (review 2026-08-26). Worse, the rule was enforced by a doc comment, so any
/// field added to any variant would have silently joined the hash.
///
/// Every variant below destructures exhaustively with no `..` rest pattern.
/// That is the guard: adding a field to `NodeConfig` fails to compile here and
/// forces a deliberate semantic-or-presentation decision. Changing what this
/// projection covers re-hashes every pipeline in existence — `role:critical`.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SemanticConfig<'a> {
    Doc { subagent: &'a LibraryRef, skill: &'a LibraryRef, output_path: &'a str },
    Agent { subagent: &'a LibraryRef, fanout: Option<u32> },
    Hook {
        hook: &'a LibraryRef,
        event: &'a str,
        matcher: Option<&'a str>,
        scope: &'a HookScope,
    },
    Skill { skill: &'a LibraryRef },
    Stage { command: &'a str },
    /// `collapsed` is excluded: it is canvas state, named by INV-ID-2 itself.
    Block { members: &'a [String], exposed_params: &'a [String] },
}

fn semantic(config: &NodeConfig) -> SemanticConfig<'_> {
    match config {
        NodeConfig::Doc { subagent, skill, output_path } => {
            SemanticConfig::Doc { subagent, skill, output_path }
        }
        NodeConfig::Agent { subagent, fanout } => SemanticConfig::Agent { subagent, fanout: *fanout },
        NodeConfig::Hook { hook, event, matcher, scope } => {
            SemanticConfig::Hook { hook, event, matcher: matcher.as_deref(), scope }
        }
        NodeConfig::Skill { skill } => SemanticConfig::Skill { skill },
        NodeConfig::Stage { command } => SemanticConfig::Stage { command },
        NodeConfig::Block { members, exposed_params, collapsed: _ } => {
            SemanticConfig::Block { members, exposed_params }
        }
    }
}

/// The exact node fields the hash covers.
#[derive(Serialize)]
struct SemanticNode<'a> {
    id: &'a str,
    human_gate: bool,
    config: SemanticConfig<'a>,
}

#[derive(Serialize)]
struct SemanticEdge<'a> {
    id: &'a str,
    from_node: &'a str,
    to_node: &'a str,
    trigger: &'a surge_domain::pipeline::EdgeTrigger,
    gate_required: bool,
}

pub fn pipeline_content_hash(nodes: &[Node], edges: &[Edge]) -> String {
    let mut ns: Vec<SemanticNode> = nodes
        .iter()
        .map(|n| SemanticNode { id: &n.id, human_gate: n.human_gate, config: semantic(&n.config) })
        .collect();
    ns.sort_by(|a, b| a.id.cmp(b.id));
    let mut es: Vec<SemanticEdge> = edges
        .iter()
        .map(|e| SemanticEdge {
            id: &e.id,
            from_node: &e.from_node,
            to_node: &e.to_node,
            trigger: &e.trigger,
            gate_required: e.gate_required,
        })
        .collect();
    es.sort_by(|a, b| a.id.cmp(b.id));

    let canonical = serde_json::to_vec(&(ns, es)).expect("semantic structs always serialize");
    format!("sha256:{}", hex::encode(Sha256::digest(&canonical)))
}

/// Materialization identity (INV-ID-1): the pipeline's semantic hash × the
/// project × the exact bytes emitted.
pub fn materialization_hash(
    pipeline_hash: &str,
    project_id: &str,
    files: &[crate::CompiledFile],
) -> String {
    let mut h = Sha256::new();
    h.update(pipeline_hash.as_bytes());
    h.update([0]);
    h.update(project_id.as_bytes());
    for f in files {
        h.update([0]);
        h.update(f.rel_path.as_bytes());
        h.update([0]);
        h.update(f.contents.as_bytes());
    }
    format!("sha256:{}", hex::encode(h.finalize()))
}
