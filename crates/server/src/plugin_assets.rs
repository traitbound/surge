//! The Claude Code plugin ships *inside* the binary (ADR-4/ADR-8): the
//! `integrations/claude-plugin/` tree is embedded with rust-embed and
//! extracted to a machine-local data dir at boot, so `SURGE_PLUGIN_DIR`
//! points somewhere real no matter where the operator started the process.
//!
//! Before this, `plugin_dir` defaulted to a cwd-relative path; started
//! anywhere but the source checkout it resolved to nothing, and `claude -p`
//! tolerated the missing MCP config silently — workers ran blind, with no
//! spans, no heartbeats and no abort guard (smoke re-walk 2026-08-25, NEW-1).

use std::path::{Path, PathBuf};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../integrations/claude-plugin/"]
struct Plugin;

/// The file whose absence means "this is not a usable plugin dir".
pub const ENTRY: &str = "mcp/server.mjs";

/// Extract the embedded plugin beside the database, under a version-stamped
/// directory so an upgraded binary never reuses a stale tree. Returns the
/// absolute path to hand workers as `SURGE_PLUGIN_DIR`.
pub fn extract_beside_db(db_path: &Path) -> anyhow::Result<PathBuf> {
    let base = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = std::path::absolute(base.join(".surge").join("plugin").join(env!("CARGO_PKG_VERSION")))?;

    anyhow::ensure!(
        Plugin::iter().next().is_some(),
        "no plugin assets embedded in this build — the binary cannot spawn workers"
    );
    for name in Plugin::iter() {
        let file = Plugin::get(&name).expect("iter yields existing files");
        let out = dir.join(name.as_ref());
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, file.data)?;
        // Hook scripts are exec'd by the runtime, not sourced.
        #[cfg(unix)]
        if out.extension().is_some_and(|e| e == "sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    verify(&dir)?;
    Ok(dir)
}

/// A plugin dir is usable only if its MCP entry point is really there. Called
/// at boot and again immediately before every spawn — a worker that cannot
/// reach Surge must never start (NEW-1).
pub fn verify(dir: &Path) -> anyhow::Result<()> {
    let entry = dir.join(ENTRY);
    anyhow::ensure!(
        entry.is_file(),
        "plugin dir has no {ENTRY}: {} — workers would run blind (no spans, no heartbeats, \
         no abort guard). Pass --plugin-dir to point at a real plugin tree.",
        dir.display()
    );
    Ok(())
}
