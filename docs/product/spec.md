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

A single Rust binary serves everything on the loopback port. Inside it: an Axum HTTP API (human-token and runtime-token routes, enforced by middleware), a SQLite database (all twelve entities, runs/spans, audit), a materialization compiler that writes compiled files into bound workplace repos, a dispatcher/lease manager driving the execution lifecycle, a tracker mirror that reads external trackers (Linear/GitHub/built-in) and never writes back, and an SSE stream feeding the UI. The React UI is embedded in the binary and runs in the operator's browser; it is a pure projection of API state. IDE runtimes are thin clients: they fetch their compiled pipeline and work orders via the runtime token, claim leases, heartbeat, and append spans — nothing else. Bound repos receive exactly three kinds of writes: `surge.yaml`, compiled `.claude/` files, and pipeline-declared docs.

See [`architecture.md`](architecture.md) for the diagram (must agree exactly with this paragraph) and ADRs.

## Non-goals

- Multi-user, auth beyond the two-token model, remote/cloud deployment.
- Writing to external trackers (Plan is mirror-only).
- Executing agent work itself — runtimes execute; Surge compiles, dispatches, observes.
- Template push-back from project canvases (cut for v1 — promote-to-fork instead, design §23).
