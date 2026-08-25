//! Checked-in pipeline fixtures — Phase 0 has no visual editor; pipelines are
//! defined as data. This is the two-node pipeline (one doc node, one agent
//! node) the Phase 0 done-when checklist dispatches.

use crate::pipeline::{Edge, EdgeTrigger, LibraryRef, Node, NodeConfig, Pipeline};

pub const FIXTURE_CREATED_AT: crate::Millis = 1_756_000_000_000;

/// `two-node v1`: doc node writes a summary doc, then an agent node acts on it.
pub fn two_node_pipeline() -> (Pipeline, Vec<Node>, Vec<Edge>) {
    let pipeline = Pipeline {
        id: "pl_two_node_v1".into(),
        name: "two-node".into(),
        version: 1,
        // Real content hashing lands with the compiler task (INV-ID-2).
        content_hash: "sha256:fixture-two-node-v1".into(),
        blessed: false,
        forked_from: None,
        created_at: FIXTURE_CREATED_AT,
    };
    let nodes = vec![
        Node {
            id: "nd_write_summary".into(),
            pipeline_id: pipeline.id.clone(),
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
            pipeline_id: pipeline.id.clone(),
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
        pipeline_id: pipeline.id.clone(),
        from_node: "nd_write_summary".into(),
        to_node: "nd_implement".into(),
        trigger: EdgeTrigger::DocWritten,
        gate_required: false,
    }];
    (pipeline, nodes, edges)
}
