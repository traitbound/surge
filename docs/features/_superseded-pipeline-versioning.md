# Feature: pipeline-versioning

**Status:** superseded — split into `pipeline-assignment`, `pipeline-revisions` and `promote-to-fork` on 2026-08-28. Kept because its Grounded claims table and two review punch lists are the research the successors are written from. **Do not implement from this file** — its Component design contains a retracted migration that would destroy every graph in the database, and its replace-never-mutate design is FK-blocked by `materialization.pipeline_id`.
**Phase:** phase-1.1
**Owner:** solo
**Last updated:** 2026-08-28

## Summary

The identity machinery every other phase-1 feature stands on: which pipeline version a project runs, what happens to identity the instant its graph is edited, and how a divergence becomes a fork without disturbing what it forked from. It exists as its own feature — and is spec'd before the canvas — because INV-ID-3 requires a project-local revision to appear *immediately* on edit, which makes the revision a prerequisite of the editor rather than a consequence of it.

Grounding established that three things this feature is usually assumed to have do not exist: there is no revision concept anywhere in the tree, `run` has no column for one, and **pipeline assignment is entirely unimplemented** — `project.assigned_pipeline_id` is in the schema and written by no code path. This feature builds all three.

## User-facing behaviour

A project is assigned a pipeline version and reports it: name, version, content hash. The moment the operator changes that project's graph, the assignment line stops claiming a bare version and starts reading `v14 + local rev 9c4e…` — there is no window in which the project displays `v14` while running something else. Further edits refine the same revision rather than accumulating new ones.

Promoting the revision to a fork produces a new pipeline at v1 whose provenance names the version it diverged from. The original is untouched: same hash, same graph, same history, and if it was blessed it stays blessed while the fork is not.

## Acceptance criteria

1. A pipeline version can be assigned to a project and read back — `pipeline_id`, `name`, `version`, `content_hash` — through `projects::get` and `projects::list`, both of which hardcode `None` today.
2. Moving **one** node with **no explicit save** causes the project to report `version + local_revision_hash`. This AC must fail against a canvas that batches edits behind a Save button — that is the whole point of it (INV-ID-3, "immediately").
3. Refining a revision **replaces** its graph in one transaction: after N mutations the project has exactly one live revision (`project.local_revision_id`), and a mutation whose transaction rolls back leaves neither a revision row nor a graph change.
4. Promote-to-fork creates a published pipeline at v1 whose node and edge **ids are preserved verbatim** from the revision, whose `content_hash` therefore equals the revision's, and whose `forked_from` names the base version row.
5. After any fork, the base version's row, graph and `content_hash` are byte-identical to before (INV-DATA-3); a blessed base stays `blessed = 1` and the fork is `blessed = 0`.
6. Creating or refining a revision marks the project's materialization **stale**, and dispatch is refused with a visible record until the revision is recompiled (INV-ID-1, INV-ERR-1).
7. A run dispatched against a compiled revision records that revision's hash in `run.pipeline_revision_hash`; a run against a plain published version records `NULL`.
8. Every pipeline row this feature writes carries `content_hash == pipeline_content_hash(its graph)`, and the seeded fixture's placeholder hash is replaced with a computed one.

## Component design

**A project-local revision is a `pipeline` row with a synthetic name — and it is never mutated.**

Two decisions, taken together, dissolve the migration risk the first draft carried:

*Synthetic name.* A revision row's `name` is `"<base name>@<project id>"`, not the base's name. The `(name, version)` collision that would have forced relaxing `UNIQUE (name, version)` (`crates/store/migrations/0002_object_model.sql:31`) therefore never occurs. **No table rebuild, no partial index, no `role:critical` migration on the pipeline table.** The first draft proposed the rebuild; it was wrong twice over — SQLite applies each sqlx migration inside a transaction, where `PRAGMA foreign_keys` is a documented no-op, so with `foreign_keys(true)` set on every connection (`crates/store/src/lib.rs:33`, `:44`) a `DROP TABLE pipeline` would have fired `ON DELETE CASCADE` on `node` and `edge` (`0002_object_model.sql:38`, `:55`) and silently destroyed every graph in the operator's database at boot.

