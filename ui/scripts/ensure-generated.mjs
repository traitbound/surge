#!/usr/bin/env node
// Fresh-clone guard for `ui/src/generated/` (smoke walk 5, F3).
//
// The TypeScript object model is ts-rs output from `crates/domain` and is
// gitignored by design (only a `.gitkeep` is tracked), so a fresh clone has an
// EMPTY directory and `npm run build` failed with a wall of TS2307s that named
// the symptom, never the cause. `crates/server/build.rs` gives `ui/dist/` the
// same kind of guard one layer down; this is its sibling.
//
// npm runs this as `prebuild`/`predev`, so neither entry point can reach tsc
// without types: we regenerate them, or we fail naming the exact command.

import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const uiDir = dirname(dirname(fileURLToPath(import.meta.url)));
const repoRoot = dirname(uiDir);
const generated = join(uiDir, "src", "generated");

const GENERATE = "cargo test -p surge-domain";

function typeCount() {
  if (!existsSync(generated)) return 0;
  return readdirSync(generated).filter((f) => f.endsWith(".ts")).length;
}

function die(why) {
  console.error(
    [
      "",
      `ui/src/generated/ ${why}.`,
      "",
      "Those files are ts-rs output from crates/domain — the Rust structs are the",
      "single source of truth (ADR-1) and the TypeScript is gitignored, so a fresh",
      "clone starts with an empty directory. Generate them from the repo root:",
      "",
      `    ${GENERATE}`,
      "",
      "then re-run this command. Never hand-edit the generated files.",
      "",
    ].join("\n"),
  );
  process.exit(1);
}

if (typeCount() > 0) process.exit(0);

console.error(`ui/src/generated/ is empty — running \`${GENERATE}\`…`);
mkdirSync(generated, { recursive: true });
const cargo = spawnSync("cargo", ["test", "-p", "surge-domain"], {
  cwd: repoRoot,
  stdio: ["ignore", "inherit", "inherit"],
});

if (cargo.error?.code === "ENOENT") {
  die("is empty and `cargo` is not on PATH, so the types cannot be generated here");
}
if (cargo.status !== 0) {
  die(`is empty and \`${GENERATE}\` exited ${cargo.status}`);
}
if (typeCount() === 0) {
  die(`is still empty after \`${GENERATE}\` — the ts-rs export test wrote nothing`);
}
