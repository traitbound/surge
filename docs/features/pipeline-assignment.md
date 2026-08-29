# Feature: pipeline-assignment

**Status:** draft
**Phase:** phase-1.1
**Owner:** solo
**Last updated:** 2026-08-29

> **Spec depth note (2026-08-29).** This spec deliberately states *contracts* — acceptance criteria, wire shapes, invariants touched, seams — and not *mechanism*. It prescribes no store-function signatures, no SQL shapes, no module layout and no migration recipe. Those are settled against the compiler and the schema by the implementer, under the fresh-reviewer gate. Earlier drafts in this epic prescribed mechanism in prose and were wrong about it three times.

## Summary

Which pipeline version a project runs. The schema column, the domain type and three UI sites that render it already exist; the write does not, and `projects::insert` actively refuses one. This feature closes that gap, gives the operator surfaces to use it from, and makes the stored fact consequential by letting compile default to it.

**It does not add a state to `PipelineAssignmentStatus`.** An earlier draft added `Unassigned`. That was wrong: dispatch gates on materialization freshness alone and never reads assignment, so an assignment-gated badge would assert a dispatch verdict the supervisor contradicts — the defect ESC-3 removed, one level up. Status keeps meaning **dispatchability**; "unassigned" is already on the wire as `assigned_pipeline === null`.

## User-facing behaviour

An operator assigns a published pipeline version to a project, from the CLI or the API. The card then shows `name · vN` instead of "not assigned", the switcher shows `vN`, and the compile dialog opens pre-filled with that pipeline rather than a hardcoded fixture id. Compiling without naming a pipeline compiles the assigned one.

Assigning a pipeline the project was not already compiled against stales its materialization — the graph it runs has moved since its last compile — so the card returns to "not compiled" and dispatch is refused until recompile. Where a fresh materialization was nonetheless built from a different pipeline than the one assigned, the project says so rather than implying they match.

Assigning something that does not exist is refused by name, recorded, and leaves the previous assignment standing.

## Acceptance criteria

1. Assigning a pipeline version to a project records it, and both project read paths return it as `assigned_pipeline`; a project with none returns `null`.
2. An assignment and its audit entry (`project.pipeline_assigned`) are atomic — neither can be observed without the other (INV-OBS-1, INV-DATA-8).
3. Assigning with an unknown pipeline id, or against an unknown project, is refused with a named error **and an audit record**, and leaves all project state unchanged (INV-ERR-1).
4. **Staling predicate:** an assignment write stales the project's fresh materialization *unless* that materialization was compiled from the pipeline being assigned. Re-assigning the pipeline already assigned is a no-op — no state change, no audit row. Consequence: assigning to a project whose fresh materialization already matches does **not** knock it out of `published`.
5. Compile with no pipeline id compiles the assigned pipeline; with an explicit id, that one; with neither an assignment nor an id, it is refused with a visible record naming what is absent (INV-ERR-1).
6. Both project read paths report `compiled_from` — the pipeline the fresh materialization was built from, `null` when there is none — and it is correct when it differs from `assigned_pipeline`. Where they differ, the project detail names both.
7. Every hook in the smoke checklist is reachable from a shipped surface: a **second seeded pipeline** so two versions exist to move between, `surge assign <project> <pipeline>`, and `GET /api/pipelines`.

## Component design

Only the decisions an implementer should not have to re-derive. Mechanism is theirs.

**Three facts, three fields.** `pipeline_status` answers "will dispatch proceed" and is derived from the same predicate dispatch uses — unchanged from ESC-3. `assigned_pipeline` answers "what is this project meant to run". `compiled_from` answers "what was actually compiled". Collapsing any two into one field is what produced the rejected `Unassigned` variant; they vary independently and all three are producible today.

**`compiled_from` is a reference, not an assignment.** It must not reuse the `AssignedPipeline` type name for a value that is not an assignment (INV-NAME-1). A neutral shared shape carrying pipeline id, name, version and content hash serves both fields.

**Multiplicity hazard, stated because it is easy to miss:** nothing in the schema enforces one fresh materialization per project — only `cache_key` is unique, and the freshness discipline lives in a repository function that `phase.md`'s known-defects table already records as falsifiable. Whatever expresses `compiled_from` must be immune to two fresh rows producing two projects.

**Assignment does not gate dispatch.** The supervisor decides on freshness alone, and this feature does not change that. AC 4's staling therefore forces a recompile; it does not guarantee the running graph is the assigned one. That is why AC 6 exists — the honest response to a divergence the system permits is to name it, not to claim it cannot happen.

