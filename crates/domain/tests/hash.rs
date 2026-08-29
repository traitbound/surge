//! INV-ID-2, executable — the tests that make `pipeline_content_hash`
//! `role:critical`-safe. They moved here with the function on 2026-08-29
//! (ESC-4); the assertions are unchanged.

use surge_domain::pipeline::EdgeTrigger;
use surge_domain::{fixtures, pipeline_content_hash};

/// The identity of the checked-in fixture, pinned to a literal.
///
/// Captured from `surge-compiler` before ESC-4 moved the function into this
/// crate, and asserted after: a relocation that changed one byte of the hash
/// would have re-keyed every pipeline in existence. It is also the standing
/// guard for anything that edits the semantic projection — if this constant
/// has to change, the change is `role:critical` and needs a migration story,
/// not a green diff.
const FIXTURE_HASH: &str =
    "sha256:796565e8138a76f516d5a4fc71440b2526a7cf11bd301a4fff965d4e6de8a29d";

/// ESC-4's other half (Defect B): the fixture states its own identity, and
/// that identity is the hash of its own graph — nothing downstream has to
/// supply it, so nothing downstream can supply a different one.
#[test]
fn fixture_identity_is_self_derived_and_byte_stable() {
    let (nodes, edges) = fixtures::two_node_graph();
    assert_eq!(pipeline_content_hash(&nodes, &edges), FIXTURE_HASH, "INV-ID-2 output changed");
    assert_eq!(fixtures::two_node_pipeline().content_hash, FIXTURE_HASH);
}

#[test]
fn hash_covers_semantics_and_ignores_presentation() {
    let (nodes, edges) = fixtures::two_node_graph();
    let base = pipeline_content_hash(&nodes, &edges);

    // Presentation churn: same hash (INV-ID-2).
    let mut moved = nodes.clone();
    moved[0].x = 999.0;
    moved[1].label = "Renamed".into();
    moved[0].metric_binding = Some("pass@k".into());
    assert_eq!(pipeline_content_hash(&moved, &edges), base);

    // Node order never matters.
    let mut reversed = nodes.clone();
    reversed.reverse();
    assert_eq!(pipeline_content_hash(&reversed, &edges), base);

    // Semantic changes: different hash.
    let mut gated = nodes.clone();
    gated[1].human_gate = true;
    assert_ne!(pipeline_content_hash(&gated, &edges), base);

    let mut retriggered = edges.clone();
    retriggered[0].trigger = EdgeTrigger::Passed;
    assert_ne!(pipeline_content_hash(&nodes, &retriggered), base);

    let mut repinned = nodes.clone();
    if let surge_domain::pipeline::NodeConfig::Agent { subagent, .. } = &mut repinned[1].config {
        subagent.version = 2; // a version bump is semantic (INV-DATA-2)
    }
    assert_ne!(pipeline_content_hash(&repinned, &edges), base);
}
/// INV-ID-2, the case the original test never built: a Block node's collapse
/// state is canvas state and must not change the pipeline's identity. Hashing
/// `NodeConfig` wholesale meant it did (review 2026-08-26).
#[test]
fn block_collapse_state_never_enters_the_hash() {
    use surge_domain::pipeline::{Node, NodeConfig};
    let (mut nodes, edges) = fixtures::two_node_graph();
    let block = |collapsed: bool| Node {
        id: "nd_block".into(),
        pipeline_id: "pl_two_node_v1".into(),
        label: "Group".into(),
        x: 0.0,
        y: 0.0,
        human_gate: false,
        emits_span: false,
        metric_binding: None,
        metric_note: None,
        config: NodeConfig::Block {
            members: vec!["nd_write_summary".into(), "nd_implement".into()],
            exposed_params: vec!["model".into()],
            collapsed,
        },
    };

    nodes.push(block(false));
    let open = pipeline_content_hash(&nodes, &edges);
    nodes.pop();
    nodes.push(block(true));
    let shut = pipeline_content_hash(&nodes, &edges);
    assert_eq!(open, shut, "collapse state is presentation (INV-ID-2)");

    // But the block's semantic content still counts.
    nodes.pop();
    let mut different = block(true);
    if let NodeConfig::Block { members, .. } = &mut different.config {
        members.pop();
    }
    nodes.push(different);
    assert_ne!(pipeline_content_hash(&nodes, &edges), open, "membership is semantic");
}
