//! Checked-in pipeline fixtures — Phase 0 has no visual editor; pipelines are
//! defined as data. This is the two-node pipeline (one doc node, one agent
//! node) the Phase 0 done-when checklist dispatches.

use crate::pipeline::{Edge, EdgeTrigger, LibraryRef, Node, NodeConfig, Pipeline};

pub const FIXTURE_CREATED_AT: crate::Millis = 1_756_000_000_000;

/// The id of the fixture pipeline. Node and edge rows key on it, and the seed
/// checks it for idempotency. It is not itself a hash input — INV-ID-2 hashes
/// node and edge ids, not the pipeline's.
pub const TWO_NODE_PIPELINE_ID: &str = "pl_two_node_v1";

/// The pipeline row for `two-node v1`, carrying the identity of its own graph.
///
/// The `content_hash` is **derived here** from [`two_node_graph`] by
/// [`crate::pipeline_content_hash`] — it is not a literal and not a parameter.
/// Through phase 0 it was a literal (`sha256:fixture-two-node-v1`) that was
/// never the hash of anything, which made the seeded row's identity
/// meaningless (ESC-1). ESC-1 pushed the derivation out to every caller,
/// because `pipeline_content_hash` then lived in `surge-compiler`, downstream
/// of this crate. ESC-4 moved that function into `surge-domain`, so the
/// fixture can finally state its own identity and no caller can state a
/// different one.
pub fn two_node_pipeline() -> Pipeline {
    let (nodes, edges) = two_node_graph();
    Pipeline {
        id: TWO_NODE_PIPELINE_ID.into(),
        name: "two-node".into(),
        version: 1,
        content_hash: crate::pipeline_content_hash(&nodes, &edges),
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
