//! The §04 capability report: what a human accepts when they compile —
//! computed from the live graph, never hand-maintained.

use crate::{item_key, LibraryIndex};
use surge_domain::library::LibraryItemKind;
use surge_domain::materialization::CapabilityReport;
use surge_domain::pipeline::{Node, NodeConfig};

pub fn capability_report(nodes: &[Node], library: &LibraryIndex) -> CapabilityReport {
    let mut writes = Vec::new();
    let mut shell = Vec::new();
    let mut network = Vec::new();

    let mut sorted: Vec<&Node> = nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for n in &sorted {
        match &n.config {
            NodeConfig::Doc { output_path, subagent, .. } => {
                writes.push(output_path.clone());
                if holds_websearch(library, subagent) {
                    network.push(subagent.name.clone());
                }
            }
            NodeConfig::Agent { subagent, .. } => {
                if holds_websearch(library, subagent) {
                    network.push(subagent.name.clone());
                }
            }
            NodeConfig::Stage { command } => shell.push(command.clone()),
            NodeConfig::Hook { hook, .. } => shell.push(format!("hook:{}", hook.name)),
            NodeConfig::Skill { .. } | NodeConfig::Block { .. } => {}
        }
    }
    network.sort();
    network.dedup();

    CapabilityReport {
        shell_count: shell.len() as u32,
        shell_first: shell.iter().take(3).cloned().collect(),
        writes,
        network,
        // The egress allowlist is a project setting that lands in phase 2;
        // until then the default is the closed one, stated rather than hidden
        // (INV-DEPLOY-1).
        egress: "empty — all egress refused (loopback to Surge always allowed)".into(),
    }
}

/// A subagent "holds WebSearch" if its frontmatter tools line grants it.
/// Body-derived until subagent capability metadata is modelled (post-V3 note).
fn holds_websearch(library: &LibraryIndex, r: &surge_domain::pipeline::LibraryRef) -> bool {
    library
        .get(&item_key(LibraryItemKind::Subagent, r))
        .map(|item| {
            item.body
                .lines()
                .any(|l| l.trim_start().starts_with("tools:") && l.contains("WebSearch"))
        })
        .unwrap_or(false)
}
