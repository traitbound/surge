//! Materialization identity (INV-ID-1).
//!
//! `pipeline_content_hash` (INV-ID-2) used to live here too; it moved to
//! `surge_domain::pipeline_content_hash` on 2026-08-29 (ESC-4). Its inputs
//! were only domain types, so keeping it downstream of the object model meant
//! neither the fixture nor the store could derive an identity for a graph they
//! held in their hands. [`materialization_hash`] stays: its third argument is
//! [`crate::CompiledFile`] — the exact bytes this crate emits — and the object
//! model has no notion of a compiled file. Moving it would drag emission into
//! `surge-domain`; INV-ID-1 is about what a compile produced, not about the
//! object model.

use sha2::{Digest, Sha256};

/// Materialization identity (INV-ID-1): the pipeline's semantic hash × the
/// project × the exact bytes emitted.
pub fn materialization_hash(
    pipeline_hash: &str,
    project_id: &str,
    files: &[crate::CompiledFile],
) -> String {
    let mut h = Sha256::new();
    h.update(pipeline_hash.as_bytes());
    h.update([0]);
    h.update(project_id.as_bytes());
    for f in files {
        h.update([0]);
        h.update(f.rel_path.as_bytes());
        h.update([0]);
        h.update(f.contents.as_bytes());
    }
    format!("sha256:{}", hex::encode(h.finalize()))
}
