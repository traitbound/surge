# Surge

## Top rules

1. Read `ENGAGEMENT.md` before any orchestration or process decision — it is the operational contract.
2. Docs are repo-canonical under `docs/`; never fork planning content into external tools or the vault.
3. Hybrid push policy: docs and trivial fixes direct to `main`; agent task work via per-task worktree + PR.
4. Tasks and todos never go in this file — they belong in the tracker (pending) or `docs/`.
5. Update this file in the same commit as any code change that makes it stale.

## Stack

Decided 2026-08-08 — this section records the **decisions**, not what is built; phase 0 has landed the server, store, compiler, supervisor, CLI, plugin and a minimal UI, and `docs/phases/phase-0/phase.md` is the authority on which parts exist. Local single-user service on `127.0.0.1:7420`, distributed as one Rust service binary with UI/plugin assets embedded; worker integration still requires Codex, Node.js, git and hook utilities.

- **Server: Rust** — Axum (tower middleware enforces the human-token/runtime-token boundary), SQLite embedded via `sqlx` (single file, WAL, compile-time-checked queries, offline metadata committed; no second process, no network listener), `serde`, `sha2` for content hashing.
- **Store discipline:** every query lives in `crates/store` behind a typed repository function, compile-checked by `sqlx` and covered by an in-memory integration test. Lease, gate, trust and hash paths are `role:critical` (ADR-2). ADR-3's commit broadcast — a write repository function firing the SSE channel inside the committing transaction — is **not built yet**; it ships with Phase 2.2's SSE bridge (see `crates/store/src/lib.rs:5`, `docs/phases/phase-0/phase.md` Done-when).
- **UI: React + Vite** — React Flow (`@xyflow/react`) for the pipeline canvas, TanStack Router/Query, Tailwind. UI is a projection of server state; all rules enforced at the API.
- **Type seam: `ts-rs`** — Rust structs are the single source of truth for the object model; TypeScript types are generated from them at build time.
- **Realtime: SSE** (`axum::response::sse`) for span streaming, heartbeats, toasts.
- **Distribution:** built UI embedded via `rust-embed`; `cargo build` yields the service binary. UI assets need no Node server; the embedded MCP script still requires Node.js at worker runtime.

## Layout

- `docs/product/spec.md` — persisted PRD (compact product layer)
- `docs/product/architecture.md` — container diagram + ADRs
- `docs/product/invariants.md` — INV-* rows; binding on all specs and code
- `docs/product/code-map.md` — area → path → safe-parallel rules
- `docs/phases/README.md` — current execution order and binding replan contracts (2026-10-12). Phase 0 is historical Claude acceptance; **0.1 is in progress** for real Codex/browser validation. Phase 1: author (1.1), execute one graph (1.2), library (1.3). Phase 2: approval/Ops (2.1), scheduling/policy (2.2), integration (2.3). Phase 3: authoring surfaces (3.1), observatory/Docs (3.2), operations (3.3), Plan/mirrors (3.4).
- `docs/features/INDEX.md` · `docs/phases/phases-manifest.json` — feature readiness and phase projection. Old plans are preserved under `docs/phases/archive/2026-10-12/`; they are historical, not active instructions.
- `docs/design.md` — V3 page-by-page UI spec, the behavioural authority (latest dated §23 resolutions override prototype page text)
- `ENGAGEMENT.md` — operational decisions (project type, repo shape, push/branch policy, ceremony tier)
- `.halfcycle.json` — machine-readable projection of ENGAGEMENT decisions
- `.claude/context/` — per-area agent context files (append-only)
- `crates/` — cargo workspace: `domain` (object model, `ts-rs` derives, INV-ID-2's `pipeline_content_hash`; hash-input changes are `role:critical`), `store` (SQLite/`sqlx`, embedded migrations), `server` (Axum, bin `surge-server`), `cli` (bin `surge`), `compiler` (materialization compiler + INV-ID-1's `materialization_hash` over emitted bytes)
- `ui/` — Vite + React app; `ui/src/generated/` is ts-rs output (gitignored, regenerate via domain tests, never hand-edit)
- `integrations/claude-plugin/` — host-agnostic MCP server and hook scripts (ADR-8). Codex is the primary runtime (2026-10-09, design §23-Twenty-Three): compiled `.codex/config.toml` registers the server and `.codex/hooks.json` registers the hooks. Four of INV-AUTH-1's five capabilities are exposed (work-order-fetch/span/heartbeat/status); claim-lease is the Phase 2.2 interactive path. The directory name is the Phase 0 path.

## Key commands