*Replace, never refine.* A revision is not edited in place. Each mutation writes a **new** revision row and its graph, repoints `project.local_revision_id`, and deletes the superseded row (its graph goes with it by cascade) — all in one transaction. So no pipeline row is ever updated, `crates/store/src/pipelines.rs`'s "it never updates one" (`:1-3`) stays true, and INV-DATA-3 needs no exemption for unpublished rows. The first draft's `upsert_local_revision` adopted exactly the mutable-pipeline-row alternative the same section rejected.

The remaining alternatives are rejected as before: a `pipeline_revision` table with its own graph tables duplicates `node`/`edge` and forces `materialization.pipeline_id` (`0002_object_model.sql:86`) to become polymorphic, which SQLite foreign keys cannot express; a JSON blob on `project` makes the graph unqueryable and gives the compiler a second input path.

**Schema.** One additive migration, `0006_pipeline_local_revision.sql` — no rebuild, no constraint drop:

```sql
ALTER TABLE pipeline ADD COLUMN local_for_project TEXT REFERENCES project(id);
ALTER TABLE project  ADD COLUMN local_revision_id TEXT REFERENCES pipeline(id);
ALTER TABLE run      ADD COLUMN pipeline_revision_hash TEXT;
```

`run.pipeline_revision_hash` exists because INV-ID-3 says runs **record** the revision hash. Transitive reachability through the materialization is not recording, and would in any case join through `materialization.content_hash`, which is neither unique nor indexed — only `cache_key` is (`0002_object_model.sql:85`).

**Staleness.** Creating a revision means the project's graph has moved since its last compile, which is precisely what `PipelineAssignmentStatus::Stale` documents itself as: *"The pipeline moved since the last compile — dispatch refused (INV-ID-1)"* (`crates/domain/src/project.rs:37`). The first draft claimed the field was about materialization freshness only; the field's own doc comment says otherwise. So a revision write marks the project's materialization `fresh = 0` in the same transaction. Without this, dispatch keeps running the published graph while the operator edits a different one.

**Compiling a revision.** `POST /api/projects/{id}/compile` takes `pipeline_id` from its request body (`crates/server/src/compile_api.rs:19-21`) and never consults the assignment. This feature adds: when a project has a live revision, compile targets the revision unless a `pipeline_id` is given explicitly. That is what makes AC 6 and AC 7 reachable at all.

**Store.** New typed repository functions, each compile-checked with an in-memory test:

- `projects::assign(tx, project_id, pipeline_id)` — and the `ensure!` at `crates/store/src/projects.rs:9-12` that **actively refuses** any project carrying an assignment ("assignment lands with the compiler task") is removed, along with the hardcoded `assigned_pipeline: None` in `project_from` (`:73`) that feeds both `get` (`:90`) and `list` (`:114`).
- `pipelines::write_local_revision(tx, project_id, base_pipeline_id, nodes, edges) -> Pipeline` — writes the new revision, repoints `local_revision_id`, deletes the superseded revision, stales the materialization.
- `pipelines::live_revision(pool, project_id) -> Option<Pipeline>`
- `pipelines::promote_to_fork(tx, project_id, revision_id, new_name) -> Pipeline`
- `pipelines::list_published(pool) -> Vec<Pipeline>` — `canvas-editor` needs it; it does not exist.

`crates/store/src/pipelines.rs`'s module header is amended in the same change to describe the revision row class.

**Domain.** `AssignedPipeline` gains `local_revision_hash: Option<String>`. `Pipeline` is **not** amended — `local_for_project` is a DB-only column, never a `#[derive(TS)]` field, so it cannot leak onto the wire and render as a phantom version in a list.

