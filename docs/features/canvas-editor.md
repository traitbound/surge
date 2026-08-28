# Feature: canvas-editor

**Status:** draft
**Phase:** phase-1.1
**Owner:** solo
**Last updated:** 2026-08-28

## Summary

The React Flow pipeline canvas: load a pipeline graph, render all six node kinds and their edges, edit the graph, and hand a well-formed graph payload back to the server. This is the surface phase-1.1's thesis is tested on — *what you draw is exactly what materializes* — so its obligation is faithfulness to the domain model, not richness of interaction. It also carries two costs the phase doc did not originally name: there is **no** pipeline HTTP route today, and the UI has **no** canvas, router, query or styling library installed.

## User-facing behaviour

The operator opens a pipeline and sees its graph: nodes positioned where they were left, each rendered by kind, edges drawn between them with their trigger labelled and gated edges visually distinct. Selecting a node opens an inspector showing that kind's fields. The operator can move nodes, select several at once, connect and disconnect edges, edit node fields, and undo/redo any of it. Saving hands the graph to the server; what the server *does* with it — new version, or project-local revision — is `pipeline-versioning`'s contract, not this feature's.

Moving a node is not an edit to the pipeline's identity. The canvas must make that legible: repositioning changes nothing about what will materialize.

## Acceptance criteria

1. A pipeline graph loaded from the API renders every one of the six node kinds — `doc`, `agent`, `hook`, `skill`, `stage`, `block` — each with a kind-specific inspector exposing exactly the fields that kind carries in `NodeConfig`.
2. Edges render with their `trigger` labelled and `gate_required` edges visually distinguished from ungated ones; connecting two nodes produces an edge whose `trigger` and `gate_required` are explicit, never defaulted silently.
3. A `block` node renders as one opaque node showing its member count, and survives load → edit-something-else → save with its `members`, `exposed_params` and `collapsed` values byte-identical. (Composing, collapsing and parameter-exposing are 1.3.)
4. Moving any node, or changing any node's `label`, produces a graph whose `pipeline_content_hash` is **equal** to the hash before the change — asserted by comparing hashes across the mutation, not by inspecting the serializer.
5. Multi-select (marquee and shift-click) applies move and delete to every selected node, and undo/redo restores both graph state and selection across at least 20 sequential operations.
6. `GET /api/pipelines/{id}` returns the graph (pipeline row, nodes, edges) and the canvas renders it without client-side reshaping of any hashed field; a pipeline id that does not exist returns 404 with a JSON body, not an HTML page.

## Component design

**Boundary contract, pinned first.** The wire shape is the ts-rs projection of the domain types; the client does not invent a view model for hashed fields.

```
GET  /api/pipelines/{id}        -> PipelineGraph { pipeline: Pipeline, nodes: Node[], edges: Edge[] }
GET  /api/pipelines             -> Pipeline[]            (list, for the picker; 1.3 owns the real page)
POST /api/pipelines/{id}/graph  -> body: GraphPayload { nodes: Node[], edges: Edge[] }
                                   response + semantics owned by pipeline-versioning
```

`PipelineGraph` and `GraphPayload` are new `#[derive(TS)]` structs in `crates/domain`, so `ui/src/generated/` gains them from the existing `cargo test -p surge-domain` path — no hand-written duplicates (phase-0 Done-when).

**Backend.** One new route module `crates/server/src/pipeline_api.rs` mounted on the existing human-token router (`crates/server/src/human_api.rs:18`, `lib.rs:95` layers `require_human` over `/api/*`). Reads go through `surge_store::pipelines::load_graph`; nothing new in the store for the read path. The write path calls into `pipeline-versioning`'s repository function — this feature does not write pipeline rows.

**Frontend.** New deps: `@xyflow/react`, TanStack Query, TanStack Router, Tailwind — none currently installed (`ui/package.json` has `react` and `react-dom` only). Structure:

- `ui/src/canvas/` — `Canvas.tsx` (React Flow host), `nodes/` (one component per kind, discriminating on `NodeConfig.kind`), `edges/`, `Inspector.tsx`
- Graph state in a reducer whose actions are the undo/redo unit; React Flow's internal state is a projection of it, not the source of truth. Undo of a multi-select move is one step, not N.
- Server state via TanStack Query; the graph is fetched once and edited locally until save.

**The `kind` discriminant is the switch on both sides.** `NodeConfig` is `#[serde(tag = "kind", rename_all = "snake_case")]` (`crates/domain/src/pipeline.rs:44-46`), and the generated `NodeConfig.ts` is a discriminated union on the same field — so the renderer switches on `kind` with an exhaustive `never` check, and a seventh kind added in Rust fails the TS build rather than rendering blank.

## User flow

- **Empty** — a pipeline with zero nodes renders an empty canvas with its grid and an explicit "no nodes yet" affordance, not a blank rectangle.
- **Loading** — skeleton canvas; the inspector panel is present but disabled, so layout does not jump on arrival.
- **Error** — a failed fetch shows the server's JSON `error` string verbatim plus a retry control. A 404 says the pipeline does not exist; it does not render an empty canvas, which would be indistinguishable from a pipeline with no nodes.
- **Retry** — re-fetch without losing unsaved local edits; if local edits exist, the operator is asked before the refetch replaces them.
- **Exit** — navigating away with unsaved edits prompts. Unsaved state is not silently discarded and not silently persisted.

## Artefact verdicts

- Sequence diagram: **skip** — two actors (browser, server), synchronous request/response, no async coordination, no multi-service state, no compensation. The component design's boundary contract carries what a diagram would.
- Component design: **include** — full-stack feature crossing a new HTTP seam into an existing typed domain; a competent engineer would otherwise have to guess whether the client reshapes hashed fields. The boundary contract is pinned first for exactly that reason.
- User flow: **include** — this is a frontend surface, and its 404-vs-empty distinction is a real correctness concern rather than polish.

## Non-goals

- Deciding what a save *creates*. New version vs project-local revision is `pipeline-versioning`'s contract; this feature produces a graph payload and calls the endpoint.
- Text round-trip. Canvas ⇄ text is `code-roundtrip`; this feature owns only the canvas side of the graph.
- Block authoring UX — composing from selection, collapse/expand, exposing parameters, palette publish (1.3).
- Validating that a graph is *executable* (reachability, cycles, gate sanity). The compiler owns materialization validity; the canvas does not pre-judge it.
- Any pipelines list/detail page beyond the minimal picker needed to open a graph (1.3 owns §09).

## Touches

- **INV-ID-2** — the hash covers semantic content only; AC 4 is this feature's slice of it. The canvas must never send presentation state in a way that reaches a hashed field.
- **INV-DATA-3** — a published version is immutable; the canvas edits a working graph, never a published row in place.
- **INV-ID-3** — a project-canvas edit creates a project-local revision. Consumed, not implemented here (`pipeline-versioning` owns it); this feature must not assume a save is a version bump.
- **INV-AUTH-5** — the canvas is a human-token surface; it mounts behind `require_human` like every other `/api/*` route.

## Events

- Written: none. This feature writes no audit rows of its own; the write path's audit belongs to `pipeline-versioning`.
- Consumed: none. No SSE in phase 1 (parent Out of scope).

## Environment variables

| Var | Purpose | Arg type (build-arg / runtime) | Where set |
|---|---|---|---|
| — | none introduced | — | — |

The UI is served embedded from the binary (`rust-embed`, ADR-4) and talks to a same-origin API, so it needs no `VITE_*` base-URL var. Introducing one would make it a build-arg baked at compile time, which is exactly wrong for a binary shipped to an operator who chooses the port.

## Wire-format contract