**Badge slot is not this feature's.** `registry.tsx`'s badge is a binary ternary already carrying a known precedence defect assigned to `project-overview` (1.3). This feature adds no contender to it: the assignment/compile mismatch of AC 6 is named in the project's **pipeline line**, beside `name · vN`, not in the badge.

## Artefact verdicts

- Sequence diagram: **skip** — two actors, synchronous, one service, no async coordination or compensation.
- Component design: **include** — the three-facts decision and the staling predicate are the two things a reasonable implementer would get wrong, and one earlier draft did.
- User flow: **skip** — no new UI surface. Three existing sites change, enumerated below; their state design belongs to `project-overview` (1.3).

## Non-goals

- The assign **picker dialog** and the pipelines page — 1.3. Operator access here is the CLI and the API.
- Project-local revisions, forking, promote-to-fork, history, diff.
- Making `Stale` producible — status stays two-valued; `Stale` returns with `pipeline-revisions` (§23-Twenty-Two).
- Refusing an explicit-id compile that differs from the assignment. Trying a pipeline before assigning it is a real capability; AC 6 makes the result visible rather than illegal.
- Making assignment gate dispatch.
- Resolving the badge-slot precedence (`project-overview`, 1.3).
- Any change to `pipeline_content_hash` or where it lives.

## Touches

- **INV-ERR-1** — four refusal paths (unknown pipeline, unknown project, compile with nothing to compile, dispatch after staling), each producing a visible record with its reason.
- **INV-OBS-1** — assignment is a privileged act and writes an audit entry. *Note: the invariant's enumerated list does not currently contain assignment; see Promotion candidates.*
- **INV-DATA-8** — the audit entry and the staling commit with the assignment (AC 2).
- **INV-ID-1** — AC 4 stales so dispatch is refused until recompile; AC 6 keeps the displayed graph honest about what was compiled.
- **INV-ID-3** — "a project is never described as running plain `vN` while local edits exist". This feature ships the **two-field form** (`assigned_pipeline` + `compiled_from`) at the three sites below. Widening them to `vN + local rev <hash>` is `pipeline-revisions`' obligation **on these exact sites**, recorded here so the seam has an owner.
- **INV-DATA-3** — assignment writes project state only; no pipeline row is created or mutated.
- **INV-AUTH-1/2** — new routes are human-token surfaces behind `require_human`; runtime tokens are refused loudly and audited. INV-AUTH-1's runtime "fetch pipeline" capability is a worker fetching the pipeline for its own run, not enumerating all pipelines, so the list endpoint stays human-only.
- **INV-NAME-1** — operator copy for the unassigned state must be one string, not three. Today: `"not assigned"` (card), `"—"` (switcher). Pick one and use it in both.

## Events

- Written: `project.pipeline_assigned`, `project.assign_refused` (unknown pipeline **and** unknown project), and the existing `compile.refused` with a new subject for AC 5's no-target case. Subjects follow the `"{thing} — {reason}"` shape `phase.md`'s unification row prescribes.
- Consumed: none. No SSE in phase 1.

## Environment variables

| Var | Purpose | Arg type | Where set |
|---|---|---|---|
| — | none introduced | — | — |

## Wire-format contract

**`POST /api/projects/{id}/assign`** — request `{ "pipeline_id": string }`; response `200` with the updated project, **read back** rather than constructed, so the caller sees derived status (the shape ESC-3 established on the create path). Refusals `404 { "error": … }`, each audited.

**`GET /api/pipelines`** — response is the existing ts-rs `Pipeline[]`, not a hand-shaped DTO (`ui/src/api.ts:1-4`: never hand-write shapes the server already defines). It carries `id`, `name`, `version`, `content_hash`, `blessed`, `forked_from`, `created_at`; the operator surfaces use the first five.

**`POST /api/projects/{id}/compile`** — `pipeline_id` becomes optional.

| Field | Rust type | JSON / TS | Notes |
|---|---|---|---|
| `Project.assigned_pipeline` | `Option<…>` | `… \| null` | `null` **is** the unassigned state — the reason no enum variant is added |
| `Project.compiled_from` | `Option<…>` | `… \| null` | new; `null` when no fresh materialization exists |
| `…​.version` | `i64` | `bigint` (ts-rs) | serde emits a JSON number; the `bigint` is type-level only (`ui/src/api.ts:87-89`). Feeds no hash |
| `…​.content_hash` | `String` | `string` | opaque, equality only; a real computed hash since ESC-1 |
| `CompileBody.pipeline_id` | `Option<String>` | optional | backward-compatible: both shipped callers always send it |