**API.** `POST /api/projects/{id}/assign`, `POST /api/projects/{id}/promote-fork`, and the graph-mutation route is **`POST /api/projects/{id}/graph`** — project-scoped, not `/api/pipelines/{id}/graph`. A pipeline id does not determine a project (two projects may be assigned the same version), so a pipeline-keyed route cannot resolve which project's revision to write. `canvas-editor` must adopt this shape.

**Obligation on every other feature:** no graph-write path may touch an assigned project's graph except `write_local_revision`. A second writer reintroduces the unnamed-divergence window INV-ID-3 exists to close.

## Artefact verdicts

- Sequence diagram: **skip** — two actors, synchronous, single service. The one ordering property that matters (revision created in the same transaction as the mutation) is a transaction boundary, stated in the component design and asserted by AC 2; a diagram would restate it less precisely.
- Component design: **include** — the whole feature is a data-model decision, and the "a revision is a pipeline row" choice needs its rejected alternatives on the record. A competent engineer would otherwise reasonably build the separate-table version and inherit its polymorphic-FK problem.
- User flow: **skip** — no surface of its own. The assignment line renders in `project-overview` (1.3); the fork affordance is `canvas-editor`'s. This feature is the machinery beneath both.

## Non-goals

- Version **history and diff** surfaces — 1.3 (`pipelines-pages`).
- The assign **dialog** — 1.3. This feature builds the assignment data path and endpoint; the picker UI is a page concern.
- Deciding *when* to promote. Promote-to-fork is operator-initiated; nothing auto-promotes.
- Library-item version bumping — that is `upgrade-review` (1.2) and moves a pin, not a pipeline lineage.
- Merge, rebase or three-way reconciliation between a revision and a moved base. If the base version moves under a live revision, that is Open question 2.
- Multi-project revisions: a revision belongs to exactly one project by construction.

## Touches

- **INV-DATA-3** — a published version is immutable; AC 5 is this feature's proof. Revisions are a *different row class*, never a mutation of a published row.
- **INV-ID-3** — the reason this feature exists and is spec'd first; AC 2 and AC 6 are its two halves.
- **INV-ID-2** — a revision's `content_hash` is computed by the same `pipeline_content_hash` as any other graph; a revision is not a special hash, only a special row. **Node and edge ids are hash inputs** under the 2026-08-28 amendment, so `promote_to_fork` must copy them verbatim (AC 4). An implementer minting fresh ids for the fork — the natural instinct when a new `pipeline_id` appears — breaks hash equality with no compile error and no obvious symptom.
- **INV-ID-1** — run → graph traceability; AC 6 keeps it landing on a named graph.
- **INV-OBS-1** — assignment, promote-to-fork and revision creation are privileged acts and write audit entries.
- **INV-DATA-8** — those audit entries commit in the same transaction as the act (the pattern `5331cda` established).
- **INV-ID-1** — a stale materialization refuses dispatch; AC 6 is this feature's slice. Creating a revision is what makes a materialization stale.
- **INV-ERR-1** — the refused dispatch in AC 6 produces a visible record carrying the reason, not a silent no-op.
- **INV-NAME-1** — this feature mints the operator-visible noun **local revision** and the display form `v14 + local rev 9c4e…`. It is a distinct concept from `work_order.revision` (`crates/domain/src/board.rs:93`), a per-issue counter; UI copy must not blur them.

## Events

- Written: audit entries `pipeline.assigned`, `pipeline.revision_created`, `pipeline.promoted_to_fork` — each carrying project, base version and resulting hash, written in the acting transaction (INV-DATA-8).
- Consumed: none. No SSE in phase 1.

## Environment variables

| Var | Purpose | Arg type (build-arg / runtime) | Where set |
|---|---|---|---|
| — | none introduced | — | — |

## Wire-format contract

