//! Checked-in pipeline fixtures — Phase 0 has no visual editor; pipelines are
//! defined as data. This is the two-node pipeline (one doc node, one agent
//! node) the Phase 0 done-when checklist dispatches.

use crate::pipeline::{Edge, EdgeTrigger, LibraryRef, Node, NodeConfig, Pipeline};

pub const FIXTURE_CREATED_AT: crate::Millis = 1_756_000_000_000;

/// The id of the fixture pipeline. Node and edge rows key on it, and the seed
/// checks it for idempotency. It is not itself a hash input — INV-ID-2 hashes
/// node and edge ids, not the pipeline's.
pub const TWO_NODE_PIPELINE_ID: &str = "pl_two_node_v1";

/// The pipeline row for `two-node v1`, given the identity of its graph.
///
/// `content_hash` is a parameter and not a constant on purpose. A pipeline's
/// identity is *derived* from its graph by `pipeline_content_hash` (INV-ID-2),
/// which lives in `surge-compiler`; `surge-compiler` depends on this crate, so
/// this crate cannot call it. A fixture that carried its own hash could
/// therefore only carry a literal — and the literal it carried through phase 0
/// (`sha256:fixture-two-node-v1`) was never the hash of anything, which made
/// the seeded row's identity meaningless. So the fixture no longer claims an
/// identity it cannot compute: every caller sees both crates and passes
/// `surge_compiler::pipeline_content_hash(&nodes, &edges)` over the graph from
/// [`two_node_graph`].
pub fn two_node_pipeline(content_hash: impl Into<String>) -> Pipeline {
    Pipeline {
        id: TWO_NODE_PIPELINE_ID.into(),
        name: "two-node".into(),
        version: 1,
        content_hash: content_hash.into(),
        blessed: false,
        forked_from: None,
        created_at: FIXTURE_CREATED_AT,
    }
}

/// The graph of `two-node v1`: a doc node writes a summary doc, then an agent
/// node acts on it. This is the whole hash input — pair it with
/// [`two_node_pipeline`].
pub fn two_node_graph() -> (Vec<Node>, Vec<Edge>) {
    let nodes = vec![
        Node {
            id: "nd_write_summary".into(),
            pipeline_id: TWO_NODE_PIPELINE_ID.into(),
            label: "Write summary doc".into(),
            x: 80.0,
            y: 120.0,
            human_gate: false,
            emits_span: true,
            metric_binding: None,
            metric_note: None,
            config: NodeConfig::Doc {
                subagent: LibraryRef { name: "doc-writer".into(), version: 1 },
                output_path: "docs/summary.md".into(),
                skill: LibraryRef { name: "write-summary".into(), version: 1 },
            },
        },
        Node {
            id: "nd_implement".into(),
            pipeline_id: TWO_NODE_PIPELINE_ID.into(),
            label: "Implement".into(),
            x: 360.0,
            y: 120.0,
            human_gate: false,
            emits_span: true,
            metric_binding: None,
            metric_note: None,
            config: NodeConfig::Agent {
                subagent: LibraryRef { name: "implementer".into(), version: 1 },
                fanout: None,
            },
        },
    ];
    let edges = vec![Edge {
        id: "ed_summary_to_impl".into(),
        pipeline_id: TWO_NODE_PIPELINE_ID.into(),
        from_node: "nd_write_summary".into(),
        to_node: "nd_implement".into(),
        trigger: EdgeTrigger::DocWritten,
        gate_required: false,
    }];
    (nodes, edges)
}
