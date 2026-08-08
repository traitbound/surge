# Code map

Area → path → safe-parallel rules. Paths are planned; update in the same commit that scaffolds them.

| Area | Path | Contents | Safe to parallelize? |
|---|---|---|---|
| server | `crates/server/` | Axum API, token middleware, dispatcher/leases, tracker mirror, SSE | Yes across route modules; **no** two tasks touching the schema or migrations at once |
| compiler | `crates/compiler/` (or module in server initially) | pipeline → materialization, hashing, repo writes | Yes, but any change to hash inputs is `role:critical` and serialized |
| domain | `crates/domain/` | the twelve entities, `ts-rs` derives, invariant-bearing types | **No** — one task at a time; every other area depends on it |
| ui | `ui/` | React + Vite app, React Flow canvas, generated types (read-only output of domain) | Yes across surfaces; never hand-edit generated types |
| docs | `docs/` | product layer, features, phases | Yes; append-only under parallel agents |