| Field | Rust type | JSON / TS | Who transforms | Notes |
|---|---|---|---|---|
| `AssignedPipeline.version` | `i64` | `bigint` (ts-rs) | ts-rs | serde emits a JSON **number**; `JSON.parse` yields a JS `number`. The `bigint` is a type-level artefact — see `ui/src/api.ts:87-89`. Coerce for display with the existing `ms`/`Number` pattern. (`version` feeds no hash — `pipeline_content_hash` takes nodes and edges only — so the usual hashed-field caution does not apply here) |
| `AssignedPipeline.local_revision_hash` | `Option<String>` | `string \| null` | none | **`null` is the meaningful state**, not `""` — it is how a caller distinguishes "no local edits" from "revision with an empty hash", which cannot occur |
| `Pipeline.content_hash` | `String` | `string` | none | opaque `sha256:…`; never parsed, compared only for equality |
| `Pipeline.forked_from` | `Option<String>` | `string \| null` | none | a pipeline **id**, not a name+version pair — resolving it to a display label is a page concern (1.3) |
| `Pipeline.local_for_project` | — | **not on the wire** | — | DB-only column; deliberately **not** a `#[derive(TS)]` field, so a revision cannot leak into a list UI as a phantom version. `list_published` filters on it server-side |

## Depends on

- `domain-model` (phase-0) — `Pipeline`, `AssignedPipeline`, `Project` exist with ts-rs derives; `forked_from` and `blessed` are already columns.
- `store-layer` (phase-0) — `insert_graph`/`load_graph` and the migration-at-startup path (ADR-9) this feature adds to.
- `compiler-core` (phase-0) — `pipeline_content_hash` computes a revision's hash exactly as it computes a version's.
- **Nothing in phase-1.** This feature is the epic's dependency root; `canvas-editor` depends on it, not the reverse.

## Approach

1. Migration `0006` — the `pipeline` table rebuild that replaces table-level `UNIQUE (name, version)` with the partial index, plus the two new columns. Run the CLAUDE.md schema-change loop and commit `.sqlx/` in the same change.
2. `AssignedPipeline.local_revision_hash`; regenerate ts-rs.
3. Store functions above, each with an in-memory test.
4. Assign + promote-fork endpoints on the human router.
5. Audit writes inside each acting transaction.
6. The AC 2 atomicity test: a mutation whose transaction rolls back leaves **no** revision row and **no** graph change.

## Grounded claims

