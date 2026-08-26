//! `rust-embed` needs `ui/dist/` to exist at compile time even when the UI has
//! not been built (ADR-4). This is the sole guarantee that it does: nothing in
//! `ui/dist/` is tracked. The previous approach — a committed `.gitkeep` — was
//! deleted twice, because `npm run build` empties the directory and the next
//! `git add -A` stages the deletion, breaking `cargo build` from a fresh clone
//! (smoke walk 4, S1). A directory this script creates cannot be lost that way.

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
