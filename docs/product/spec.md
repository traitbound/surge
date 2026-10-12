# Surge — Product Spec (PRD)

**Status:** accepted 2026-08-08 · detailed page-by-page UI spec lives in [`docs/design.md`](../design.md) (V3, derived from the V9 prototype). This document is the compact product layer; where the two could disagree, the latest dated resolutions in `docs/design.md` §23 take precedence over prototype page text. [The roadmap](../phases/README.md) owns delivery order; phase plans do not imply shipped functionality.

## Problem

AI coding runtimes (Claude Code, Cursor, Codex) approximate a delivery engine with loose files — prompts, agent configs, hooks and docs scattered per-repo, with no versioning, no observability, and no link between what a human plans, what an agent executes, and what actually ran.

## Product

One local single-user service that owns:

1. **Authorable pipelines** — versioned graphs of doc/agent/hook/skill/stage/block nodes that *compile* (materialize) into the real files Codex reads (`.codex/`, `.agents/skills/`, `AGENTS.md`, `surge.yaml`).
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

A single Rust binary serves everything on the loopback port. Auth is never implied by reaching that port: the human session token is minted only via a one-time claim URL printed to the terminal (INV-AUTH-5), and runtime tokens travel only by spawn-time env injection or `surge auth` machine-local config (INV-AUTH-4). Inside it: an Axum HTTP API (human-token and runtime-token routes, enforced by middleware), an embedded SQLite database (single file, WAL — all twelve entities, runs/spans, audit in one ACID boundary), a materialization compiler that writes compiled files into bound workplace repos, a dispatcher/lease manager driving the execution lifecycle, a runtime supervisor that spawns headless `codex exec` workers for dispatched issues — one git worktree per lease, materialization compiled into it, reaped at lease end (INV-EXEC-1/2; interactive sessions stay human-launched and only claim leases; state transitions derive from Surge-observed facts, never span content, INV-EXEC-3), a repo I/O component owning the closed read path from bound repos (declared docs, `work_orders/`, git state for wave integration — INV-DATA-6), a tracker mirror that reads GitHub and never writes back, with built-in tracker records authored in Surge separately from Ops status (Linear remains deferred), and an SSE stream feeding the UI. The React UI is embedded in the binary and runs in the operator's browser; it is a pure projection of API state. IDE runtimes are thin clients holding the runtime token's five capabilities: fetch their work order/lease/materialization hash at session start (the compiled `.codex/` files, `.agents/skills/`, and `AGENTS.md` on disk are the pipeline itself), claim leases, heartbeat, append spans, and poll own-run status so aborts land at the next tool call — nothing else. Bound repos receive exactly five kinds of writes: `surge.yaml`, compiled Codex runtime files (`.codex/`, `.agents/skills/`, `AGENTS.md`), pipeline-declared docs, rendered `work_orders/` files, and the surge-managed block inside the repo-root `.gitignore` (INV-DATA-1; 2026-08-26 — the fifth was always performed by the compiler and had simply never been enumerated). The first and third are committed, the second and fourth gitignored and reproducible from the materialization hash, and the fifth is what makes that gitignoring true (INV-DATA-7). Runs come in two kinds through the one supervisor: human-triggered **doc runs** (per-run cap only) and issue-backed **work-order runs** (the full lifecycle — eligibility, waves, budgets; design §23-Fourteen). Backup is the third and last external write path: an operator-configured `surge-state.git` remote, credentialed Surge-side, never containing tokens; restore re-mints runtime tokens and requires a fresh session claim (design §23-Fifteen).

See [`architecture.md`](architecture.md) for the diagram (must agree exactly with this paragraph) and ADRs.

## Integration surface

**Codex is the primary runtime — decided 2026-10-09 (design §23-Twenty-Three).** The supervisor spawns `codex exec --sandbox workspace-write --ask-for-approval never -`, with the prompt on stdin. Compile writes Codex's real files, not a renamed `.claude/` tree: `.codex/config.toml` (MCP server `surge`, `required = true`, `features.hooks = true`), `.codex/hooks.json` (the same abort and span scripts; exit 2 still blocks), `.codex/agents/*.toml` (`name`, `description`, `developer_instructions`), `.agents/skills/*/SKILL.md`, and `AGENTS.md`. Codex ignores project `.codex/` unless `projects."<path>".trust_level = "trusted"` is set in the user layer, so each spawn gets an isolated `CODEX_HOME` and the operator's `~/.codex` is never edited. The MCP server and hook scripts still live in `integrations/claude-plugin/`; that tree is host-agnostic. Claude Code as a second runtime is not wired.

**Claude Code plugin via MCP — decided 2026-08-12 (design §23-Eighteen), registration superseded 2026-10-09.** Surge ships an MCP server that exposes the runtime-token capabilities as typed MCP tools, plus the span-emission and guard hooks. Phase 0 registered it from compiled `.claude/settings.json` for `claude -p`. That registration is no longer what the compiler emits. The server and the scripts remain the integration recipe.

**`surge` CLI** — `bind · compile · dispatch · abort · retry · status · auth`; carries first-run session-token claim (INV-AUTH-5) and interactive token setup (INV-AUTH-4). Lands with Phase 0 (thin) and grows with the API. Current authoring/graph execution milestones are 1.1/1.2; full surfaces follow them.

**Runtime providers — replanned 2026-10-12.** Codex is primary. Phase 0.1 validates credential delivery and supported runtime setup; 2.2 adds only providers/model routing proven on that runtime, metering and truthful egress policy; 3.3 exposes those settings. The old Anthropic-default and OpenAI-via-proxy taxonomy is superseded, not silently translated. Proxy bridges and additional runtimes remain deferred; unknown provider pricing/usage takes the unmeterable gate path (INV-EXEC-3).

**Post-V3 integration backlog** (recorded, not committed): spans→eval-fixtures promotion (the label source the metrics wait on) · desktop notification bridge for "Needs you" · OpenTelemetry export of runs/spans · signed pipeline export/import bundles reusing the trust machinery · scheduled dispatch (budget-capped, gate-guarded). Explicitly rejected: remote webhooks (breaks the loopback model), community template registry (hosting/identity burden; export bundles suffice).

## Non-goals

- Multi-user, auth beyond the two-token model, remote/cloud deployment.
- Writing to external trackers (Plan is mirror-only).
- Performing the creative work itself — runtimes do the thinking; Surge compiles, dispatches, supervises worker processes, and observes. (Narrowed 2026-08-12, design §23-Six: Surge *does* spawn headless workers — without an actuator, dispatch semantics were fiction.)
- Template push-back from project canvases (cut for v1 — promote-to-fork instead, design §23).

## Current delivery contract — 2026-10-12

The accepted baseline is materialization plus single-worker supervision, not a general graph executor. 0.1 revalidates Codex/browser behaviour; 1.1 establishes durable authoring; 1.2 proves graph-driven implement → verify → bounded retry and per-run cost gates. Library governance follows in 1.3. Parallel policy, integration, full surfaces and mirrors have separate acceptance milestones. See [roadmap](../phases/README.md).

Project canvas commits on completed mutation, with explicit pending/failed states and acknowledged revision identity. Blocks remain semantic nodes; visual groups are separate presentation state. Existing run snapshots do not change when the author edits. These are target contracts; the replan does not claim their implementation.