`PipelineAssignmentStatus` is **unchanged** — no new variant, so no UI fall-through and no ESC-3 test flips.

## Depends on

- `domain-model`, `store-layer`, `cli-thin` (phase-0).
- ESC-3 (`1eb48dc`) — the derived-status read path this extends without adding a variant.
- ESC-1 (`3ecd585`) — `content_hash` is real, so what AC 1 and AC 7 surface means something.
- ESC-2 (`271ccdc`) — the refusal-record shape AC 3 and AC 5 follow.
- **Nothing in phase-1.** `canvas-editor` also lists `GET /api/pipelines`; see the seam note below.

**Seam with `canvas-editor`:** that spec (currently a stale draft) claims the same list endpoint and a pipeline API module. **This feature owns `GET /api/pipelines`**, because it needs it first and `canvas-editor` is not written yet; `canvas-editor` consumes it and owns the graph read/write routes beside it. Its rewrite must record that.

## Approach

A sketch, not a plan. The taskgraph decomposes it.

1. Second seeded fixture pipeline, so two versions exist.
2. A `compiled_from` field on the project read model; regenerate ts-rs.
3. Assignment write, its refusals, and the staling predicate — atomic per AC 2.
4. Read paths return `assigned_pipeline` and `compiled_from`.
5. Assign and list routes; compile's optional target and resolution order.
6. `surge assign` subcommand, matching the existing CLI shape.
7. UI: drop the fixture fallback in the compile dialog; render the mismatch on the pipeline line; unify the unassigned string.

## Grounded claims

| Claim | Anchor | Verified how |
|---|---|---|
| **Dispatch never reads assignment** | `crates/server/src/supervisor.rs:317`, `:508`, `:1102` | all three gates call `fresh_for_project`; `grep assigned_pipeline` in that file returns **zero**. Why no status variant is added |
| A never-assigned project compiles and reports `published` today | `crates/server/tests/compile_endpoint.rs:139-143` | ESC-3's pin: "a fresh materialization exists → the card must stop warning" |
| **Exactly one pipeline exists in the system** | `crates/server/src/lib.rs:156-161`; `crates/server/src/human_api.rs:17-34` | the seed inserts one fixture graph, and no route creates a pipeline. AC 7's second fixture exists because of this — without it ACs 4 and 6 have no second version to name |
| `projects::insert` actively refuses an assignment | `crates/store/src/projects.rs:21-24` | `ensure!(p.assigned_pipeline.is_none(), …)`. **Retained** — no caller constructs a project with one (`human_api.rs:84-96` hardcodes `None`; `CreateProject` has no such field), so removing it would add no capability and would let a project be born assigned with no audit row |
| The read path hardcodes `None` and names this task | `crates/store/src/projects.rs:69`, `:88` | "Assignment modelling lands with the compiler/assignment tasks" |
| Status is derived, from the same predicate dispatch uses | `crates/store/src/projects.rs:5-8`, `:111`, `:137` | ESC-3's `EXISTS(… fresh = 1)` in both read queries |
| **Three** UI sites consume assignment — no fourth | `ui/src/registry.tsx:139-140`, `:273-275`, `ui/src/shell.tsx:115` | grep over `ui/`, `crates/`, `integrations/` returns these three only |
| The badge ternary is binary and its else-branch renders the bind badge | `ui/src/registry.tsx:259-266` | why AC 6's mismatch goes on the pipeline line, not the badge |
| Compile takes its target from the body, never the assignment | `crates/server/src/compile_api.rs:19-21`, `:44` | `CompileBody { pipeline_id: String }` then `load_graph(…, &body.pipeline_id)` |
| `compile_api`'s unknown-pipeline 404 writes **no** audit row | `crates/server/src/compile_api.rs:44-49` vs `:69-71` | the sibling `compile.refused` does. The pattern to avoid |
| Both shipped compile callers always send `pipeline_id` | `ui/src/api.ts:52-60`; `crates/cli/src/main.rs:47`, `:307` | UI always posts it; CLI declares it a required positional. So AC 5's no-id path needs a caller — the CLI's compile argument becomes optional |
| Nothing enforces one fresh materialization per project | `crates/store/migrations/0002_object_model.sql:82-91`, `:187` | only `cache_key` is `UNIQUE`; the sole index is `span_by_run`. The multiplicity hazard in Component design |
| Foreign keys are enforced, so an existence check is necessary not cosmetic | `crates/store/src/lib.rs:33`, `:44` | `.foreign_keys(true)` on both pool constructors |
| The enum's seam note deferred "unassigned" here | `crates/domain/src/project.rs:38-48` | answered by declining to add a variant, with the reason above |