| Claim | Anchor | Verified how |
|---|---|---|
| **Pipeline assignment is written by no code path** | `crates/store/src/projects.rs:21-23` | the `INSERT INTO project` column list omits `assigned_pipeline_id`; `grep -rn assigned_pipeline_id crates/ --include=*.rs` returns **zero** hits. Explains walk-6/7's B2 ("not assigned" after compile) |
| No revision concept exists anywhere | `grep -rn revision crates/` | every hit is `work_order.revision` (`crates/domain/src/board.rs:93`, `migrations/0002_object_model.sql:146`), an unrelated per-issue counter |
| `run` has no column for a revision | `crates/store/migrations/0002_object_model.sql` run DDL | columns are id, project_id, issue_id, kind, materialization_hash, work_order_hash, status, started_at, ended_at, cost — INV-ID-3's "runs record the local revision hash" is unimplemented and unschema'd |
| Traceability already reaches a graph through materialization | `0002_object_model.sql` materialization DDL | `materialization.pipeline_id REFERENCES pipeline(id)` — so a revision-as-pipeline-row is reachable from a run with no `run` change |
| `node`/`edge` are keyed by pipeline id with cascade delete | `0002_object_model.sql:49,60` and `:38,55` | `PRIMARY KEY (pipeline_id, id)`, `REFERENCES pipeline(id) ON DELETE CASCADE` (`:38`, `:55`) — a revision row gets graph storage and cleanup for free |
| `UNIQUE (name, version)` is declared inline on the table | `0002_object_model.sql:31` | table-level constraint; SQLite has no `DROP CONSTRAINT`, so relaxing it is a table rebuild, not an `ALTER` |
| The store has no update, no list and no fork path | `crates/store/src/pipelines.rs` | exactly four public functions: `insert_graph:11`, `exists:76`, `load_graph:84`, `reachable_nodes:155`. The module header states it "never updates one" |
| `pipeline_status` is only ever `Published` in code | `grep PipelineAssignmentStatus::` | every construction site is `Published` (`human_api.rs:89` and seven test files); nothing computes `Stale` |
| `blessed` and `forked_from` already exist | `crates/domain/src/pipeline.rs:20,22`, `0002_object_model.sql:28-29` | both are columns and domain fields today; this feature gives them their first writer |
| A revision's hash needs no new hashing code | `crates/compiler/src/hash.rs:82` | `pipeline_content_hash(nodes, edges)` takes a graph, not a pipeline row — it is indifferent to row class |
| **`projects::insert` actively refuses an assignment** | `crates/store/src/projects.rs:9-12` | `ensure!(p.assigned_pipeline.is_none(), "assignment lands with the compiler task")` — a deliberate phase-0 tripwire, not merely a missing write. `project_from:73` also hardcodes `None`, feeding `get:90` and `list:114` |
| **`pipeline.content_hash` is an unmaintained placeholder** | `crates/domain/src/fixtures.rs:15-16` | the only pipeline row in the tree carries the literal `"sha256:fixture-two-node-v1"` with the comment "Real content hashing lands with the compiler task"; `crates/compiler/src/lib.rs:119` computes the real hash and never writes it back. AC 8 fixes this |
| Compile never consults the assignment | `crates/server/src/compile_api.rs:19-21` | `CompileBody { pipeline_id: String }` — the target comes from the request body, so nothing would ever compile a revision without AC 6's change |
| Migrations run at boot with foreign keys enforced | `crates/store/src/lib.rs:33`, `:36`, `:44` | `.foreign_keys(true)` on both pool constructors, `MIGRATOR.run(&pool)` at startup — which is why the first draft's `DROP TABLE pipeline` rebuild would have cascaded away every `node` and `edge` row |
| `materialization.content_hash` is not unique and not indexed | `crates/store/migrations/0002_object_model.sql:85` | only `cache_key` carries `UNIQUE`; so a run→materialization join on that column is not a sound traceability path, which is why AC 7 adds a column instead |
| `PipelineAssignmentStatus::Stale` already means "the graph moved" | `crates/domain/src/project.rs:37` | doc comment: "The pipeline moved since the last compile — dispatch refused (INV-ID-1)" — a live revision is exactly that |
| `i64` → TS `bigint` is type-level only; the wire carries a number | `ui/src/api.ts:87-89` | "Timestamps cross the wire as JSON numbers; ts-rs types them `bigint`" |

## Constraint blast radius

**New constraint: commit-on-mutation — there is no client-side unsaved graph buffer.**

- *Protects:* INV-ID-3's "immediately". Any buffer leaves a window where the project reports a bare `vN` while the operator is looking at a different graph — the unnamed divergence the invariant exists to close.
- *Blocks:* real and valuable canvas affordances, and they should be named rather than discovered. It forbids a Save button, a local undo stack over unsaved state, navigate-away "you have unsaved changes" prompts, and offline editing. `canvas-editor`'s current draft specifies three of those four (`docs/features/canvas-editor.md:24`, `:46`, `:55-56`) and must be rewritten against this. Undo becomes a server round-trip per step, which is a genuine latency cost on a graph editor.

**New constraint: only `write_local_revision` may write an assigned project's graph.**

- *Protects:* the single-writer property AC 2 and AC 3 rest on.
- *Blocks:* any future bulk-import, template-apply or migration path that wants to write a graph directly. Each must route through this function or explicitly detach the assignment first.

**New constraint: `local_for_project REFERENCES project(id)`.**

- *Protects:* revisions cannot outlive their project as unowned graphs.
- *Blocks:* deleting a project that has ever been revised, unless the delete cascades or clears revisions first. No project-delete path exists today; whoever writes one inherits this.

**Removed constraint: `projects::insert`'s `ensure!(p.assigned_pipeline.is_none())`.**

