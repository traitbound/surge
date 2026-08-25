//! Work-order rendering — the fourth INV-DATA-1 write kind. Deterministic:
//! the hash of the rendered bytes is what the issue pins and dispatch checks.

use sha2::{Digest, Sha256};
use surge_domain::board::Issue;

pub fn render_work_order(issue: &Issue) -> String {
    format!(
        "# Work order — {title}\n\nissue: {id}\nproject: {project}\nphase: {phase}\nwave: {wave}\n\n\
         Complete the task described by this issue on its task branch. The compiled\n\
         `.claude/` files in this worktree are the pipeline; follow them.\n",
        title = issue.title,
        id = issue.id,
        project = issue.project_id,
        phase = issue.phase,
        wave = issue.wave,
    )
}

pub fn work_order_hash(content: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())))
}
