//! `rust-embed` needs `ui/dist/` to exist at compile time even when the UI has
//! not been built (ADR-4). A tracked `.gitkeep` is the nominal guarantee, but
//! `npm run build` wipes the directory and a subsequent `git add -A` will
//! happily stage that deletion — which is exactly how a fresh clone lost the
//! ability to `cargo build` once already (smoke walk 4, S1). Creating the
//! directory here makes the build structurally immune instead of dependent on
//! one empty file surviving every future commit.

use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    if !dist.is_dir() {
        if let Err(e) = std::fs::create_dir_all(&dist) {
            println!("cargo:warning=could not create {}: {e}", dist.display());
        }
    }
    println!("cargo:rerun-if-changed=../../ui/dist");
}