| Field | Rust type | JSON / TS | Who transforms | Notes |
|---|---|---|---|---|
| `Node.x`, `Node.y` | `f64` | `number` | none | presentation only; never hashed (`hash.rs:60-65` omits them) |
| `Node.label` | `String` | `string` | none | **presentation** — `SemanticNode` omits it, so editing a label must not change the hash (AC 4) |
| `Node.human_gate` | `bool` | `boolean` | none | **hashed** |
| `Node.config` | `NodeConfig` | discriminated union on `kind` | none | **hashed** (via the `SemanticConfig` allowlist) |
| `NodeConfig::Agent.fanout` | `Option<u32>` | `number \| null` | none | **hashed**; absent and `0` are different graphs — the inspector must not coerce empty to `0` |
| `NodeConfig::Block.collapsed` | `bool` | `boolean` | none | **presentation** — explicitly excluded from the hash (`hash.rs:38`) |
| `LibraryRef.version` | `i64` | **`bigint`** | ts-rs | ts-rs maps `i64` → `bigint`, not `number` (confirmed in `ui/src/generated/AssignedPipeline.ts`). Any arithmetic or `JSON.stringify` on a version must handle BigInt; naive `JSON.stringify` throws on it |
| `Pipeline.version`, `Millis` fields | `i64` | **`bigint`** | ts-rs | same hazard; the existing UI already coerces with `Number(...)` at render sites (`ui/src/observatory.tsx:57`) |

**Coercion site:** BigInt → display happens in the render layer only. Hashed fields are never round-tripped through a coercion — they go back to the server exactly as received.

## Depends on

- `domain-model` (phase-0) — `Pipeline`, `Node`, `NodeConfig`, `Edge`, `EdgeTrigger`, `LibraryRef`, `HookScope` all exist with `ts-rs` derives.
- `store-layer` (phase-0) — `pipelines::load_graph` exists and is compile-checked; the read path needs no new query.
- `minimal-shell-ui` (phase-0) — the global shell, sidebar and project switcher the canvas mounts into.
- `compiler-core` (phase-0) — `pipeline_content_hash` is the oracle AC 4 asserts against.
- `pipeline-versioning` (phase-1.1) — **for the write path only.** This feature calls the save endpoint; that feature defines what a save creates. Authoring order: this spec can be built against a stub write endpoint and completed when versioning lands.

## Approach

1. Add `PipelineGraph` / `GraphPayload` to `crates/domain`, regenerate ts-rs.
2. `crates/server/src/pipeline_api.rs` — `GET /api/pipelines`, `GET /api/pipelines/{id}` over `load_graph`; mount on the human router.
3. Install the four UI deps; add Tailwind and the router shell around the existing surfaces without disturbing them.
4. `Canvas.tsx` over React Flow, six node components discriminating on `kind` with an exhaustive check.
5. Graph reducer + undo/redo stack; React Flow state as a projection.
6. Inspector per kind, driven by the same discriminant.
7. Hash-equality test for presentation mutations (AC 4) — the load-bearing test of this spec.

## Grounded claims

| Claim | Anchor | Verified how |
|---|---|---|
| Six node kinds exist, tagged by `kind`, snake_case | `crates/domain/src/pipeline.rs:44-83` | read the enum and its `kind()` impl; variants are Doc, Agent, Hook, Skill, Stage, Block |
| `Block` is a *node kind*, not a UI grouping | `crates/domain/src/pipeline.rs:76-81` | `NodeConfig::Block { members, exposed_params, collapsed }` is a variant like any other — so it must render in 1.1 even though its authoring UX is 1.3 |
| `members`/`exposed_params` are hashed; `collapsed` is not | `crates/compiler/src/hash.rs:38` | `SemanticConfig::Block { members, exposed_params }` with an explicit comment excluding `collapsed` |
| `label`, `x`, `y` are **not** hashed | `crates/compiler/src/hash.rs:60-65` | `SemanticNode` carries only `id`, `human_gate`, `config` — no label, no coordinates |
| The hash projection is exhaustive-by-compiler, so a new field forces a decision | `crates/compiler/src/hash.rs:20-24` | every variant destructures with no `..` rest pattern; adding a field to `NodeConfig` fails to compile here |
| **No pipeline HTTP route exists today** | `crates/server/src/human_api.rs:18-33` | read every `.route(...)` on the human router: projects, bind, runtime-token, compile, session/rotate, audit, issues, dispatch, retry, runs, abort, spans, doc-run. No `/pipelines` |
| The store can already read a graph; nothing new needed for the read path | `crates/store/src/pipelines.rs:84` | `load_graph` exists alongside `insert_graph` and `reachable_nodes` |
| The store **never updates** a published pipeline | `crates/store/src/pipelines.rs:1-3` | module header: "it inserts whole graphs and reads them; it never updates one" (INV-DATA-3) — so the canvas cannot edit a published row in place |
| The UI has no canvas, router, query or CSS library | `ui/package.json` | dependencies are exactly `react` and `react-dom`; devDependencies are types, the Vite React plugin, TypeScript and Vite |
| `i64` becomes TS `bigint`, not `number` | `ui/src/generated/AssignedPipeline.ts` | generated type reads `version: bigint`; the existing UI coerces with `Number(...)` at render (`ui/src/observatory.tsx:57`) |
| `/api/*` is behind human-token middleware | `crates/server/src/lib.rs:95` | `require_human` is layered over the `/api` router, so a new pipeline route inherits it |
| ts-rs output is generated, gitignored, and regenerated by a test | `docs/product/code-map.md` generated-directories table | `ui/src/generated/` is produced by `cargo test -p surge-domain`, guarded by `ui/scripts/ensure-generated.mjs` |

