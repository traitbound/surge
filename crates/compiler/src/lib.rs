//! The materialization compiler: pipeline × project → the compiled files a
//! runtime reads (design §03; ADR-6's write half). Pure — callers persist the
//! result and write it into the bound repo via [`write_to_repo`].
//!
//! Hash discipline is this crate's reason to exist as a crate (`role:critical`):
//! [`pipeline_content_hash`] covers semantic content only (INV-ID-2), and a
//! materialization's identity is a content hash every run records (INV-ID-1).

mod capability;
mod emit;
mod hash;
mod write;

pub use capability::capability_report;
pub use hash::{materialization_hash, pipeline_content_hash};
pub use write::write_to_repo;

use std::collections::BTreeMap;
use surge_domain::library::{LibraryItem, LibraryItemKind, TrustState};
use surge_domain::materialization::CapabilityReport;
use surge_domain::pipeline::{Edge, LibraryRef, Node, NodeConfig, Pipeline};
use surge_domain::project::Project;

/// One compiled file, relative to the bound repo root.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFile {
    pub rel_path: String,
    pub contents: String,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    /// Deterministic: sorted by path, conflict-checked.
    pub files: Vec<CompiledFile>,
    pub materialization_hash: String,
    /// e.g. `mk_a1b2c3d4..fleet`.
    pub cache_key: String,
    pub report: CapabilityReport,
}

/// Compile refusals are product behaviour, not errors — each carries the
/// reason string a visible record requires (INV-ERR-1).
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileRefusal {
    /// INV-AUTH-3: compile is hard-blocked while any referenced item is untrusted.
    #[error("Compile refused — {} imported but not yet reviewed. Review in the library first.",
            .names.join(", "))]
    Untrusted { names: Vec<String> },
    #[error("Compile refused — {} referenced but not in the library.", .names.join(", "))]
    Missing { names: Vec<String> },
    #[error("Compile refused — two nodes emit different content at {path}.")]
    PathConflict { path: String },
}

/// The library items a compile needs, keyed by (kind, name, version).
pub type LibraryIndex = BTreeMap<(LibraryItemKind, String, i64), LibraryItem>;

fn item_key(kind: LibraryItemKind, r: &LibraryRef) -> (LibraryItemKind, String, i64) {
    (kind, r.name.clone(), r.version)
}

/// Every pinned library reference in the graph, with the kind each position requires.
pub fn referenced_items(nodes: &[Node]) -> Vec<(LibraryItemKind, LibraryRef)> {
    let mut refs = Vec::new();
    for n in nodes {
        match &n.config {
            NodeConfig::Doc { subagent, skill, .. } => {
                refs.push((LibraryItemKind::Subagent, subagent.clone()));
                refs.push((LibraryItemKind::Skill, skill.clone()));
            }
            NodeConfig::Agent { subagent, .. } => {
                refs.push((LibraryItemKind::Subagent, subagent.clone()));
            }
            NodeConfig::Hook { hook, .. } => refs.push((LibraryItemKind::Hook, hook.clone())),
            NodeConfig::Skill { skill } => refs.push((LibraryItemKind::Skill, skill.clone())),
            NodeConfig::Stage { .. } | NodeConfig::Block { .. } => {}
        }
    }
    refs.sort_by(|a, b| (a.0.as_str(), &a.1.name, a.1.version).cmp(&(b.0.as_str(), &b.1.name, b.1.version)));
    refs.dedup();
    refs
}

/// The compile. Trust is checked first (INV-AUTH-3), then files are emitted
/// deterministically and the materialization identity computed (INV-ID-1/2).
pub fn compile(
    pipeline: &Pipeline,
    nodes: &[Node],
    edges: &[Edge],
    library: &LibraryIndex,
    project: &Project,
) -> Result<Compiled, CompileRefusal> {
    // 1. Every referenced item must exist and be trusted.
    let mut missing = Vec::new();
    let mut untrusted = Vec::new();
    for (kind, r) in referenced_items(nodes) {
        match library.get(&item_key(kind, &r)) {
            None => missing.push(format!("{} v{} ({})", r.name, r.version, kind.as_str())),
            Some(item) => {
                if matches!(item.trust, TrustState::ImportedUntrusted) {
                    untrusted.push(format!("{} ({})", item.name, item.kind.as_str()));
                }
            }
        }
    }
    if !missing.is_empty() {
        return Err(CompileRefusal::Missing { names: missing });
    }
    if !untrusted.is_empty() {
        return Err(CompileRefusal::Untrusted { names: untrusted });
    }

    // 2. Emit files (sorted, conflict-checked).
    let files = emit::emit_files(pipeline, nodes, library)?;

    // 3. Identity: pipeline content hash × project × emitted bytes.
    let pipeline_hash = pipeline_content_hash(nodes, edges);
    let materialization_hash = materialization_hash(&pipeline_hash, &project.id, &files);
    let short = &materialization_hash[7..15]; // skip "sha256:"
    let cache_key = format!("mk_{short}..{}", project.name);

    let report = capability_report(nodes, library);
    Ok(Compiled { files, materialization_hash, cache_key, report })
}