- `cargo build --workspace` — build everything (set `SQLX_OFFLINE=true` if no dev DB; CI/fresh clones work from committed `.sqlx/`)
- `cargo test --workspace` — tests; the `surge-domain` test target also regenerates `ui/src/generated/` (ts-rs)
- Schema-change loop: edit `crates/store/migrations/` → `sqlx migrate run --source crates/store/migrations` (with `DATABASE_URL=sqlite://$PWD/.dev.db`; `sqlx database create` once) → `cargo sqlx prepare --workspace -- --all-targets` → commit `.sqlx/` in the same change
- `cargo run -p surge-server` — serve `127.0.0.1:7420` (`--db <path>`, default `surge.db`; `--plugin-dir <path>` to run a working plugin tree instead of the embedded copy); serves the embedded UI from `/` — build `ui/` first (`npm run build`) or the page tells you to (assets embed at compile time, ADR-4). The runtime plugin is embedded too and extracted to `<db-dir>/.surge/plugin/<version>` at boot, so `SURGE_PLUGIN_DIR` resolves from any cwd. The tree still lives at `integrations/claude-plugin/`; Codex is the process that loads it.
- `ui/`: `npm ci` once, then `npm run dev` (proxies to :7420) · `npm run build` · `npm run typecheck`. All three are gated by `ui/scripts/ensure-generated.mjs` (npm `pre*` hooks): `ui/src/generated/` is gitignored ts-rs output, so on a fresh clone the guard runs `cargo test -p surge-domain` for you, or fails naming that exact command when `cargo` is unreachable. No manual ordering to remember — `cargo` and `npm` can be run in either order (smoke walk 5, F3)

## Conventions

- Monorepo workspaces from day one; one package per deployable/major concern.
- Vocabulary from `docs/design.md` §01 is binding: pipeline, library, materialization, work order, Board·Plan, Board·Ops, observatory. Don't invent synonyms.
- Only five things may be written into a bound workplace repo (see design §01 closed exception list / INV-DATA-1): `surge.yaml`, compiled Codex runtime files (`.codex/`, `.agents/skills/`, `AGENTS.md`), pipeline-declared docs, rendered `work_orders/` files, and the surge-managed block inside the repo-root `.gitignore` (that block only, INV-DATA-7). Reads are closed too (INV-DATA-6): declared docs, `work_orders/`, git state.
- Surge owns the actuator: a runtime supervisor spawns headless `codex exec` workers for leased issues (INV-EXEC-1, ADR-5); it never performs the creative work and never drives interactive sessions. Codex loads project `.codex/` only when the project is trusted, so each spawn gets an isolated `CODEX_HOME` that marks the worktree trusted and never edits `~/.codex`.

## Env var names

- `DATABASE_URL` / `SQLX_OFFLINE` — sqlx dev tooling only (compile-time query checking); never read by Surge at runtime.
- Worker-facing (injected by the supervisor at spawn, INV-AUTH-4): `SURGE_API`, `SURGE_RUN_ID`, `SURGE_ISSUE_ID`, `SURGE_RUNTIME_TOKEN`, `SURGE_PLUGIN_DIR`, and `CODEX_HOME` (a per-run directory Surge owns, not the operator's `~/.codex`).
- CLI-facing (read by the `surge` binary): `SURGE_API` (base URL, default `http://127.0.0.1:7420`), `SURGE_TOKEN` (session token, overrides the config file), `SURGE_CONFIG_DIR` (config dir override; default `~/.config/surge`, token stored at `token`, mode 0600 per INV-AUTH-4).

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

Greenfield, solo (Tier 0, no gates). Monorepo, repo-canonical docs, hybrid push, per-task worktrees off `main`, CI establishment is scoped to 0.1; a workflow is drafted locally but no hosted result is verified. Rust server + React UI, `ts-rs` seam, embedded SQLite (`sqlx`) as the single store. Product layer is persisted under `docs/product/`; invariants there are binding. `docs/design.md` remains the detailed behavioural authority.

## Current authoring/runtime contract — 2026-10-12

Project canvas commits on completed mutation with explicit saving/failed state; semantic revisions atomically stale materialization and audit, presentation changes do not. Blocks are semantic (membership/parameters hashed); visual groups are separate presentation state. No hash algorithm changes in the replan. Six-kind authoring does not imply six-kind execution. Current workers have network access enabled with no Surge egress enforcement; the capability report says so. Provider and credential setup must be verified on Codex, not inherited from Claude assumptions. 1.2 proves supported graph execution/attribution/per-run cost gates; 2.2 adds wave policy and SSE.

## Runtime investigation checkpoint — 2026-10-12

Resume from `docs/phases/phase-0.1/investigation-2026-10-12.md`. Runtime changes are uncommitted; real task commit and timed abort passed, but phase acceptance is open. Isolated worker auth can be explicitly supplied via `--codex-auth-file`; source credentials are never written into compiled project files. The handoff records verified evidence, unresolved independent review findings and remaining acceptance checks.
