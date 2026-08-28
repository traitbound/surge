//! Work-order rendering — the fourth INV-DATA-1 write kind. Deterministic:
//! the hash of the rendered bytes is what the issue pins and dispatch checks.
//!
//! The commit sentence is load-bearing, not politeness: `supervisor.rs`'s floor
//! terminalizes a work-order run `failed` when its task branch carries no commit,
//! so this template is where that condition is stated to the party it is enforced
//! against. It cannot live only in the `implementer` subagent definition — a
//! headless top-level worker never reads one (smoke walk 7, W1).

use sha2::{Digest, Sha256};
use surge_domain::board::Issue;

pub fn render_work_order(issue: &Issue) -> String {
    format!(
        "# Work order — {title}\n\nissue: {id}\nproject: {project}\nphase: {phase}\nwave: {wave}\n\n\
         Complete the task described by this issue on its task branch, and commit your\n\
         work there — a work order that leaves no commit on its branch is recorded as\n\
         failed, because Surge cannot see that anything was produced. The compiled\n\
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
