# Feature: canvas-editor

**Status:** draft — revised contract, not yet independently reviewed or implemented
**Phase:** phase-1.1
**Owner:** solo
**Last updated:** 2026-10-12

## Summary

A minimal project canvas that faithfully edits the assigned graph without mutating its published source. Each completed mutation persists through the project-local revision API. This feature proves authoring/hash fidelity; [phase-1.2](../phases/phase-1.2/phase.md) separately proves graph-driven execution. The pre-replan draft is [archived](../phases/archive/2026-10-12/features/canvas-editor.md).

## User-facing behaviour

The operator opens a project's assigned pipeline, sees all six existing node kinds and explicit edges, and edits the project graph. Published versions remain immutable. A completed edit immediately enters `saving`; semantic edits are acknowledged with a durable revision hash displayed beside the pinned assignment. There is no separate Save button or hidden unsaved editing mode. Drag/resize previews stay transient until gesture completion, then constitute one mutation.

Compile and dispatch are disabled while a mutation is pending or failed. A failed save leaves the attempted change visible as `not saved`, names the server error and offers retry or explicit discard back to the last acknowledged graph. Navigation warns while a mutation is pending or failed. It never represents an optimistic graph as the graph an existing run is executing.

## Acceptance criteria

1. Load the project graph and render every existing kind (`doc`, `agent`, `hook`, `skill`, `stage`, `block`) with its actual binding/config. An absent assignment is a named state with an assignment action; a missing graph is a named error, not an empty canvas.
2. Edges render trigger and gate state explicitly. Connecting nodes sends explicit values; no silent default alters semantic fields.
3. Opaque Blocks survive unrelated mutations with membership, exposed parameters and collapse state preserved. Composition/parameter authoring is 3.1; rendering does not promise Block execution.
4. Moving a node or changing its label persists but preserves semantic hash and compilation freshness. Semantic mutations return the durable project revision hash and stale materialization, atomically with audit, before the UI claims saved success.
5. Multi-select move/delete is one undo unit. Undo/redo restores graph and selection over at least 20 operations, persists each completed mutation, and creates new semantic revisions rather than rewriting previous rows.
6. Saving/failed states, retry and explicit discard are exercised in the browser. Compile/dispatch cannot use an unacknowledged graph. The server checks the base revision and refuses a conflicting write without silently overwriting newer state.
7. Node/edge ids and all hashed fields survive JSON and save/reload unchanged, including custom triggers whose text matches a reserved trigger. Unsafe numeric input is refused visibly; static `bigint` annotations are not evidence of a lossless JSON transport.

## Boundary contract

Human-token API only. The domain supplies generated DTOs for the project graph, mutation and response; the UI does not hand-write competing hashed-field shapes. `pipeline-revisions` owns the exact endpoint/DTO definition before this draft can become ready. Required inputs are project identity, base revision identity and full nodes/edges; required acknowledgement identifies the persisted graph, semantic hash and materialization freshness. Published pipeline ids are read-only sources, not mutation targets.

Assignment/picker list reads are shared with `pipeline-assignment`. No separate canvas save API may bypass the revision/audit transaction. The actor is the operator and every semantic edit is audited by the revision write path.

## Frontend contract

React Flow renders the domain graph. A reducer records mutation/undo units; graph state is acknowledged, pending or failed. Serialize completed mutations to avoid out-of-order saves; a response never overwrites a newer pending edit. Refetch does not discard an attempted failed mutation. Add query/router/styling dependencies only where the slice needs them, not as a prerequisite to acceptance.

The renderer switches exhaustively on `NodeConfig.kind`. Hash-bearing configs, ids, gate flags and triggers are carried exactly as received unless the operator explicitly changes them. No trimming, silent sorting, defaulting `null` to zero or node-id regeneration.

## Wire precision

Rust `i64` currently generates TS `bigint` declarations, but the existing HTTP serializer sends JSON numbers and `response.json()` creates JavaScript numbers. The declarations do not convert runtime values. The owning revision DTO must define a lossless representation and reject out-of-range version/timing values before this spec is ready. For ordinary safe-integer version fields, JSON must return the exact same value after the canvas mutation. Do not implement a blanket BigInt conversion or stringify workaround in the render layer.

## Dependencies and invariants

- `pipeline-assignment` first completes legacy identity repair and enables truthful list reads. `pipeline-revisions`: persisted project identity, immutable source, atomic revision/audit/freshness, conflict handling and lossless DTO.
- `code-roundtrip`: lossless EdgeTrigger persistence and text interchange.
- Existing domain/store/compiler/shell: graph types, graph read, semantic hash oracle and authenticated navigation.
- INV-ID-1/2/3, INV-DATA-3/8, INV-OBS-1 and INV-ERR-1 apply. Client tests compare pre/post hash on the payload emitted after a real UI mutation; compiler fixtures alone do not prove this surface.

## Smoke hooks

- Assign and load the graph, including an all-six-kind fixture; inspect each kind and its references.
- Move/rename, await acknowledgement and reload: presentation persists, hash/freshness unchanged.
- Change a semantic field: returned revision is displayed and stale dispatch refuses.
- Interrupt a mutation request: failed state preserves the attempt; retry succeeds; explicit discard restores acknowledged graph.
- Undo/redo 20 mixed operations and await each save; inspect persisted graph, history and hash.
- Save/reload Custom("passed"): it remains custom, not the unit Passed variant.

## Non-goals

Full pipelines pages, Block composition and visual groups, dry run/diff/run overlay (3.1), library governance (1.3), debugger (3.2), EVAL/calibration (deferred), multi-user editing. General executable validation belongs to the server executor in 1.2; the UI must never imply unsupported authored kinds will execute.

## Readiness

Draft until `pipeline-revisions` pins mutation/acknowledgement DTOs and precision, `code-roundtrip` owns the trigger fix, and the revised browser contract is reviewed. No stub save endpoint counts as completion.
