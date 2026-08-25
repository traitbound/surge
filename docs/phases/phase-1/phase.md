# Phase 1 — Author: canvas & library

**Status:** not_started
**Commitment level:** Phase 1 — ships to the operator.
**Time horizon:** ~4–5 weeks — flagged optimistic (2026-08-12): React Flow blocks with exposed parameters *plus* hash-faithful two-way code sync is the least defensible estimate in the plan; if it slips, cut blocks/grouping to a later phase before cutting round-trip fidelity.

## Purpose

Make pipelines and library items *authorable* instead of data-defined: the React Flow canvas with all six node kinds, versioning (fork-never-edit), and the trust-gated library. Tests the second-biggest assumption: that the graph editor can stay faithful to the compiled artifact — what you draw is exactly what materializes (same hash inputs).

## In scope

1. Pipeline editor: React Flow canvas, six node kinds, edges with triggers and required-gate locks, multi-select, grouping/blocks with exposed parameters, undo/redo (design §11).
2. Two-way code sync: canvas ⇄ textual pipeline representation (the Phase 0 data format becomes the paste/round-trip format). Hash inputs per INV-ID-2: semantic content only, presentation state never hashed.
3. Pipeline versioning: fork with provenance, version history with diff, blessed flag (INV-DATA-3); project-local edits create a local revision hash immediately, promote-to-fork adopts it (INV-ID-3, design §23-Nine).
4. Library: Hooks · Subagents · Skills tabs, immutable-per-version publish flow, drafts (INV-DATA-2).
5. Trust flow: imports land untrusted, red banner, Mark reviewed, compile hard-block naming untrusted items (INV-AUTH-3).
6. Compile dialog with the four-line capability report (writes · shell · network · egress) and signature line (design §04).
7. Upgrade review dialog for bumping pinned library versions (design §23-Two).
8. **Pipelines page** (design §09): filter rail, card grid, and the detail view — composition tables, version history with diff, materializations list, broken-reference states. The upgrade-review dialog (7) launches from this page's composition table. Includes the assign-pipeline dialog and reassignment semantics ("fetched at the next session start", design §18).
9. **Project·Overview** (design §12): binding/subagents/hooks cards, assignment card with the stale box, docs chips (data-only until the Phase 3 Docs surface), recent runs.
10. **Canvas modes, first two**: dry run (topological walk, cost estimate, gate callouts) and the builder diff overlay (design §11). Run overlay → Phase 2; debugger → Phase 3.
11. **Inspector EVAL tab** in its disclaimed deterministic form, plus the context-budget bars (design §11); real evals and calibration stay post-V3.
12. **Default library, full set**: the normative seven hooks · six subagents · seven skills backing the doc chain (design §03/§10), authored and published through the library surfaces this phase builds — on the Phase 0 seed.

## Out of scope

- Board·Plan mirror, tracker connections → Phase 2
- Board·Ops, work orders, gates on issues, dispatch queue → Phase 2
- Wave integration, budgets, aborts → Phase 2
- SSE, heartbeat live-lines, toasts beyond basics → Phase 2
- Observatory waterfall, COE, ratchet, metrics, replay, debugger → Phase 3 (the EVAL panel's disclaimed form ships here — in-scope 11)
- Retention/compaction → Phase 3
- Settings surfaces, backup/restore, token rotation UI, egress allowlist editor → Phase 3 (egress *data* exists for the capability report)
- Frames/stickies round-trip fidelity — explicitly deferred; known-lossy per design §23 "Designed but not wired"

## Done when

- A pipeline drawn from scratch on the canvas compiles to the same hash as its pasted textual form (canvas↔code fidelity per INV-ID-2 — moving nodes or adding stickies changes no hash).
- Editing a project canvas immediately shows `vN + local rev <hash>` on the assignment line; promote-to-fork names that revision as the fork's provenance (INV-ID-3).
- Forking a blessed template, editing the fork and compiling leaves the template's hash and history untouched.
- Importing a skill marks it untrusted; compile of a referencing pipeline is refused naming it; Mark reviewed unblocks; every step audit-logged.
- Bumping a pinned library version runs the upgrade review dialog (diff + affected nodes) and produces a new pipeline version.
- The capability report lines match the graph: adding a stage node changes the shell line; granting WebSearch changes the network line.

## Architecture (this phase)

Superset of Phase 0 (same containers, supervisor still single-task; the UI grows the canvas/library surfaces). Still no dispatch queue, repo I/O reads, mirror or SSE.

```mermaid
graph TB
    operator([Operator - single user])

    subgraph binary["Surge binary — Rust · 127.0.0.1:7420"]
        api["Axum HTTP API<br/>human-token & runtime-token routes<br/>(middleware-enforced boundary)"]
        db[("SQLite — embedded, single file<br/>WAL · sqlx compile-checked · one ACID boundary<br/>entities · runs/spans · audit")]
        compiler["Materialization compiler<br/>pipeline × project → files"]
        supervisor["Runtime supervisor (single-task)<br/>worktree per lease · spawn · TTL · abort<br/>(INV-EXEC-1/2)"]
        ui_assets["Embedded React UI<br/>rust-embed static assets"]
    end

    browser["React UI in browser<br/>+ React Flow canvas · library · compile dialog"]
    runtime["Claude Code + surge plugin (MCP)<br/>(runtime token via env / surge auth)"]
    repo[("Bound workplace repo<br/>surge.yaml · .claude/* · work_orders/*")]

    operator --> browser
    browser -->|"human token"| api
    ui_assets --> browser
    supervisor --> db
    supervisor -->|"spawns headless worker"| runtime
    runtime -->|"fetch work order/lease · claim lease<br/>heartbeat · append spans · poll run status"| api
    api --> db
    compiler --> db
    compiler -->|"writes compiled files"| repo
```

## Anticipated specs

| Feature | Hint |
|---|---|
| canvas-editor | React Flow, node kinds, edges/gates, selection, undo/redo |
| blocks-and-groups | composite nodes, palette publish, exposed parameters |
| code-roundtrip | canvas ⇄ text format, hash-fidelity contract |
| pipeline-versioning | fork, provenance, history, diff, blessed flag |
| library-store | items, drafts, publish vN+1, pinning |
| trust-and-import | untrusted state, review flow, compile hard-block |
| compile-dialog | capability report computation + signature |
| upgrade-review | pinned-version bump dialog, affected-node list |
| pipelines-pages | §09 list + detail (composition, history/diff, materializations, broken refs), assign dialog |
| project-overview | §12 two-column health page, cross-surface jumps |
| canvas-modes | dry run walk + builder diff overlay; EVAL tab disclaimed panel + context-budget bars |
| default-library | the normative 7·6·7 item set backing the doc chain, authored on the Phase 0 seed |

Twelve specs — over the rescope threshold (grown by the 2026-08-23 coverage audit: §09/§12 surfaces, two canvas modes, the EVAL panel and the default library previously had no owner). Run `/halfcycle:phase-rescope` before the spec sprint; expected split if needed: editor epic (canvas, blocks, round-trip, modes) vs. surfaces epic (pipelines pages, overview, library, trust, dialogs).

## Scoping assumptions

- scoping assumption — verify at spec time: React Flow's grouping/sub-flow support can express collapsible blocks with exposed parameters without a custom layout engine.
- scoping assumption — verify at spec time: the Phase 0 pipeline data format is expressive enough to be the round-trip format (canvas-only state — positions, frames, stickies — is excluded from the hash by INV-ID-2, so lossiness there cannot break fidelity).
