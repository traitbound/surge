# Code map

Area → path → safe-parallel rules. Paths are planned; update in the same commit that scaffolds them.

Contents are annotated with when each piece arrives: **(now)** exists in the tree today · numeric phase ids scheduled for that epic and not yet written. *(2026-08-26 smoke walk 5, invariant audit: annotations added. The `ui` and `server` rows had listed a React Flow canvas, a tracker mirror and an SSE bridge with no marking — none of them exist, and nothing in the row said so. A planned path is fine here; a planned path indistinguishable from a shipped one is not.)*

| Area | Path | Contents | Safe to parallelize? |
|---|---|---|---|
| server | `crates/server/` | Axum API **(now)**, token middleware **(now)**, dispatcher/leases **(now)**, embedded UI + plugin asset serving **(now)**, tracker mirror **(3.4)**, SSE bridge **(2.2)** | Yes across route modules |
| store | `crates/store/` | SQLite pool **(now)**, embedded `sqlx` migrations **(now)**, typed repository functions **(now)**, compile-checked queries + in-memory integration tests **(now)**; commit-broadcast emission **(2.2, with the SSE bridge — ADR-3)** | Yes across repository modules; **no** two tasks touching the schema definitions at once |
| compiler | `crates/compiler/` | pipeline → materialization **(now)**, INV-ID-1 hashing over emitted bytes (`materialization_hash`) **(now)**, capability report **(now)**, repo writes + surge-managed gitignore block **(now)**; truthful not-enforced egress line **(now)**; tested per-line egress tiering and project allowlist **(2.2)** | Yes, but any change to hash inputs is `role:critical` and serialized |
| domain | `crates/domain/` | the twelve entities **(now)**, `ts-rs` derives **(now)**, invariant-bearing types **(now)**, INV-ID-2's `pipeline_content_hash` **(now, moved from `compiler` by ESC-4 2026-08-29 — a pipeline's identity is a pure function of its graph, so it derives beside the graph types; any change to hash inputs is `role:critical` and serialized)**. `crates/domain/bindings/` is ts-rs's *default* export dir — a byproduct of the `#[ts(export)]` derives, gitignored via `crates/*/bindings/`, superseded by `ui/src/generated/` and read by nothing. Do not import from it. | **No** — one task at a time; every other area depends on it |
| ui | `ui/` | React + Vite app **(now)**, global shell + project list + runs/span tree **(now)**, generated types (read-only output of domain) **(now)**; minimal React Flow canvas **(1.1)**, supporting UI dependencies only as needed **(1.1)**, SSE subscriptions **(2.2)**, full authoring pages **(3.1)** | Yes across surfaces; never hand-edit generated types |
| supervisor | `crates/server/` (supervisor module) | worktree-per-lease spawn **(now)**, env token injection **(now)**, TTL/reclaim/abort **(now)**, observability floors on terminalization — no-spans (NEW-2) and no-commit-on-task-branch **(now, walk-7 R1; git state is the signal, never span content — INV-EXEC-3)**, graph sequencing/attribution and per-run metering/gates **(1.2 — unwritten)**; role/wave metering and parallel queue **(2.2)**; `run.cost`/`span.cost` are unmetered 0.0 today and must display `cost n/m` | **No** — serialized; `role:critical` |
| cli | `crates/cli/` | `surge` command: auth/claim URL **(now)**, status **(now)**, compile **(now)**, dispatch **(now)**, abort **(now)** | Yes; thin shim over the API |
| claude-plugin | `integrations/claude-plugin/` | Host-agnostic MCP server and hook scripts. Codex is the primary spawn (2026-10-09): `.codex/config.toml` registers the server, `.codex/hooks.json` registers the scripts **(now)**. Four runtime tools — work-order fetch · span append · heartbeat · own-run poll **(now)**; claim-lease tool **(2.2)**; authoritative fetch/node attribution **(1.2)**. The directory name is the Phase 0 Claude plugin path; the process that loads it is `codex exec`. | Yes; independent of server internals — talks only to the public runtime API |
| docs | `docs/` | product layer, features, phases | Yes; append-only under parallel agents |

## Generated directories

Three directories are build output, gitignored, and must never be hand-edited or imported from by path:

| Path | Produced by | Guard |
|---|---|---|
| `ui/src/generated/` | `cargo test -p surge-domain` (ts-rs) | `ui/scripts/ensure-generated.mjs`, run by npm `predev`/`prebuild`/`pretypecheck` (smoke walk 5, F3) |
| `ui/dist/` | `cd ui && npm run build` | `crates/server/build.rs` creates the directory so `rust-embed` compiles on a fresh clone (smoke walk 4, S1) |
| `crates/domain/bindings/` | ts-rs `#[ts(export)]` default output, a byproduct of the same test | none needed — nothing reads it; `ui/src/generated/` is the canonical copy |

## Replan boundary — 2026-10-12

See [roadmap](../phases/README.md). Existing paths are not proof of execution semantics: the compiler emits files/hashes today; graph validation, node sequencing, attempt attribution and cost gates are 1.2. No new datastore or runtime adapter is introduced by the replan. Runtime/frontend browser acceptance remains 0.1.