## Constraint blast radius

**New constraint: the client must not reshape hashed fields.** Enforced by using the generated types directly for `config`, `human_gate`, and edge fields rather than a client view model.

- *Protects:* AC 4 and INV-ID-2 — a client-side normalization (trimming a command string, defaulting `fanout` to 0, sorting `members`) would silently change a pipeline's identity, and the change would be invisible in the UI.
- *Blocks:* any legitimate client-side convenience on those fields. Notably it blocks trimming whitespace in `Stage.command` and coercing an empty `fanout` input to `0`. Both are real ergonomic losses; both must be handled by explicit operator action (an inspector that shows the exact stored value) rather than a silent transform. If a future feature wants normalization, it belongs server-side, before the hash is computed, where it is visible to every client.

## Smoke checklist hooks

Each references a surface this feature actually builds:

- Open a pipeline from the picker; every node kind in the seeded pipeline renders with a kind-specific inspector.
- Drag a node to a new position, save, reload: the position persists **and** `GET /api/pipelines/{id}` reports the same `content_hash` as before the drag.
- Edit a node label, save, reload: label persists, `content_hash` unchanged.
- Request a pipeline id that does not exist: JSON 404, not an HTML page and not an empty canvas.
- Perform 20 mixed operations (move, connect, edit, multi-delete) then undo 20 times: the graph returns to its loaded state and the hash matches the loaded hash.

## Open questions

1. **Does the seeded phase-0 pipeline contain all six node kinds?** `crates/server/src/lib.rs:105-107` seeds a library and a two-node pipeline; if the seed lacks `hook`, `stage` and `block` nodes, AC 1 and the smoke hooks need a richer fixture. Resolve at task time — it may add a seed change to this feature's scope.
2. **What does the canvas do about a graph whose `members` reference node ids not present in the graph?** `reachable_nodes` exists (`crates/store/src/pipelines.rs:155`) but nothing validates block membership. Rendering an opaque block whose members are dangling is possible today. Proposed: render it, surface the dangling ids in the inspector, and do not repair it silently — repair is an authoring act and belongs to 1.3.

## Out of scope

- Dry run, diff overlay, run overlay, debugger (1.3, Phase 2, Phase 3).
- The EVAL tab and context-budget bars (1.3).
- Any change to `pipeline_content_hash` itself. Hash-input changes are `role:critical` (code map, compiler row) and are not in this feature's remit.
- Keyboard-shortcut surface beyond undo/redo.
- Collaborative or multi-tab editing; phase 1 is single-operator with no SSE.

## Notes

Two of this spec's grounded claims contradict the epic doc as originally written — there was no pipeline route, and the UI had none of its named libraries. Both were caught by grounding against code rather than against the phase doc, which is why the write-spec procedure treats the phase doc as untrusted. The epic doc was amended in the same session (in-scope items 2 and 3).
