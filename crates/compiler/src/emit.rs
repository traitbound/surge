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
    format!(".agents/skills/{name}/SKILL.md")
}

fn agent_path(name: &str) -> String {
    format!(".codex/agents/{name}.toml")
}

fn hook_script_path(name: &str) -> String {
    format!(".codex/hooks/{name}.sh")
}

/// Codex reads repo skills from `.agents/skills/*/SKILL.md` and requires
/// `name` and `description` in the frontmatter. A body that already opens
/// with frontmatter is emitted unchanged; a bare markdown body gets a
/// wrapper so the skill is discoverable.
fn skill_markdown(name: &str, body: &str) -> String {
    if body.starts_with("---\n") || body.starts_with("---\r\n") {
        return body.to_string();
    }
    let description = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(name)
        .trim_start_matches('#')
        .trim();
    format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}")
}

/// Codex custom agents are TOML (`name`, `description`,
/// `developer_instructions`), not Claude markdown agent files. A library
/// body written as Claude frontmatter keeps its `description:` line and its
/// prose; the frontmatter itself is not pasted through, because this schema
/// has no `tools:` key.
fn agent_toml(name: &str, body: &str) -> String {
    let (description, instructions) = split_agent_body(name, body);
    format!(
        "name = {}\ndescription = {}\ndeveloper_instructions = {}\n",
        toml_basic(name),
        toml_basic(&description),
        toml_basic(&instructions),
    )
}

fn split_agent_body(name: &str, body: &str) -> (String, String) {
    let trimmed = body.trim_start();
    if let Some(after) = trimmed.strip_prefix("---") {
        if let Some(end) = after.find("\n---") {
            let frontmatter = &after[..end];
            let instructions = after[end + 4..].trim_start_matches(['\r', '\n']);
            let description = frontmatter
                .lines()
                .find_map(|line| line.trim().strip_prefix("description:"))
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .unwrap_or(name);
            let instructions = if instructions.is_empty() { body } else { instructions };
            return (description.to_string(), instructions.to_string());
        }
    }
    (name.to_string(), body.to_string())
}

fn toml_basic(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// User-level config the supervisor writes into an isolated `CODEX_HOME`.
/// Codex ignores project `.codex/` (config, hooks, rules) unless
/// `projects."<path>".trust_level` is `trusted`, and that key lives in the
/// user layer — never in the project file it is trying to unlock. The
/// supervisor points `CODEX_HOME` at a directory it owns so this trust is
/// not written into the operator's real `~/.codex`.
pub fn codex_trust_config(worktree: &str) -> String {
    format!(
        "# Isolated Codex home for one Surge worker.\n\
         # Project .codex/ layers load only when this path is trusted.\n\
         [projects.{}]\n\
         trust_level = \"trusted\"\n\
         \n",
        toml_basic(worktree),
    )
}

const AGENTS_MD: &str = "\
# Surge\n\
\n\
This worktree is a Surge materialization. The pipeline is the compiled files under `.codex/` and `.agents/skills/`.\n\
Do not edit `surge.yaml`, `.codex/`, `.agents/skills/`, or this file.\n\
\n\
When this session has an issue, call `surge_fetch_work_order` before planning.\n\
Report progress with `surge_append_span`. Check `surge_poll_run` before long steps;\n\
if the run is aborted, stop.\n";

/// Project config Codex reads only after the trust file above has unlocked it.
/// The MCP command is a shell so `$SURGE_PLUGIN_DIR` expands at spawn. An
/// absolute plugin path must not enter the materialization hash.
const CODEX_CONFIG: &str = "\
# Compiled by Surge. Loaded only when the project is trusted.\n\
[features]\n\
hooks = true\n\
\n\
[mcp_servers.surge]\n\
command = \"sh\"\n\
args = [\"-c\", \"exec node \\\"$SURGE_PLUGIN_DIR/mcp/server.mjs\\\"\"]\n\
required = true\n\
default_tools_approval_mode = \"approve\"\n\
env_vars = [\"SURGE_PLUGIN_DIR\", \"SURGE_API\", \"SURGE_RUN_ID\", \"SURGE_RUNTIME_TOKEN\", \"SURGE_ISSUE_ID\"]\n";

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
                insert(
                    &mut files,
                    skill_path(&skill.name),
                    skill_markdown(&skill.name, &body(LibraryItemKind::Skill, skill)),
                )?;
                insert(
                    &mut files,
                    agent_path(&subagent.name),
                    agent_toml(&subagent.name, &body(LibraryItemKind::Subagent, subagent)),
                )?;
                // The doc node's output path is a *declared* write (INV-DATA-1
                // third kind) — recorded in the capability report, produced by
                // the run, never emitted at compile time.
                let _ = output_path;
            }
            NodeConfig::Agent { subagent, .. } => {
                insert(
                    &mut files,
                    agent_path(&subagent.name),
                    agent_toml(&subagent.name, &body(LibraryItemKind::Subagent, subagent)),
                )?;
            }
            NodeConfig::Skill { skill } => {
                insert(
                    &mut files,
                    skill_path(&skill.name),
                    skill_markdown(&skill.name, &body(LibraryItemKind::Skill, skill)),
                )?;
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

    // `.codex/hooks.json` — Codex's command-hook schema. Exit 2 still blocks
    // a tool call, so the existing abort and span scripts stay. `features.hooks`
    // in config.toml is what makes Codex load this file at all.
    let mut events: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let hook_json = |command: &str, matcher: Option<&str>| {
        // Codex treats "*", "", and an omitted matcher as match-all
        // (hooks guide, 2026-10-09). "*" is the one that reads as match-all
        // in the file. A library hook may pass a narrower matcher.
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
    let hooks = serde_json::json!({ "hooks": events });
    insert(
        &mut files,
        ".codex/hooks.json".into(),
        serde_json::to_string_pretty(&hooks).expect("json") + "\n",
    )?;
    insert(&mut files, ".codex/config.toml".into(), CODEX_CONFIG.to_string())?;
    insert(&mut files, "AGENTS.md".into(), AGENTS_MD.to_string())?;

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
