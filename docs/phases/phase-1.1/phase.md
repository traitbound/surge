# Phase 1.1 — The faithful canvas

**Status:** not_started
**Parent:** [phase-1](../phase-1/phase.md) · **Epic 1 of 3, runs first**
**Commitment level:** Phase 1 — ships to the operator.
**Time horizon:** ~2 weeks — the least defensible estimate in the plan (carried from the parent, 2026-08-12). If it slips, cut from 1.3 first; nothing in this epic is cuttable without losing the thesis.

## Purpose

Prove the architectural thesis of phase 1: **what you draw is exactly what materializes.** A pipeline authored on the canvas and the same pipeline pasted as text must compile to the same hash, and presentation state must never touch that hash. Every later epic consumes an editable, versioned pipeline object; until the hash contract holds, nothing built on it can be trusted.

This is the load-bearing epic (diagnostic Q2) and runs first for the same reason Phase 0 proved the actuator before the canvas: the risky claim goes first, even ugly.

## In scope

1. **Pipeline editor core**: React Flow canvas, six node kinds, edges with triggers and required-gate locks, multi-select, undo/redo (design §11). *Not* grouping/blocks — those are 1.3.
2. **Two-way code sync**: canvas ⇄ textual pipeline representation (the Phase 0 data format becomes the paste/round-trip format). Hash inputs per INV-ID-2: semantic content only, presentation state never hashed.
3. **Pipeline versioning**: fork with provenance, version history, blessed flag (INV-DATA-3); project-local edits create a local revision hash immediately, promote-to-fork adopts it (INV-ID-3, design §23-Nine). The history *surface* (list, diff view) is 1.3; the identity machinery is here.

## Out of scope

- Grouping/blocks with exposed parameters → 1.3 (deliberately: it is the parent's named first cut, and must not sit in the epic that proves the thesis)
- Library, trust, compile dialog, upgrade review → 1.2
- Pipelines page, project overview, canvas modes, EVAL tab → 1.3
- Everything in the parent's whole-phase Out of scope

## Done when

- A pipeline drawn from scratch on the canvas compiles to the same hash as its pasted textual form (INV-ID-2).
- Moving nodes, adding frames or adding stickies changes **no** hash — proved by asserting hash equality across a presentation-only mutation, not by inspecting the serializer.
- Editing a project canvas immediately shows `vN + local rev <hash>` on the assignment line; promote-to-fork names that revision as the fork's provenance (INV-ID-3).
- Forking a blessed template, editing the fork and compiling leaves the template's hash and history untouched.
- Round-tripping canvas → text → canvas → text produces byte-identical text on the second pass for every one of the six node kinds.

*Each line above names an exercisable surface. That is deliberate: three of seven phase-0 walks found a Done-when line asserting something the tree did not do (`smoke-patterns.md`, walk-5 F2 / walk-6 R2 / walk-7 W1).*

## Architecture (this epic)

Strict subset of the parent. Same containers as Phase 0; the browser grows the canvas and the round-trip seam only. No library surfaces, no dialogs.

```mermaid
graph TB
    operator([Operator - single user])

    subgraph binary["Surge binary — Rust · 127.0.0.1:7420"]
        api["Axum HTTP API<br/>human-token & runtime-token routes"]
        db[("SQLite — embedded, single file<br/>entities · runs/spans · audit<br/>+ pipeline versions & provenance")]
        compiler["Materialization compiler<br/>pipeline × project → files<br/>INV-ID-2 hash inputs"]
        supervisor["Runtime supervisor (single-task)"]
        ui_assets["Embedded React UI"]
    end

    browser["React UI in browser<br/>+ React Flow canvas (six node kinds, edges/gates)<br/>+ canvas ⇄ code round-trip"]
    repo[("Bound workplace repo")]

    operator --> browser
    browser -->|"human token"| api
    ui_assets --> browser
    api --> db
    compiler --> db
    compiler -->|"writes compiled files"| repo
    supervisor --> db
```

## Anticipated specs

| Feature | Hint |
|---|---|
| canvas-editor | React Flow, six node kinds, edges/gates, selection, undo/redo |
| code-roundtrip | canvas ⇄ text format, hash-fidelity contract, byte-identical second pass |
| pipeline-versioning | fork, provenance, local revision hash, blessed flag (identity machinery, not the history UI) |

## Scoping assumptions

- verify at spec time: the Phase 0 pipeline data format is expressive enough to be the round-trip format. Canvas-only state (positions, frames, stickies) is excluded from the hash by INV-ID-2, so lossiness there cannot break fidelity — but it *can* break the byte-identical second pass, which is why that is a separate Done-when line.
