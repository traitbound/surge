//! The hash discipline (INV-ID-2), trust gate (INV-AUTH-3) and emission
//! determinism — the tests that make this crate `role:critical`-safe.

use surge_compiler::{compile, pipeline_content_hash, write_to_repo, CompileRefusal, LibraryIndex};
use surge_domain::fixtures;
use surge_domain::library::{LibraryItem, LibraryItemKind, TrustState};
use surge_domain::pipeline::EdgeTrigger;
use surge_domain::project::{PipelineAssignmentStatus, Project, TrackerKind};

fn item(kind: LibraryItemKind, name: &str, body: &str, trust: TrustState) -> LibraryItem {
    LibraryItem {
        id: format!("li_{name}"),
        kind,
        name: name.into(),
        version: 1,
        body: body.into(),
        trust,
        created_at: 1,
    }
}

fn fixture_library() -> LibraryIndex {
    let mut lib = LibraryIndex::new();
    for it in [
        item(LibraryItemKind::Subagent, "doc-writer", "---\ntools: Read, Write\n---\nWrite the doc.", TrustState::Local),
        item(LibraryItemKind::Skill, "write-summary", "# write-summary\nSummarize the repo.", TrustState::Local),
        item(LibraryItemKind::Subagent, "implementer", "---\ntools: Read, Edit, Bash, WebSearch\n---\nImplement.", TrustState::Local),
    ] {
        lib.insert((it.kind, it.name.clone(), it.version), it);
    }
    lib
}

fn project() -> Project {
    Project {
        id: "prj_fix".into(),
        name: "fixture".into(),
        repo_path: "/tmp/unused".into(),
        assigned_pipeline: None,
        pipeline_status: PipelineAssignmentStatus::Published,
        surge_yaml_written: false,
        tracker: TrackerKind::None,
        branch_format: "task/{issue}".into(),
        created_at: 1,
    }
}

#[test]
fn hash_covers_semantics_and_ignores_presentation() {
    let (_, nodes, edges) = fixtures::two_node_pipeline();
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

#[test]
fn fixture_compiles_deterministically() {
    let (pipeline, nodes, edges) = fixtures::two_node_pipeline();
    let lib = fixture_library();
    let a = compile(&pipeline, &nodes, &edges, &lib, &project()).unwrap();
    let b = compile(&pipeline, &nodes, &edges, &lib, &project()).unwrap();
    assert_eq!(a.materialization_hash, b.materialization_hash);

    let paths: Vec<&str> = a.files.iter().map(|f| f.rel_path.as_str()).collect();
    assert_eq!(paths, vec![
        ".claude/agents/doc-writer.md",
        ".claude/agents/implementer.md",
        ".claude/mcp.json",
        ".claude/settings.json",
        ".claude/skills/write-summary/SKILL.md",
        "surge.yaml",
    ]);
    // The always-on runtime hooks and the MCP registration are wired in.
    let settings = &a.files.iter().find(|f| f.rel_path == ".claude/settings.json").unwrap().contents;
    assert!(settings.contains("poll-abort.sh") && settings.contains("emit-span.sh"), "{settings}");
    let mcp = &a.files.iter().find(|f| f.rel_path == ".claude/mcp.json").unwrap().contents;
    assert!(mcp.contains("mcp/server.mjs"), "{mcp}");
    assert!(a.cache_key.starts_with("mk_") && a.cache_key.ends_with("..fixture"));

    // Capability report (§04): the doc write, no shell, implementer holds WebSearch.
    assert_eq!(a.report.writes, vec!["docs/summary.md"]);
    assert_eq!(a.report.shell_count, 0);
    assert_eq!(a.report.network, vec!["implementer"]);
    assert!(a.report.egress.contains("all egress refused"));
}

#[test]
fn untrusted_import_blocks_compile_with_names() {
    let (pipeline, nodes, edges) = fixtures::two_node_pipeline();
    let mut lib = fixture_library();
    lib.get_mut(&(LibraryItemKind::Skill, "write-summary".into(), 1))
        .unwrap()
        .trust = TrustState::ImportedUntrusted;
    let err = compile(&pipeline, &nodes, &edges, &lib, &project()).unwrap_err();
    assert_eq!(err, CompileRefusal::Untrusted { names: vec!["write-summary (skill)".into()] });
    // The dialog line reads like design §04's example.
    assert!(err.to_string().contains("not yet reviewed. Review in the library first."));
}

#[test]
fn missing_item_refuses() {
    let (pipeline, nodes, edges) = fixtures::two_node_pipeline();
    let mut lib = fixture_library();
    lib.remove(&(LibraryItemKind::Subagent, "implementer".into(), 1));
    assert!(matches!(
        compile(&pipeline, &nodes, &edges, &lib, &project()).unwrap_err(),
        CompileRefusal::Missing { .. }
    ));
}

#[test]
fn write_to_repo_lands_files_and_maintains_gitignore_block() {
    let (pipeline, nodes, edges) = fixtures::two_node_pipeline();
    let compiled = compile(&pipeline, &nodes, &edges, &fixture_library(), &project()).unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();

    write_to_repo(repo.path(), &compiled).unwrap();
    write_to_repo(repo.path(), &compiled).unwrap(); // idempotent upsert

    assert!(repo.path().join(".claude/agents/implementer.md").is_file());
    assert!(repo.path().join("surge.yaml").is_file());
    let stamp = std::fs::read_to_string(repo.path().join(".claude/surge-materialization.json")).unwrap();
    assert!(stamp.contains(&compiled.materialization_hash));

    let gi = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(gi.starts_with("target/\n"), "user entries preserved");
    assert_eq!(gi.matches("# >>> surge-managed").count(), 1, "block upserted, not appended");
    assert!(gi.contains(".claude/settings.json"));
    assert!(gi.contains("work_orders/"));
    assert!(!gi.contains("\nsurge.yaml"), "surge.yaml stays committed (INV-DATA-7)");
}