- *Protected:* a phase-0 invariant that assignment was not yet implemented — a deliberate tripwire ("assignment lands with the compiler task", `crates/store/src/projects.rs:9-12`).
- *Unblocking it* means every existing caller that relied on being handed `None` must be checked: `human_api.rs:88` constructs projects, and eight test files construct `Project` literals with `assigned_pipeline: None`. None breaks, but the tripwire's removal is the moment assignment becomes real, so it is called out rather than deleted quietly.

## Smoke checklist hooks

- Assign a pipeline to a project; the project reads back name, version and hash — where today `projects::insert` would refuse the assignment outright.
- Move one node and perform **no** save; the project immediately reports `vN + local rev <hash>`, and the DB shows exactly one revision row.
- Move two more nodes; still exactly one live revision, pointed at by `project.local_revision_id`, with a hash that changed and no orphaned superseded rows.
- Attempt to dispatch while a revision is live and un-recompiled: refused, with a visible refusal record naming staleness.
- Recompile, then dispatch: the run's `pipeline_revision_hash` equals the live revision's hash.
- Promote to fork: the fork is v1, its node and edge ids match the revision's exactly, its `content_hash` equals the revision's, and `forked_from` names the base. Re-reading the base shows an unchanged `content_hash` and node/edge count.
- Fork the seeded blessed template: base stays `blessed = 1`, fork is `blessed = 0`.

## Open questions

1. **What happens when the base version moves under a live revision?** If `flow v3` is superseded by `v4` while a project holds a revision of `v3`, the revision stays pinned to `v3`. No merge semantics exist and none are in scope; the divergence surfaces in 1.3's history view. `upgrade-review` (1.2) is the analogous decision for library pins and may be the pattern to copy.
2. **Should promote-to-fork require a unique published name?** AC 4 takes a `new_name`. Forking `flow` into `flow` produces two lineages sharing a display name — permitted by the schema, confusing in a list. A uniqueness rule on published names may be a small new constraint; resolve at task time.
3. **Does the audit envelope fit what Events promises?** `audit_entry` is `action · subject · actor · project_id · at` with a single `&str` subject (`crates/store/migrations/0002_object_model.sql:200-207`, `crates/store/src/audit.rs:15-24`), but this feature wants to record project, base version and resulting hash. Proposed: format the subject as `<base pipeline id> → <resulting hash>` and rely on `project_id` for the third. Flagged rather than assumed, because a structured-audit change is a schema decision beyond this feature.
4. **Is `forked_from` naming the base version enough to satisfy "promote-to-fork names it"?** INV-ID-3 and design §23-Nine say the fork's provenance names the *local revision*. AC 4 makes `forked_from` point at the base **version**, and ties the fork to the revision by hash equality plus the `pipeline.promoted_to_fork` audit entry — the revision row itself is deleted on promotion. If a durable revision→fork link is wanted in the schema, that is a fifth column and should be decided before implementation.

## Out of scope

- Any UI. The assignment line, history, diff and fork affordance all render elsewhere (1.3, `canvas-editor`).
- Changing `pipeline_content_hash`. Hash-input changes are `role:critical` and belong to nothing in this epic.
- Retention or compaction of superseded revisions.
- Cross-project pipeline sharing beyond assignment.

## Notes

Spec'd before `canvas-editor` because the sprint's first grounding pass established that INV-ID-3's "immediately" forbids a browser-side unsaved buffer, which makes the revision a prerequisite of the editor. The epic doc records the inverted order.

This spec was rewritten after its first review drew 16 blockers. Three findings changed the design rather than the prose: the revision row needed a synthetic name (which dissolved a `role:critical` table rebuild that would have destroyed every graph in the operator's database at boot), refinement had to replace rather than mutate (the first draft adopted, in its acceptance criteria, the very alternative its component design rejected), and `run` needed a real column because reachability is not recording.

The assignment finding is the kind that only grounding surfaces: `AssignedPipeline` exists as a domain type, is rendered by the UI, appears in generated TypeScript, and has a schema column — and nothing has ever written it. Every artefact suggested the feature existed except the code.
