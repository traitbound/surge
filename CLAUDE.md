# Surge

## Top rules

1. Read `ENGAGEMENT.md` before any orchestration or process decision — it is the operational contract.
2. Docs are repo-canonical under `docs/`; never fork planning content into external tools or the vault.
3. Hybrid push policy: docs and trivial fixes direct to `main`; agent task work via per-task worktree + PR.
4. Tasks and todos never go in this file — they belong in the tracker (pending) or `docs/`.
5. Update this file in the same commit as any code change that makes it stale.

## Stack

Decided 2026-08-08 (nothing implemented yet). Local single-user service on `127.0.0.1:7420`, shipped as one static binary.

- **Server: Rust** — Axum (tower middleware enforces the human-token/runtime-token boundary), SQLite embedded via `sqlx` (single file, WAL, compile-time-checked queries, offline metadata committed; no second process, no network listener), `serde`, `sha2` for content hashing.
- **Store discipline:** every query lives in `crates/store` behind a typed repository function, compile-checked by `sqlx` and covered by an in-memory integration test; repository functions that write also fire the commit broadcast (ADR-3). Lease, gate, trust and hash paths are `role:critical` (ADR-2).
- **UI: React + Vite** — React Flow (`@xyflow/react`) for the pipeline canvas, TanStack Router/Query, Tailwind. UI is a projection of server state; all rules enforced at the API.
- **Type seam: `ts-rs`** — Rust structs are the single source of truth for the object model; TypeScript types are generated from them at build time.
- **Realtime: SSE** (`axum::response::sse`) for span streaming, heartbeats, toasts.
- **Distribution:** built UI embedded via `rust-embed`; `cargo build` yields one binary, no Node at runtime.

## Layout

- `docs/product/spec.md` — persisted PRD (compact product layer)
- `docs/product/architecture.md` — container diagram + ADRs
- `docs/product/invariants.md` — INV-* rows; binding on all specs and code
- `docs/product/code-map.md` — area → path → safe-parallel rules
- `docs/features/INDEX.md` · `docs/phases/` — Layer 4/5 artefacts (empty until phase scoping)
- `docs/design.md` — V3 page-by-page UI spec, the behavioural authority (§23 holds resolutions)
- `ENGAGEMENT.md` — operational decisions (project type, repo shape, push/branch policy, ceremony tier)
- `.halfcycle.json` — machine-readable projection of ENGAGEMENT decisions
- `.claude/context/` — per-area agent context files (append-only)
- `crates/` — cargo workspace: `domain` (object model, `ts-rs` derives), `store` (SQLite/`sqlx`, embedded migrations), `server` (Axum, bin `surge-server`), `cli` (bin `surge`)
- `ui/` — Vite + React app; `ui/src/generated/` is ts-rs output (gitignored, regenerate via domain tests, never hand-edit)
- `integrations/claude-plugin/` — Claude Code plugin (MCP server + hooks, ADR-8); lands with Phase 0 item 6

## Key commands

- `cargo build --workspace` — build everything (set `SQLX_OFFLINE=true` if no dev DB; CI/fresh clones work from committed `.sqlx/`)
- `cargo test --workspace` — tests; the `surge-domain` test target also regenerates `ui/src/generated/` (ts-rs)
- Schema-change loop: edit `crates/store/migrations/` → `sqlx migrate run --source crates/store/migrations` (with `DATABASE_URL=sqlite://$PWD/.dev.db`; `sqlx database create` once) → `cargo sqlx prepare --workspace -- --all-targets` → commit `.sqlx/` in the same change
- `cargo run -p surge-server` — serve `127.0.0.1:7420` (`--db <path>`, default `surge.db`)
- `ui/`: `npm run dev` (proxies to :7420) · `npm run build` · `npm run typecheck`

## Conventions

- Monorepo workspaces from day one; one package per deployable/major concern.
- Vocabulary from `docs/design.md` §01 is binding: pipeline, library, materialization, work order, Board·Plan, Board·Ops, observatory. Don't invent synonyms.
- Only four things may be written into a bound workplace repo (see design §01 closed exception list / INV-DATA-1): `surge.yaml`, compiled `.claude/` runtime files, pipeline-declared docs, rendered `work_orders/` files. Reads are closed too (INV-DATA-6): declared docs, `work_orders/`, git state.
- Surge owns the actuator: a runtime supervisor spawns headless `claude -p` workers for leased issues (INV-EXEC-1, ADR-5); it never performs the creative work and never drives interactive sessions.

## Env var names

- `DATABASE_URL` / `SQLX_OFFLINE` — sqlx dev tooling only (compile-time query checking); never read by Surge at runtime.

Record names only here — values go to personal memory or local `.env` (gitignored).

## Tasks pointer

No tracker yet. Labels to scaffold when one exists are listed in `ENGAGEMENT.md` → Tracker.

## Context Maintenance

Where does a new fact go?

- Team-relevant fact → this file
- Secret / machine-specific → personal memory (`~/.claude/projects/<slug>/memory/`)
- Agent infrastructure → `.claude/`
- Task / todo → tracker or flat file, **never** this file
- Gotcha / debugging story → personal memory, cross-linked
- History / narrative → `docs/project-log.md`, never here

Git rule: update CLAUDE.md **in the same commit** as the code change that makes it stale. `.claude/context/<area>.md` files are append-only under parallel agents: add a date+branch-stamped section at the bottom, never overwrite.

## Must-knows recap

Greenfield, solo (Tier 0, no gates). Monorepo, repo-canonical docs, hybrid push, per-task worktrees off `main`, no CI yet. Rust server + React UI, `ts-rs` seam, embedded SQLite (`sqlx`) as the single store. Product layer is persisted under `docs/product/`; invariants there are binding. `docs/design.md` remains the detailed behavioural authority.