## Constraint blast radius

**New constraint: assignment stales unless the materialization already matches (AC 4).**

- *Protects:* a project whose assigned graph moved is not left reporting `published` against a materialization built from something else.
- *Blocks:* flipping between two pipelines without a recompile each way — except in the matching case the predicate exempts. It does **not** guarantee the running graph is the assigned one, because dispatch reads freshness alone; an explicit-id compile restores dispatchability immediately. Stated so nobody mistakes it for an enforcement.

**New constraint: `compiled_from` is reported whenever a fresh materialization exists.**

- *Protects:* the surface cannot imply the assignment was compiled when something else was.
- *Blocks:* nothing functionally; it adds a second per-project lookup on the list path, against a table with no index on `project_id` (`phase.md` follow-up row). Correct at single-user scale.

**Retained constraint: `projects::insert` refuses a constructed assignment.**

- *Protects:* a project cannot be born assigned without the audit entry AC 2 requires, and cannot assert derived fields the read path computes.
- *Blocks:* creating a project pre-assigned in one call. No caller wants that today; if one ever does, it must go through the assignment path so the audit row exists.

## Smoke checklist hooks

Reachable via `surge assign`, `surge compile` (target now optional), `GET /api/pipelines`, and the **two** seeded pipelines.

- Create a project; the card and the switcher both show the unassigned string.
- `surge assign` the first pipeline; card reads `name · vN`, switcher `vN`, and `surge status --json` shows `project.pipeline_assigned` with a non-NULL `project_id`.
- Open the compile dialog; pre-filled with the assigned pipeline, not a fixture id.
- `surge compile` with no pipeline argument; the assigned pipeline compiles and the card moves to "published".
- `surge assign` the **second** pipeline; the card returns to "not compiled" and a dispatch attempt is refused with a visible record.
- `surge assign` the same pipeline again; nothing changes and no new audit row appears (AC 4's no-op).
- `surge compile` with an explicit id naming the **first** pipeline while the **second** is assigned; the project's pipeline line names both.
- `surge assign` a pipeline id that does not exist; refused by name, an audit row records it, previous assignment stands.
- `GET /api/pipelines` lists both seeded pipelines, and neither `content_hash` is `sha256:fixture-two-node-v1`.

**Walk precondition:** a `surge.db` created before ESC-1 carries the old placeholder `content_hash`; the seed's `exists()` guard skips it. Delete the dev DB before walking. This discharges the `phase.md` known-defects row whose trigger — "a phase-1.1 feature reads the column" — AC 1 and AC 7 now meet; the row is annotated in the same change as this spec.

## Open questions

1. **Where should `pipeline_content_hash` live?** Deferred deliberately: this feature adds no hash writer. `pipeline-revisions` forces it. `phase.md`'s heading currently says "decide before `pipeline-assignment`"; it is re-pointed at `pipeline-revisions` in the same change as this spec.
2. **Should the second seeded fixture be blessed?** `blessed` has no writer, and `promote-to-fork` asserts a blessed base. Seeding one blessed would give that spec an exercisable base at no cost here. Proposed yes; confirm when `promote-to-fork` is written.
3. **Does an unassigned project with a non-null `compiled_from` count as a mismatch?** Reachable today and pinned green by `compile_endpoint.rs:139-143`. Proposed: no — with nothing assigned there is nothing to diverge from; the pipeline line simply names what was compiled.

## Out of scope

- Revisions, forking, blessing writes, history, diff.
- The assign picker and pipelines page (1.3).
- The badge-slot precedence defect (1.3).
- Multi-project or cross-project assignment.
- Any migration on the `project` table; ESC-3's parked column stays parked.

## Notes

Two earlier drafts of this spec failed in instructive ways. The first added an `Unassigned` status variant that would have made the badge contradict the supervisor. The second prescribed a store-function signature that would not compile, a join that could duplicate rows, and smoke hooks needing a second pipeline that does not exist. The third — this one — states contracts and leaves mechanism to implementation, which is what the template asked for in the first place.
