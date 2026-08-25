# Surge — Product Spec (PRD)

**Status:** accepted 2026-08-08 · detailed page-by-page UI spec lives in [`docs/design.md`](../design.md) (V3, derived from the V9 prototype). This document is the compact product layer; where the two could disagree, `docs/design.md` §01–§06 is the behavioural authority and this file must be amended.

## Problem

AI coding runtimes (Claude Code, Cursor, Codex) approximate a delivery engine with loose files — prompts, agent configs, hooks and docs scattered per-repo, with no versioning, no observability, and no link between what a human plans, what an agent executes, and what actually ran.

## Product

One local single-user service that owns:

1. **Authorable pipelines** — versioned graphs of doc/agent/hook/skill/stage/block nodes that *compile* (materialize) into the real files a runtime reads (`.claude/*`, `surge.yaml`).
2. **A cross-project library** — hooks, subagents, skills; immutable per version, trust-gated on import.
3. **Projects & boards** — a Plan half mirrored read-only from the repo's tracker, and an Ops half Surge owns outright (work orders, gates, leases, dispatch).
4. **An Observatory** — runs, spans, cause-of-error records, audit log, wired to every pipeline node.

The bet: the graph a human edits, the files a runtime executes, the issues a team tracks and the spans an operator reads are **the same object seen from four angles**, one click apart.

## Users & scale

One operator, local machine, loopback only (`127.0.0.1:7420`). Multi-user, remote access and SaaS are explicitly out of scope.

## Scope anchors (authoritative detail in design.md)

- Object model — twelve entities: §03
- Trust & capability — two tokens, capability report at compile: §04
- Versioning — immutable library versions, fork-not-edit pipelines, amend-not-re-expand taskgraph: §05
- Execution lifecycle — eligibility → dispatch → lease → implement/verify/retry → wave integration → budgets/aborts: §06
- Surfaces — 10 top-level pages, 7 dialogs: §07–§22
- Resolved open questions: §23 Resolutions block

## Architecture (prose)

A single Rust binary serves everything on the loopback port. Auth is never implied by reaching that port: the human session token is minted only via a one-time claim URL printed to the terminal (INV-AUTH-5), and runtime tokens travel only by spawn-time env injection or `surge auth` machine-local config (INV-AUTH-4). Inside it: an Axum HTTP API (human-token and runtime-token routes, enforced by middleware), an embedded SQLite database (single file, WAL — all twelve entities, runs/spans, audit in one ACID boundary), a materialization compiler that writes compiled files into bound workplace repos, a dispatcher/lease manager driving the execution lifecycle, a runtime supervisor that spawns headless `claude -p` workers for dispatched issues — one git worktree per lease, materialization compiled into it, reaped at lease end (INV-EXEC-1/2; interactive sessions stay human-launched and only claim leases; state transitions derive from Surge-observed facts, never span content, INV-EXEC-3), a repo I/O component owning the closed read path from bound repos (declared docs, `work_orders/`, git state for wave integration — INV-DATA-6), a tracker mirror that reads external trackers (Linear/GitHub/built-in) and never writes back, and an SSE stream feeding the UI. The React UI is embedded in the binary and runs in the operator's browser; it is a pure projection of API state. IDE runtimes are thin clients holding the runtime token's five capabilities: fetch their work order/lease/materialization hash at session start (the compiled `.claude/` files on disk are the pipeline itself), claim leases, heartbeat, append spans, and poll own-run status so aborts land at the next tool call — nothing else. Bound repos receive exactly four kinds of writes: `surge.yaml`, compiled `.claude/` files, pipeline-declared docs, and rendered `work_orders/` files (INV-DATA-1); the first and third are committed, the second and fourth gitignored and reproducible from the materialization hash (INV-DATA-7). Runs come in two kinds through the one supervisor: human-triggered **doc runs** (per-run cap only) and issue-backed **work-order runs** (the full lifecycle — eligibility, waves, budgets; design §23-Fourteen). Backup is the third and last external write path: an operator-configured `surge-state.git` remote, credentialed Surge-side, never containing tokens; restore re-mints runtime tokens and requires a fresh session claim (design §23-Fifteen).

See [`architecture.md`](architecture.md) for the diagram (must agree exactly with this paragraph) and ADRs.

## Integration surface

**Claude Code plugin via MCP — decided 2026-08-12 (design §23-Eighteen).** Surge ships a Claude Code plugin (`integrations/claude-plugin/`) bundling an MCP server that exposes the five runtime-token capabilities as typed MCP tools, plus the span-emission and guard hooks. Compiled `.claude/settings.json` registers the plugin's MCP server; raw hook-script HTTP glue remains the fallback for MCP-less runtimes, and the same MCP server is the template for post-V3 Cursor/Codex adapters. Landing: skeleton + span/heartbeat tools in Phase 0 (it *is* the integration recipe), full tool surface in Phase 2.

**`surge` CLI** — `bind · compile · dispatch · abort · status · auth`; carries first-run session-token claim (INV-AUTH-5) and interactive token setup (INV-AUTH-4). Lands with Phase 0 (thin) and grows with the API.

**Model provider registry — decided 2026-08-23 (design §23-Twenty-One).** Instance-level registry of custom model APIs: `anthropic` (default), `anthropic-compatible` (e.g. DeepSeek — base-URL/key env injection at worker spawn), `openai-compatible via proxy` (through a local translation proxy; not shipped in v1). All model references become provider-qualified; keys follow INV-AUTH-6; provider hosts surface on the capability report's egress line. Registry + injection + provider-qualified routing land in Phase 2, the settings card in Phase 3; per-provider cost normalization is post-V3.

**Post-V3 integration backlog** (recorded, not committed): spans→eval-fixtures promotion (the label source the metrics wait on) · desktop notification bridge for "Needs you" · OpenTelemetry export of runs/spans · signed pipeline export/import bundles reusing the trust machinery · scheduled dispatch (budget-capped, gate-guarded). Explicitly rejected: remote webhooks (breaks the loopback model), community template registry (hosting/identity burden; export bundles suffice).

## Non-goals

- Multi-user, auth beyond the two-token model, remote/cloud deployment.
- Writing to external trackers (Plan is mirror-only).
- Performing the creative work itself — runtimes do the thinking; Surge compiles, dispatches, supervises worker processes, and observes. (Narrowed 2026-08-12, design §23-Six: Surge *does* spawn headless workers — without an actuator, dispatch semantics were fiction.)
- Template push-back from project canvases (cut for v1 — promote-to-fork instead, design §23).
