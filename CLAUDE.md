# Surge

## Top rules

1. Read `ENGAGEMENT.md` before any orchestration or process decision — it is the operational contract.
2. Docs are repo-canonical under `docs/`; never fork planning content into external tools or the vault.
3. Hybrid push policy: docs and trivial fixes direct to `main`; agent task work via per-task worktree + PR.
4. Tasks and todos never go in this file — they belong in the tracker (pending) or `docs/`.
5. Update this file in the same commit as any code change that makes it stale.

## Stack

Decided 2026-08-08 (nothing implemented yet). Local single-user service on `127.0.0.1:7420`, shipped as one static binary.

- **Server: Rust** — Axum (tower middleware enforces the human-token/runtime-token boundary), SQLite via `sqlx` (compile-time-checked queries, WAL mode), `serde`, `sha2` for content hashing.
- **UI: React + Vite** — React Flow (`@xyflow/react`) for the pipeline canvas, TanStack Router/Query, Tailwind. UI is a projection of server state; all rules enforced at the API.
- **Type seam: `ts-rs`** — Rust structs are the single source of truth for the object model; TypeScript types are generated from them at build time.
- **Realtime: SSE** (`axum::response::sse`) for span streaming, heartbeats, toasts.
- **Distribution:** built UI embedded via `rust-embed`; `cargo build` yields one binary, no Node at runtime.

## Layout

- `docs/design.md` — V3 page-by-page product spec (source of the concept; open questions in its §23)
- `Design.pdf` — original design export
- `ENGAGEMENT.md` — operational decisions (project type, repo shape, push/branch policy, ceremony tier)
- `.halfcycle.json` — machine-readable projection of ENGAGEMENT decisions
- `.claude/context/` — per-area agent context files (append-only)
- Code will land as a cargo workspace under `crates/` (server) plus `ui/` (Vite app) when scaffolded

## Key commands

None yet — no build system exists. Add them here in the same commit that introduces the workspace tooling.

## Conventions

- Monorepo workspaces from day one; one package per deployable/major concern.
- Vocabulary from `docs/design.md` §01 is binding: pipeline, library, materialization, work order, Board·Plan, Board·Ops, observatory. Don't invent synonyms.
- Only three things may be written into a bound workplace repo (see design §01 closed exception list): `surge.yaml`, compiled `.claude/` runtime files, pipeline-declared docs.

## Env var names

None defined yet. Record names only here — values go to personal memory or local `.env` (gitignored).

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

Greenfield, solo (Tier 0, no gates). Monorepo, repo-canonical docs, hybrid push, per-task worktrees off `main`, no CI yet. Concept lives in `docs/design.md`; it is not yet complete — close §23 open questions before L4 specs.
