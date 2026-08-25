//! File emission per node kind (design §03 node table). Deterministic: files
//! land sorted by path; two nodes may emit the same path only with identical
//! content (e.g. two agent nodes sharing a subagent).

use crate::{item_key, CompileRefusal, CompiledFile, LibraryIndex};
use std::collections::BTreeMap;
use surge_domain::library::LibraryItemKind;
use surge_domain::pipeline::{HookScope, Node, NodeConfig, Pipeline};
use surge_domain::project::Project;

/// The committed `surge.yaml` base — what binding a project writes (phase 0
/// item 4, INV-DATA-1) and what the compiled `surge.yaml` extends with its
/// step blocks. One renderer for both, so bind and compile can never disagree
/// about the header.
pub fn surge_yaml_base(project: &Project) -> String {
    format!(
        "# surge.yaml — managed by surge (committed)\n\
         # Step blocks under `steps:` are compiler-managed; edit the pipeline in Surge, not here.\n\
         project: {}\n\
         name: {}\n",
        project.id, project.name
    )
}

fn skill_path(name: &str) -> String {
    format!(".claude/skills/{name}/SKILL.md")
}

fn agent_path(name: &str) -> String {
    format!(".claude/agents/{name}.md")
}

fn hook_script_path(name: &str) -> String {
    format!(".claude/hooks/{name}.sh")
}

fn insert(
    files: &mut BTreeMap<String, String>,
    path: String,
    contents: String,
) -> Result<(), CompileRefusal> {
    match files.get(&path) {
        Some(existing) if *existing != contents => {
            Err(CompileRefusal::PathConflict { path })
        }
        _ => {
            files.insert(path, contents);
            Ok(())
        }
    }
}

pub fn emit_files(
    pipeline: &Pipeline,
    nodes: &[Node],
    library: &LibraryIndex,
    project: &Project,
) -> Result<Vec<CompiledFile>, CompileRefusal> {
    let body = |kind: LibraryItemKind, r: &surge_domain::pipeline::LibraryRef| -> String {
        library
            .get(&item_key(kind, r))
            .expect("trust check ran before emission")
            .body
            .clone()
    };

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    // settings.json hook entries per event, in stable node-id order.
    let mut hook_entries: Vec<(String, Option<String>, String)> = Vec::new();
    // surge.yaml step blocks, in stable node-id order.
    let mut steps: Vec<(String, String)> = Vec::new();

    let mut sorted: Vec<&Node> = nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    for n in &sorted {
        match &n.config {
            NodeConfig::Doc { subagent, skill, output_path } => {
                insert(&mut files, skill_path(&skill.name), body(LibraryItemKind::Skill, skill))?;
                insert(&mut files, agent_path(&subagent.name), body(LibraryItemKind::Subagent, subagent))?;
                // The doc node's output path is a *declared* write (INV-DATA-1
                // third kind) — recorded in the capability report, produced by
                // the run, never emitted at compile time.
                let _ = output_path;
            }
            NodeConfig::Agent { subagent, .. } => {
                insert(&mut files, agent_path(&subagent.name), body(LibraryItemKind::Subagent, subagent))?;
            }
            NodeConfig::Skill { skill } => {
                insert(&mut files, skill_path(&skill.name), body(LibraryItemKind::Skill, skill))?;
            }
            NodeConfig::Hook { hook, event, matcher, scope } => {
                let script = hook_script_path(&hook.name);
                insert(&mut files, script.clone(), body(LibraryItemKind::Hook, hook))?;
                match scope {
                    HookScope::Session => {
                        hook_entries.push((event.clone(), matcher.clone(), script))
                    }
                    // Step-scoped hooks become surge.yaml step blocks.
                    HookScope::Step => steps.push((n.id.clone(), script)),
                }
            }
            NodeConfig::Stage { command } => steps.push((n.id.clone(), command.clone())),
            // A block compiles to its members, which are ordinary nodes in
            // `nodes` and were emitted above; the composite itself is canvas
            // structure.
            NodeConfig::Block { .. } => {}
        }
    }

    // .claude/settings.json — Claude Code's real hooks schema. The two Surge
    // runtime hooks are always wired: the abort guard is how an abort lands at
    // the next tool call (§06), and span emission is the raw-HTTP fallback for
    // the MCP span tool (ADR-8). Both reach Surge over the always-allowed
    // loopback (INV-DEPLOY-1 exemption) via $SURGE_PLUGIN_DIR scripts.
    let mut events: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let hook_json = |command: &str, matcher: Option<&str>| {
        serde_json::json!({
            "matcher": matcher.unwrap_or("*"),
            "hooks": [{ "type": "command", "command": command }],
        })
    };
    events
        .entry("PreToolUse".into())
        .or_default()
        .push(hook_json("\"$SURGE_PLUGIN_DIR\"/hooks/poll-abort.sh", None));
    events
        .entry("PostToolUse".into())
        .or_default()
        .push(hook_json("\"$SURGE_PLUGIN_DIR\"/hooks/emit-span.sh", None));
    for (event, matcher, script) in &hook_entries {
        events
            .entry(event.clone())
            .or_default()
            .push(hook_json(script, matcher.as_deref()));
    }
    let settings = serde_json::json!({ "hooks": events });
    insert(
        &mut files,
        ".claude/settings.json".into(),
        serde_json::to_string_pretty(&settings).expect("json") + "\n",
    )?;

    // .claude/mcp.json — registers the plugin's MCP server; the supervisor
    // spawns workers with `--mcp-config .claude/mcp.json`. Lives under
    // .claude/ so the write list stays closed (INV-DATA-1).
    let mcp = serde_json::json!({
        "mcpServers": {
            "surge": { "command": "node", "args": ["${SURGE_PLUGIN_DIR}/mcp/server.mjs"] }
        }
    });
    insert(
        &mut files,
        ".claude/mcp.json".into(),
        serde_json::to_string_pretty(&mcp).expect("json") + "\n",
    )?;

    // surge.yaml (committed, INV-DATA-7): the bind-time base header (item 4)
    // extended with the compiler-managed step blocks.
    let mut yaml = surge_yaml_base(project);
    yaml.push_str(&format!("pipeline: {} v{}\nsteps:\n", pipeline.name, pipeline.version));
    if steps.is_empty() {
        yaml.push_str("  []\n");
    } else {
        for (id, run) in &steps {
            yaml.push_str(&format!("  - node: {id}\n    run: {run}\n"));
        }
    }
    insert(&mut files, "surge.yaml".into(), yaml)?;

    Ok(files
        .into_iter()
        .map(|(rel_path, contents)| CompiledFile { rel_path, contents })
        .collect())
}
