//! INV-ID-2, executable: the pipeline content hash covers semantic content
//! only — nodes (kind, config, gates, fanout), edges (endpoints, triggers,
//! gate flags), pinned library references (inside config). Presentation state
//! (positions, labels, metric annotations) never enters the hash. Two graphs
//! that execute identically must hash identically.

use serde::Serialize;
use sha2::{Digest, Sha256};
use surge_domain::pipeline::{Edge, Node, NodeConfig};

/// The exact node fields the hash covers. Adding a field here is a
/// `role:critical` change — it re-hashes every pipeline in existence.
#[derive(Serialize)]
struct SemanticNode<'a> {
    id: &'a str,
    human_gate: bool,
    config: &'a NodeConfig,
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
        .map(|n| SemanticNode { id: &n.id, human_gate: n.human_gate, config: &n.config })
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
