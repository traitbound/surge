//! Writing a compile into the bound repo — the INV-DATA-1 write path for three
//! of the five kinds: compiled `.claude/` files, `surge.yaml`, and the
//! surge-managed block inside the repo-root `.gitignore` that INV-DATA-7
//! requires the compiler to maintain (enumerated as the fifth write kind
//! 2026-08-26; outside the markers not a byte is touched).

use crate::Compiled;
use std::fs;
use std::path::Path;

const BLOCK_START: &str = "# >>> surge-managed — do not edit (INV-DATA-7)";
const BLOCK_END: &str = "# <<< surge-managed";

/// Write every compiled file under `repo_root`, then upsert the surge-managed
/// gitignore block: compiled `.claude/` files and `work_orders/` are
/// reproducible from the materialization hash and never merge material;
/// `surge.yaml` stays committed.
pub fn write_to_repo(repo_root: &Path, compiled: &Compiled) -> anyhow::Result<()> {
    anyhow::ensure!(repo_root.is_dir(), "bound repo path does not exist: {repo_root:?}");
    for f in &compiled.files {
        let path = repo_root.join(&f.rel_path);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, &f.contents)?;
    }
    // The stamp file: how a runtime (and the work-order hash check) knows
    // which materialization is on disk. Written after hashing, not hash input.
    let stamp = repo_root.join(".claude/surge-materialization.json");
    fs::write(
        &stamp,
        serde_json::to_string_pretty(&serde_json::json!({
            "materialization_hash": compiled.materialization_hash,
            "cache_key": compiled.cache_key,
        }))? + "\n",
    )?;
    upsert_gitignore_block(repo_root, compiled)?;
    Ok(())
}

fn upsert_gitignore_block(repo_root: &Path, compiled: &Compiled) -> anyhow::Result<()> {
    let mut lines: Vec<String> = vec![BLOCK_START.into()];
    for f in &compiled.files {
        if f.rel_path.starts_with(".claude/") {
            lines.push(f.rel_path.clone());
        }
    }
    lines.push(".claude/surge-materialization.json".into());
    lines.push("work_orders/".into());
    lines.push(BLOCK_END.into());
    let block = lines.join("\n");

    let gi = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&gi).unwrap_or_default();
    let updated = match (existing.find(BLOCK_START), existing.find(BLOCK_END)) {
        (Some(start), Some(end)) => {
            let after = &existing[end + BLOCK_END.len()..];
            format!("{}{}{}", &existing[..start], block, after)
        }
        _ if existing.is_empty() => block + "\n",
        _ => format!("{}\n{}\n", existing.trim_end(), block),
    };
    fs::write(&gi, updated)?;
    Ok(())
}
