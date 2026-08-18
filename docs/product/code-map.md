# Code map

Area → path → safe-parallel rules. Paths are planned; update in the same commit that scaffolds them.

| Area | Path | Contents | Safe to parallelize? |
|---|---|---|---|
| server | `crates/server/` | Axum API, token middleware, dispatcher/leases, tracker mirror, SSE bridge | Yes across route modules |
| store | `crates/store/` | embedded SurrealDB connection, SCHEMAFULL definitions, typed repository functions, per-query `kv-mem` tests | Yes across repository modules; **no** two tasks touching the schema definitions at once |
| compiler | `crates/compiler/` (or module in server initially) | pipeline → materialization, hashing, repo writes | Yes, but any change to hash inputs is `role:critical` and serialized |
| domain | `crates/domain/` | the twelve entities, `ts-rs` derives, invariant-bearing types | **No** — one task at a time; every other area depends on it |
| ui | `ui/` | React + Vite app, React Flow canvas, generated types (read-only output of domain) | Yes across surfaces; never hand-edit generated types |
| supervisor | `crates/server/` (supervisor module) | worktree-per-lease spawn, env token injection, TTL/reclaim/abort, cost metering (INV-EXEC-1/2/3) | **No** — serialized; `role:critical` |
| cli | `crates/cli/` (or bin target in server) | `surge` command: auth/claim URL, status, compile, dispatch, abort | Yes; thin shim over the API |
| claude-plugin | `integrations/claude-plugin/` | Claude Code plugin: MCP server (five runtime tools), span/guard hooks, settings registration (ADR-8) | Yes; independent of server internals — talks only to the public runtime API |
| docs | `docs/` | product layer, features, phases | Yes; append-only under parallel agents |
