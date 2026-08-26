# Surge — Complete Product Spec · V3 · Page by Page

> COMPLETE PRODUCT SPECIFICATION · V3 · DERIVED FROM THE V9 PROTOTYPE

One local service that owns projects, authorable delivery pipelines, a shared library of hooks, subagents and skills, planning boards, and run observability. The IDE runtime — Claude Code, Cursor, Codex — becomes a thin client that fetches its compiled pipeline at session start instead of approximating an engine with loose files.

This document describes every surface, every panel, every control and every state currently designed, page by page, in the order a person meets them. Where the prototype is a stub rather than a decision, it is called out in §23.

`127.0.0.1:7420 · local` · `10 top-level surfaces` · `7 dialogs` · `single-user · token-scoped`

> **The bet, in one paragraph**
>
> An authorable pipeline canvas, an observatory wired into every node of it, and a first-party project and board system — paired. No half is novel alone. Together they mean the graph a human edits, the files a runtime executes, the issues a team tracks and the spans an operator reads are the same object seen from four angles, and every jump between them is one click.

## Contents

| # | Section | # | Section |
| --- | --- | --- | --- |
| 01 | Frame & vocabulary | 13 | Project · Docs |
| 02 | Navigation map | 14 | Project · Board (Plan) |
| 03 | Object model | 15 | Project · Board (Ops) |
| 04 | Trust & capability | 16 | Project · Observatory |
| 05 | Versioning & change | 17 | Settings (both levels) |
| 06 | Execution lifecycle | 18 | Dialogs & overlays |
| 07 | Page: global shell | 19 | Critical flows |
| 08 | Page: Registry | 20 | Empty & degraded states |
| 09 | Page: Pipelines | 21 | System copy inventory |
| 10 | Page: Library | 22 | Keyboard & cross-links |
| 11 | Page: Pipeline editor | 23 | Fidelity & open questions |
| 12 | Project · Overview | | |

---

## 01 — Frame & vocabulary

Surge runs as one local service on one port, for one operator. Everything below is a view onto its database plus the repos it is bound to. Terminology is precise and never softened: a run is a run, a span is a span, a work order is a work order.

| TERM IN UI | WHAT IT MEANS, AND WHAT IT REPLACED |
| --- | --- |
| **Pipeline** | The authored graph of nodes and edges. Internally still the *harness*; the word "harness" survives only in developer-facing strings and in fork provenance lines. |
| **Library** | The cross-project store of hooks, subagents and skills. Three tabs, one surface. |
| **Materialization** | The compiled output of a pipeline for one project — the real files a runtime reads. Identified by a hash and a cache key. |
| **Work order** | The versioned instruction for exactly one issue. Lives in Surge, is rendered to the runtime as a file under `work_orders/`. |
| **Board · Plan** | Human planning — releases, sprints, issues — mirrored read-only from the repo's tracker. |
| **Board · Ops** | Machine execution — work orders, gates, leases, dispatch. Surge owns this half outright. |
| **Observatory** | Runs, spans, cause-of-error records and the policy & audit log for one project. |

> **Closed exception list — what may live in the code repo**
>
> Planning docs, pipelines, library items and boards live Surge-side. Only five things are written into the workplace repo (INV-DATA-1; the fourth added 2026-08-12 — §03 always required the file, the list had never been amended; the fifth, the surge-managed block inside the repo-root `.gitignore`, added 2026-08-26 — §23-Twelve/INV-DATA-7 always required the compiler to maintain it).
>
> 1. `surge.yaml` at the root — workspace binding, tracker choice, branch format, compiled step definitions. Never secrets.
> 2. The compiled runtime files a materialization produces — `.claude/settings.json`, `.claude/agents/*.md`, `.claude/skills/*/SKILL.md`, `.claude/hooks/*.sh`.
> 3. The docs the pipeline itself writes, at the paths its doc nodes declare — `docs/research.md` through `docs/taskgraph.md`.
> 4. Rendered work-order files under `work_orders/` — one per dispatched issue, hash-checked against the issue on dispatch.
>
> Reads are equally closed (INV-DATA-6): declared doc paths (ingested and hashed after a doc node's run — the repo file is canonical, Surge's copy a projection), `work_orders/` for hash checks, and git state for wave integration. Nothing else in the repo is ever read.

---

## 02 — Navigation map

Two nav groups in one persistent sidebar. The top group is instance-level and always available. The bottom group is scoped to whichever project the switcher holds, and is always available too — picking a project never means leaving the instance surfaces.

**Instance** — top of the sidebar

- **Registry** — every bound project, plus the "Needs you" queue
- **Pipelines** — blessed templates, forks, by-project; detail → builder
- **Library** — Hooks · Subagents · Skills
- **Settings** — appearance, subagent roster, tokens, credentials, backup

**Project** — below the switcher card

- **Overview** — binding, assignment, subagents, hooks, docs, recent runs
- **Docs** — the doc chain and its gates, with a reading drawer
- **Board** — Plan (mirrored) | Ops (owned)
- **Pipeline** — this project's materialized graph, editable
- **Observatory** — runs, spans, COEs, audit log, metrics
- **Settings** — binding, tracker, branch format, routing, egress, hooks

### Drill-downs — reachable, not in the nav

- Pipelines → pipeline detail → **builder** (template editing, diff overlay, no run overlay)
- Library → hook / subagent / skill detail (inline editing, publish, attachment list)
- Docs → doc drawer → **amendment dialog**
- Board · Ops → issue drawer (WorkOrder · Gate-2 · Orchestration)
- Board · Plan → issue drawer (git-synced fields + Surge planning fields)
- Observatory → run detail → span → **node in the pipeline** · replay dialog

> **One rule for the two canvases**
>
> The editor is one component with two scopes. Opened from **Pipelines → Open builder** it edits the template, and every project assigned to that pipeline picks the change up at its next materialization. Opened from **Project → Pipeline** it edits this project's materialized copy, and the change stays local until pushed to the template. The scope line under the title states which one you are in, and the toolbar changes: the builder gets a version-diff overlay, the project canvas gets a run overlay, a live debugger, and Edit template / Fork buttons.

---

## 03 — Object model

Twelve entities. Everything on every screen is one of these, or a projection of one.

> Counts of shipped fixtures below ("three projects ship", "six runs ship", "seven hooks…") describe the prototype's demo data, not product requirements. The shipped *default library* (hooks, subagents, skills backing the doc chain) is normative; fixture projects and runs are illustrative.

### Project — *registry card · switcher · every project surface*

A bound repo. Carries: name, repo path, assigned pipeline (name · version · hash), pipeline status (published | stale), `surge.yaml` state, tracker kind, branch format, last-run status and board health. Three ship in the prototype: **fleet-orchestrator** (Linear, healthy, 3 in-flight), **checkout-service** (GitHub, board clear), **ingest-pipeline** (built-in tracker, stale materialization, last dispatch refused).

### Pipeline — *Pipelines library · builder · project canvas*

A named, versioned graph. Carries version, content hash, blessed flag, fork provenance, version history, and the list of materializations it has produced. Four ship: coordinator-worker-verifier v14 (blessed, 2 projects), solo-implementer v3 (blessed), coordinator-worker-verifier — retry-heavy v2 (fork of v13, unassigned), research-only v5 (blessed, unassigned).

### Node — *six kinds*

Every node carries an id, label, position, a human-gate flag, a span flag, a metric binding and a note explaining why it is measured. Beyond that, per kind:

| Kind | Behaviour |
| --- | --- |
| `doc` | Writes one file. Has a subagent, an output path, and a skill reference. Compiles to a SKILL.md. |
| `agent` | Delegates to a subagent. Optional fanout for parallel runs. Compiles to an agent `.md`. |
| `hook` | Binds a library hook to an event, a matcher and a scope (session-wide or one step). Compiles to a settings.json block, a merged agent block, or a surge.yaml step block depending on scope. |
| `skill` | Invokes a library skill by name. Compiles to a SKILL.md. |
| `stage` | A deterministic shell command. Compiles to a surge.yaml step. |
| `block` | A composite: a named group of member nodes, collapsible, publishable to the palette, with exposed per-instance parameters. Compiles to its members. |

### Edge — *from · to · fires-on · gate*

A directed link with a trigger string — `doc_written`, `invariants_approved`, `taskgraph_approved`, `leased`, `submitted`, `passed`, `failed`, `scope`, `custom` — and a required-gate flag. A gated edge draws dashed in the accent colour and carries a lock button at its midpoint; unlocking it is a versioned, logged act. Edges whose source is a hook node are the hook's scope binding, and draw dotted.

### Library item — *Hook · Subagent · Skill*

Each is immutable per version. Editing one marks it a draft; publishing bumps the version. Pipelines reference a pinned version and do not move until bumped. Each carries a trust state — local, or imported-and-untrusted until a human marks it reviewed. Seven hooks, six subagents and seven skills ship by default.

### Materialization — *pipeline × project → compiled files*

Produced by Compile, identified by a hash and a cache key (`mk_a1b2..fleet`), signed by the instance, and either fresh or stale. Stale means the pipeline moved since the last compile: dispatch is refused until recompiled. Every run records the materialization hash it ran under, so a run can always be traced back to the exact graph that produced it.

### Doc — *one per doc node, chained*

A doc is not a free file — it is the output of a doc node, derived from the previous doc in the chain. It carries the node that wrote it, the subagent that ran, its parent doc, its gate state, who approved it and when. The default chain is Research → Spec → Invariants → Phase carve → Feature design → Cross-spec audit → Taskgraph. A doc whose parent changed after approval is badged **parent changed** and offers a re-derive; downstream docs keep working.

### Issue & WorkOrder — *Board · Ops*

Approving the taskgraph generates one issue per task, each backed by a work order file. An issue carries wave, parent phase, orchestration status, a work-order hash, a Gate-2 checklist, a lease owner, a retry count and a run history. Human-owned fields — disposition and priority — are stored *beside* orchestration status, never inside it. Wave integration issues (SUR-W1-INT, SUR-W2-INT) are ordinary issues that assemble a whole wave.

### Run & Span — *Observatory*

A dispatch emits a run; a run owns a tree of spans. A span records role (coordinator | worker | verifier), the node that emitted it, start, duration, status, cost, depth, and an optional policy decision string. Six runs ship across the three projects, including a refused two-second run whose single span carries the refusal reason.

### COE — *cause-of-error record + ratchet*

Attached to a run or an issue. Free text plus an optional *ratchet* — a concrete tightening suggested from the text (routing fallback, verifier criterion, guard hook, or a required gate) that can be applied against the next pipeline version.

### Audit entry — *one trail, project-scoped view*

Action, subject, actor, when. Written by: approvals, Gate-2 reviews, gate unlock and relock, imports, import reviews, publishes, compiles, aborts, token rotations, credential changes, egress edits, work-order revisions, debugger attachment, breakpoint state edits, node replays and applied ratchets.

### Plan issue — *mirrored, never written back*

A read-only projection of a tracker issue: number, title, labels, milestone, assignee, PR state, commits. Two fields are Surge's own and never sync outward: sprint and planning status. An optional link to the Surge work order closes the loop between the two halves of the board.

---

## 04 — Trust & capability

Two credentials, one boundary. Everything a machine may do is a subset of what a human may do, and the gap is enforced at the API, not in the UI.

| Human session · `st_9f…c41` | Runtime · `rt_4a…e77` (per project) |
| --- | --- |
| Full control. Approve, unlock a gate, mark Gate-2 reviewed, review an import, compile, rotate tokens, abort. Bound to one browser; rotating signs the others out. | Fetch pipeline · claim lease · heartbeat · append spans · poll own-run status (the read that makes an abort land at the next tool call). Nothing else. Approve, unlock and review endpoints reject it — a rejected write is refused loudly and lands in the audit log. |

> **Untrusted imports**
>
> Anything imported from Claude Code or Cursor lands *untrusted*. An untrusted skill will not materialize into any pipeline; no step may delegate to an untrusted subagent. Both show a red banner in the library with a **Mark reviewed** button beside the body a human is expected to read. Compile is hard-blocked while any referenced item is untrusted, and the compile dialog names them: *Compile refused — pr-description (skill) imported but not yet reviewed. Review in the library first.*

> **The capability report**
>
> Compiling is the approval point for what a pipeline can *do*, not just what it says. The dialog computes four lines from the live graph and asks a human to accept them:
>
> | Line | Contents |
> | --- | --- |
> | writes | every output path across all doc nodes |
> | shell | count and first three of stage commands plus hook scripts |
> | network | which subagents hold WebSearch, or "none" |
> | egress | the project allowlist, or "empty — all egress refused" |
>
> Hooks and shell stages run with no network unless the host is on the project's egress allowlist. `127.0.0.1:7420` is implicitly exempt — Surge's own hooks (span emission, guards, status polls) must always reach Surge — and the report says so on the egress line rather than hiding it: *egress: empty — all egress refused (loopback to Surge always allowed)*. The list is checked at dispatch and shown here. The dialog closes with the signature line: `sha256:a92f1c9…` · signed by `st_9f…c41` (this instance) · imports verify provenance.

---

## 05 — Versioning & change management

Four things version independently, and each has one rule for what happens to everything downstream when it moves.

> **Library items — immutable per version**
>
> Editing a hook, subagent or skill marks it unpublished edits and offers **Publish vN+1**. Pipelines stay pinned to the version they reference until explicitly bumped. The detail page states this in place: "immutable per version — edits publish a new one".

> **Pipelines — fork, never edit in place**
>
> Forking copies the graph and its version-pinned references, produces a new hash, and leaves the blessed template untouched. Version history is a list of version · hash · date with an inline diff. Forks are badged fork and live under "My forks".

> **Docs — parent-change badges, not cascade invalidation**
>
> When a parent doc changes after a child was approved, the child is badged parent changed with the exact hashes ("docs/spec.md was 9f2a at approval, is b7c1 now") and offers **Re-derive from current parent**. Downstream docs keep working; approval resets on completion of the re-derive.

> **Taskgraph — amend, never re-expand**
>
> Once approved, the taskgraph never regenerates the board. Re-approving diffs against the live board: new issues insert, removed issues close as cut, and done and in-flight work is never touched. The amendment dialog shows exactly that diff before it is applied.

> **Work orders — revisions clear their review**
>
> Saving a work-order edit bumps the revision and clears the Gate-2 review, requiring a re-review. If the file changed after the issue was generated, the issue is refused with a hash mismatch and flagged red on the board until a revision adopts the change.

---

## 06 — Execution lifecycle

From an approved taskgraph to a merged wave, with every point a human can intervene.

**01 · Eligibility** — An issue becomes eligible when its Gate-2 review is recorded, its wave is open, and no required gate upstream is locked. The eligible column is ordered by priority.

**02 · Dispatch** — The dispatch stage checks the materialization first. Stale means refusal, and the refusal itself produces a run with one span carrying the reason. Queue policy and parallelism are shown on the phase banner: priority, then wave · max 3 parallel.

**03 · Lease** — A worker claims a lease with a TTL of 10 minutes and heartbeats against it. The Orchestration tab shows the live line — heartbeat 8s ago · lease expires in 9m (TTL 10m). Stop heartbeating and the lease reclaims: lease reclaimed — worker-2 stopped responding (TTL 10m) · retry 3 queued.

**04 · Implement → verify → retry** — The implement node fans out (×3 by default). Verification passes forward to integrate, or fails back to implement — the retry edge is part of the graph, not hidden control flow. Retries cap at 3 and the count is on the card.

**05 · Wave integration** — Each wave has an integration issue that rebases its task branches in dependency order, runs the integration contract checks, and opens the wave PR. A conflict halts assembly and opens a conflict report on that issue; a failed contract check fails the wave, not the tasks.

**06 · Budgets, caps and aborts** — Each wave carries a budget (wave budget $12.00 · $7.41 spent) and each role a per-run cap. A cap breach pauses the wave and queues a required gate — the same path as the budget breaker. An abort is written to the ledger and takes effect at the executor's next tool call; if heartbeats stop first, the lease reclaims at TTL.

> **Metric definitions — all provisional except three**
>
> Status, latency and cost are measured. Everything else is provisional because there is no label source yet; COE verdicts are what will eventually calibrate them. The Observatory says so in place, and both the node eval panel and the replay dialog state that their results never feed calibration.
>
> | Metric | Definition |
> | --- | --- |
> | decomposition quality | scored at the phase-carve node — the step that decides wave shape |
> | pass@k | at least one of k attempts succeeds |
> | pass^k | all k attempts succeed |
> | verifier false-positive | verifier passes later overturned by a human review |
> | cost by role | span cost summed into coordinator / worker / verifier |

---

## 07 — Page: the global shell

Present on every screen. A 236px sidebar, a content column, and two floating layers — dialogs and a toast.

### Sidebar — *236px · fixed · card surface · right border*

| Region | Behaviour |
| --- | --- |
| WORDMARK | "Surge", display face, 21px. Clicking it returns to the Registry from anywhere. |
| INSTANCE NAV | Registry · Pipelines · Library · Settings. Lucide icon plus label; the active row takes the sunken background and 500 weight. Pipelines stays active while the builder is open. |
| SWITCHER | A sunken card showing the active project name and repo path with a chevron. Opening it drops a menu of every project — name plus pipeline version — over a full-screen click-catcher, with "All projects" at the bottom. Picking one lands on that project's Overview. |
| PROJECT NAV | Overview · Docs · Board · Pipeline · Observatory · Settings, same treatment. Always visible, even while an instance surface is on screen — the switcher is a scope, not a mode. |
| FOOTER | An accent dot and `127.0.0.1:7420 · local`. The one permanent reminder that this is a local service. |

### Floating layers

| Layer | Behaviour |
| --- | --- |
| TOAST | Bottom-right, dismissible, auto-clears after 3.4s. Three tones: success, info, error. Every state-changing action produces one — the full inventory is §21. |
| DIALOGS | Seven, all mounted at the shell so any surface can open any of them: Replay, Compile, Attach COE, Bind project, Assign pipeline, Import, Taskgraph amendment. Escape closes whichever is open. |
| APPEARANCE | Theme (light \| dark) and text size (small \| default \| large \| larger) are instance settings that re-scale the entire interface, not just labels. Dark keeps the single-accent palette. |

---

## 08 — Page: Registry

The landing surface. Two jobs: show what needs a human right now, and show the health of every bound project.

### Registry — *instance · default landing*

| Region | Behaviour |
| --- | --- |
| HEADER | Title, a search field ("Search projects or repos", matches name and repo path), and a primary **+ Bind project**. |
| NEEDS YOU | An accent-bordered panel above the grid with a count and one row per item, each a direct jump. The four live conditions: *taskgraph awaiting approval* → that project's Docs; *required gate awaiting a human — Wave 3 entry* → the blocked issue; *dispatch refused — stale materialization* → that project's Pipeline; *run failed — model returned a 429* → that run in the Observatory. Approving the taskgraph removes the first row. |
| PROJECT CARD | Auto-filling grid, minimum 280px. Each card: name, repo path in mono, a board-health badge ("3 in-flight", "board clear", "stale materialization"), then a divided stat block — Pipeline name · version, surge.yaml badge, and Last run as a status pill plus relative time. Hover raises the border and a small shadow. The whole card opens the project. |
| EMPTY | A centred icon tile, "No projects yet", the line "Bind a repo to give it a home in Surge — config, docs, board, pipelines, and observability all in one place.", and **Bind your first project**. Reachable in the prototype via the `startEmpty` prop; binding one exits the state permanently. |
| NO MATCH | "No project matches *query*." below the (empty) grid. The attention panel stays. |

---

## 09 — Page: Pipelines

The cross-project pipeline library. A filter rail, a card grid, and a detail view that is the single place to understand what a pipeline is made of before you assign it to anything.

### Pipelines — list

| Region | Behaviour |
| --- | --- |
| FILTERS | A 186px rail: **Blessed templates** (default) · **My forks** · **By project**. The first two render a flat grid; the third groups cards under project headings and hides pipelines nothing is assigned to. |
| SEARCH | "Search harnesses" — matches name, version and hash. Hidden while a detail is open. |
| CARD | Name, a blessed or fork badge, version · hash, and a footer with the assigned project count and, in error red, a stale-materialization count when any exist. |
| EMPTY | Three distinct copies. Search miss: "No pipeline matches *q*" / "Clear the search to see the full library." Forks filter with none: "No forks yet" / "Fork a blessed template to edit it without touching the original." By-project with none: "Nothing here yet" / "Assign a pipeline to a project and it will show up here." |

### Pipeline detail — *← All pipelines*

| Region | Behaviour |
| --- | --- |
| HEADER | Name, then v14 · a1b2c3d · signed st_9f…c41. Actions: **Fork**, **Assign**, **Open builder**. Forks additionally show "Forked from coordinator-worker-verifier @ v13". |
| COMPOSITION | The heart of the page. A summary line ("2 hooks · 3 subagents · 7 skills"), then three tables — hooks, subagents, skills — each row giving the item name, its pinned detail (version · script path, version · file path, or version · model), and which nodes use it. Every row opens that item in the Library. Broken references render the name in red with "not in the library", and a red banner above states how many there are and that compiling will fail until they are replaced. |
| NOTES | Empty graph: "This pipeline has an empty graph — no hooks, subagents, or skills are referenced yet." Otherwise the pinning rule is restated in place: forking copies the graph and its version-pinned references; library items are immutable per version. |
| HISTORY | Version · hash · date rows with a **View diff** toggle that expands a dark monospace diff of node and edge changes. |
| MATERIALIZATIONS | Project · cache key · fresh \| stale badge, or "No materializations yet." This is where a stale project is first visible from the pipeline's side. |
| FORKING | Fork creates "*name* — fork" at v1 with a fresh hash, switches the filter to My forks and opens the new detail. Toast: "Forked to … — the blessed template is untouched." |

---

## 10 — Page: Library

One surface, three tabs, one shape: a card grid that becomes a full editor when you open an item. Every card carries a usage count, so nothing in the library is ever a mystery attachment. The header holds **Import…** and a tab-aware add button.

### Hooks — *reusable shell hooks · written once, attached anywhere*

| Region | Behaviour |
| --- | --- |
| CARD | Name, version (with "· draft" when edited), a blocking or advisory badge, the description, then the event list and the attachment count ("unattached" / "3 attachments"). Search filters by name and event. |
| DETAIL | Three stacked cards. *One* — version line plus either "immutable per version — edits publish a new one" or an unpublished edits badge with **Publish vN**; Name and Script inputs; "What it does"; "Valid for events" as eight togglable chips (the last one cannot be removed); a blocking switch with the constraint stated — "Blocking hooks can only refuse on events that support it." *Two* — the script path and a dark body editor, closing with the contract: exit 0 proceeds and logs stdout, exit 2 refuses and returns stderr to the model, anything else is a non-blocking error. *Three* — "Attached in": project, node, event, scope, each row jumping to that node on that project's canvas. Empty: "Not attached anywhere yet. Add a hook node on a pipeline and pick this hook." |
| SHIPPED | load-context (SessionStart, advisory) · guard-interface (PreToolUse, blocking — refuses writes outside the work order's stated interfaces) · block-secrets (PreToolUse, blocking) · format-on-write (PostToolUse) · collect-scores (SubagentStop) · emit-span (four events) · require-clean-tree (Stop, SubagentStop, blocking). |

### Subagents — *one file per subagent under `.claude/agents`*

| Region | Behaviour |
| --- | --- |
| CARD | Name, version (with "· untrusted" when imported), model, description, tool list, usage count. |
| DETAIL | Version line, the file path, publish control; an untrusted banner where relevant — "Imported and not yet reviewed — no pipeline step can delegate to this subagent at compile until a human marks it reviewed." with **Mark reviewed**. Then Name, Model select, "When to delegate to it" (with the note that this is what the coordinator reads when handing off work), and Allowed tools as chips over the eight-tool set. A "Used by" card lists pipeline, node and node kind. |
| SHIPPED | researcher, architect, cross-spec-auditor, planner (opus), implementer, verifier (sonnet). Read-only roles hold no Write tool — that is the point, and it is visible on the card. |

### Skills — *one SKILL.md per skill · invoked by name*

| Region | Behaviour |
| --- | --- |
| CARD | Name, version, an origin badge — local, from Claude Code, from Cursor, or *untrusted* — description, tools, usage. |
| DETAIL | Version and origin, publish control, the untrusted banner ("…this skill will not materialize into any pipeline until a human reads its body below and marks it reviewed"), Name and File, "When to invoke it", tool chips, and — for Cursor imports — a lossy-import note naming exactly what was dropped: "Dropped on import from Cursor — globs: `**/*.ts` · alwaysApply: true. Surge has no equivalent; the skill is invoked by name instead of matched by path." Below, the raw body editor, then "Used by". |
| SHIPPED | The seven that back the default doc chain: research, spec, invariants, phase-carve, feature-design, cross-spec-audit, taskgraph. |

---

## 11 — Page: Pipeline editor

The largest surface in the product and the one that carries the bet. Four regions: a toolbar, a palette rail, the canvas, an inspector, and a bottom pane under all of them. Everything a pipeline is — its graph, its prompts, its compiled output, its recorded payloads and its live execution — is reachable without leaving this screen.

### Toolbar — *wraps rather than truncates*

| Region | Behaviour |
| --- | --- |
| IDENTITY | Pipeline label with version, and under it the scope line — builder: "Editing the template. Every project assigned to this pipeline picks the change up at its next materialization." Project: "Materialized for *project*. Edits here stay on this project until you push them to the template." |
| BUILDER ONLY | "← Pipelines" back link, and a version-diff toggle labelled with the previous version ("Compare v13" → "Diff · on"). |
| PROJECT ONLY | **Edit template** (jumps into the builder for the assigned pipeline), **Fork**, **Run overlay**, and **Debug live run** / **Detach debugger**. |
| ALWAYS | Undo · Redo (60-step history over graph state only), **Dry run**, and the primary **Compile**. The three overlays — dry run, run overlay, diff, debugger — are mutually exclusive; entering one clears the others. |

### Palette & canvas

| Region | Behaviour |
| --- | --- |
| PALETTE | 172px, grouped: Docs (+ doc node · writes a `docs/*.md`) · Agents (+ subagent step) · Hooks (+ hook · PreToolUse, Stop, …) · Skills (+ skill) · Stages (+ shell stage · deterministic command) · Annotate (+ sticky note, + frame selection). A Blocks group appears once anything has been published. Each entry adds a node with sensible defaults below the existing graph and selects it. A legend pins the three colour meanings: doc writes a file, subagent, hook · shell. |
| VIEWPORT | Dot grid. Drag empty space to pan; wheel to zoom between 0.35× and 1.6× around the cursor; shift-drag to marquee-select; bottom-right controls are zoom in, zoom out, and fit-graph-to-view. The graph auto-fits when the scope changes. |
| NODE CARD | 208×78, border-defined. Top row: kind badge, an optional diff badge (added / changed), a debugger tag (paused here / state edited), fanout ("×3 parallel"), a dot when the node emits a span, and a lock glyph when a human gate sits on its exit. Then label and a mono subtitle that reads the node's real binding — the output path, the subagent name and model, the hook name plus session or scoped, the skill path, or the shell command. With the run overlay on, a footer line adds duration · cost · status. A breakpoint dot sits on the left edge; a + port on the right starts a connection. |
| EDGES | Bezier curves; back-edges bow underneath rather than crossing the graph. Gated edges are accent-coloured and dashed with a lock button at the midpoint that selects the edge. Hook scope edges are dotted. Deleting a node removes its edges. |
| SELECTION | Shift-click or marquee builds a multi-selection; a floating pill then offers Group · Frame · Duplicate · Delete *n* nodes · Clear. Deleting asks twice — the button becomes "Click again to delete" for three seconds. |
| ANNOTATION | Sticky notes (draggable, three tones, placeholder "Why this part of the graph looks the way it does.") and frames (a labelled dashed region around a selection that moves its members together). Both are scoped to the pipeline and change nothing about execution — the frame toast says so explicitly. |
| BLOCKS | Grouping two or more nodes creates a composite block that collapses into one card. Its inspector lists members, offers Expand · Publish to library · Ungroup, and exposes per-instance parameters: pick a member setting (subagent, fanout, output path, command, event, human gate) from a candidate list and it becomes a named parameter every instance of the block sets for itself. Publishing puts the block in the palette, versioned like any library item. |

### Inspector — *310px, four tabs*

| Tab | Behaviour |
| --- | --- |
| CONFIG | Name always; then per kind — *doc*: Writes path, subagent, skill; *agent*: subagent, parallel runs; *stage*: command; *skill*: skill picker with its path and an "Edit skill" jump; *hook*: the full hook panel (below). Every node ends with "Human gate on exit" — a switch whose sub-line states the consequence, "blocks every outgoing edge". When a node uses a subagent, an inline subagent editor appears: file path, name, description, model, tool chips — edited here, published from the Library. Footer: **Connect to…** and Delete. |
| HOOK PANEL | Hook select with its script and an "Edit script" jump; the hook's own description; Event select restricted to the events that hook declares, with the firing rule spelled out ("Fires before every matching tool call."); a matcher that changes shape by event — tool chips for PreToolUse and PostToolUse ("No tools selected — matches every tool call."), a subagent select for SubagentStop, nothing otherwise. Then **Scope**: Session-wide or One step, with a target picker for the latter and a plain-language note either way. A blocking badge resolves the three-way truth: advisory, blocking, or "This hook blocks, but *Event* cannot be blocked. It will run advisory-only here." Last, the exact stdin payload the hook will receive, rendered as JSON. |
| PROMPT | The prompt body the step runs, editable, with the consequence stated: "This is the file the step runs. Editing it here rewrites the skill on the next compile." |
| EVAL | For prompt-bearing nodes only. **Run eval ×3** against three fixtures, each row showing pass/fail per attempt; then pass@3 and pass^3, and a short history of previous revisions. Below, a context budget breakdown — system + harness, work order, repo context, prompt body — as bars with token counts and a total against the 200k window. Closing note: fixtures run locally against this prompt revision and are never used for calibration. Command nodes get "This node runs a command, not a prompt — nothing to eval here." |
| OBS | Emit a span (switch, "shows up in the run waterfall"), Feeds metric (five options including "not measured"), and "Why this is measured" — free text written into the run record next to the span. This is how the Observatory's metrics stay explainable at the point they are configured. |
| EDGE | Selecting an edge replaces the panel: from → to, a "Fires on" trigger field, a Required gate switch, and — when gated — an accent box explaining that it blocks until a human unlocks it, that unlocking is versioned and lands in the policy-decision log, plus the unlock switch and its state (unlocked · v2 · logged). Delete edge sits at the bottom. |
| EMPTY | "Select a node or an edge to inspect it. Drag from a node's + handle to connect it to another." |

### Bottom pane — *four tabs*

| Tab | Behaviour |
| --- | --- |
| PROMPT BODY | Read-view of the selected node's prompt, or "Select a node to read the prompt that runs there." |
| DATA IN / OUT | The recorded payload the node received and the one it emitted, keyed row by row, each row marked and tinted by what happened to that key: `+` added, `−` dropped, `~` changed, blank for unchanged. A **Replay this node** button sits in the header. While paused at a breakpoint, the "data in" column reflects the staged edit rather than the recording. |
| COMPILES TO | The exact file this node produces, titled with its real path. A doc or skill node renders a SKILL.md with frontmatter; a subagent node renders an agent `.md`; a session-wide hook renders a `.claude/settings.json` hooks block; a step-scoped hook renders either a block merged into that subagent's file — with the comment "scoped to *agent* — it does not fire for any other subagent in the session" — or a surge.yaml step block Surge injects for the step's duration and then removes; a stage renders a surge.yaml step. This pane is what makes the compile step legible rather than magic. |
| CODE | The whole pipeline as YAML, JSON or Mermaid, two-way synced. Editing marks it dirty ("edited — apply to rebuild the pipeline") and reveals Revert and **Apply to pipeline**. Applying replaces the graph wholesale, auto-lays out anything without coordinates by dependency depth, and refits the view. Validation refuses with a specific message: parse error, no nodes found, node without an id, duplicate node id, or an edge referencing a missing node. Pasting a whole graph from elsewhere is a supported path, not a side effect. |

### The four canvas modes

| Mode | Behaviour |
| --- | --- |
| DRY RUN | A topological walk of the graph with no execution. A bar shows step *n* / *N*, the node label, what it would do in plain language ("writes docs/spec.md as architect (claude-opus-4)", "delegates to implementer (claude-sonnet-4) × 3 parallel", "guard-interface fires on PreToolUse — can refuse (exit 2)"), and a running cost estimate. Nodes ahead dim, visited nodes turn green, the current node takes the accent border. Where a locked required gate sits on the way in, the bar adds a second line naming the gate. Retry and scope edges are excluded from the ordering. Back · Next step · Exit. |
| RUN OVERLAY | Project canvas only. Paints the latest run onto the graph: per-node duration, apportioned cost and status, error nodes taking a red border, plus a chip naming the run and its total. The authoring view and the execution view become the same picture. |
| DIFF OVERLAY | Builder only. Compares against the previous version: added nodes badge green, changed nodes badge accent, and removed nodes appear as dashed non-interactive ghosts in their old position — so a deletion is visible instead of merely absent. |
| DEBUGGER | Project canvas only, and the most opinionated thing in the product. Arm breakpoints by clicking the dot on a node's left edge (or cmd-B on a selection), then **Debug live run** attaches to the running run — attachment is itself an audit entry. Execution holds *before* the node runs. The paused node badges "paused here"; the inspector shows the payload it is about to receive as editable JSON with Stage edit · Revert · Replay, then Step over · Resume. Staging an edit validates the JSON, badges the node "state edited", and writes "state edited at breakpoint" to the audit log. Completed nodes turn green, pending nodes dim, and the bar tracks "paused before step 4 / 11" with the armed-breakpoint count. Detach at any time. |

---

## 12 — Page: Project · Overview

A two-column answer to "what is this project wired to, and is it healthy". Every card is a shortcut into the surface that owns the thing it shows.

### Overview — *project header on every project tab: name · repo · last-run pill*

| Region | Behaviour |
| --- | --- |
| LEFT COLUMN | **Repo binding** — path, surge.yaml badge, tracker, branch format. **Subagents in this pipeline** — only the ones actually referenced by a node, with their models. **Hooks** — event, hook name, scope, blocking badge; each row jumps to that hook node on the canvas. Empty: "No hooks in this pipeline. Add one from the palette." |
| RIGHT COLUMN | **Pipeline assignment** — a published/stale badge, name · version, hash, a red stale box ("Materialization is stale — recompile before dispatch.") when relevant, and View in library / Fork. **Docs** — a chip per doc, green when approved, the whole card opening the Docs tab. **Recent runs** — status pill, run id, start time, each opening that run in the Observatory. |
| BANNER | When the project's tracker is Linear and Linear is disconnected, an accent banner sits under the header on every project tab: "Projecting to the built-in tracker until Linear reconnects — history is split across two trackers until then." with **Reconnect Linear**. |

---

## 13 — Page: Project · Docs

A table of the doc chain plus a reading drawer. The framing line under the table is the whole idea: every row is a doc node on the pipeline — the chain, the subagent that writes each file, and the gates between them are configured there, not here.

### Docs table

| Region | Behaviour |
| --- | --- |
| COLUMNS | Doc · Derived from · Written by · Gate · Approved by · Date · action. "Written by" gives the node label as a link into the canvas plus the subagent name, so authorship is traceable in one hop. |
| GATE BADGE | Four states: draft (no gate configured), awaiting gate (gate configured, not approved — shows an Approve button), approved, and parent changed. |
| DRAWER | 360px, right side. File path, doc kind, gate badge, close. Body: written-by (link), derived-from, approved-by, date; then the parent-changed box when relevant, with the exact hash note and **Re-derive from current parent**; then the document text. Footer changes with state — an Approve button while a gate is pending, or, once the taskgraph is approved, the line "Approved. Plans change — amendments diff against the live board instead of re-expanding it." with **Propose amendment…** |
| TASKGRAPH | The one row whose approval has consequences: it generates six work orders and expands the board from Wave 1 into the full Phase 3 tree. After approval a green banner appears above the table — "Board expanded into the Phase 3 issue tree." with a *View board* → jump. |
| UNGENERATED | A doc node with no run behind it still lists, with the body "This document has not been generated yet. Run the pipeline, or open the node to edit the prompt that writes it." |

---

## 14 — Page: Project · Board · Plan

The human half of the board, and the default sub-tab. A read-only mirror of the repo's tracker: releases, sprints and issues as a team already knows them. Two fields are Surge's own — sprint and planning status — and neither is ever written back. The header carries the sync chip `acme/fleet-orchestrator · synced 2m ago · read-only` and a Sync now control.

### Three views

| Region | Behaviour |
| --- | --- |
| BOARD | Kanban with a second grouping axis: by **Status** (Backlog · Todo · In progress · In review · Done), by **Sprint** (Sprint 13 · Sprint 14 "current" · Sprint 15 · Unscheduled), or by **Milestone** (v0.3.0 shipped · v0.4.0 due Aug 28 · v0.5.0 due Oct 9). Cards: issue number, assignee, title, label and milestone chips, and a footer with PR state, commit count, and the linked Surge work order as an ↗ jump straight into Ops. |
| TABLE | Nine columns — issue · title · labels · milestone · sprint · assignee · pull request · status · work order — horizontally scrollable at a 960px minimum. The work-order cell is the cross-link; the PR cell is colour-coded by state (merged, open, draft, changes). |
| ROADMAP | Release bars across a Jul–Oct axis, styled by state — shipped (green fill), active (accent fill showing percent complete), planned (dashed outline) — with a today marker, a closed/total count and a due label per row, and a legend. Clicking a release opens its issues as a filtered board. Below it, a burndown card for the active release: ideal versus actual polylines, a today line, a trend chip ("on track · 8 open vs 7.9 ideal"), and four figures — open, closed to date, ideal remaining, due date. |
| FILTERS | A milestone segmented control and a text filter, both on board and table; a count line ("10 of 14 issues"). Roadmap ignores both. |
| DRAWER | 400px. Header: issue number · open\|closed, title, status pill. Two labelled groups — **Synced from git** (milestone, assignee, labels, pull request, and a commit list with sha, message and age) and **Surge planning** (status, sprint, work order). A closing note repeats the contract: "Read-only mirror of *repo*. Issue fields sync one way from git; sprint and status are Surge planning fields and are never written back." Footer: Open in git, and Open work order where one exists. |
| UNCONFIGURED | Projects without a mirror get a centred empty state: "No issue mirror for this repo" / "Plan tracks releases, sprints, and issues from a read-only mirror of the repo's tracker. Nothing is written back to git." with **Connect issue mirror**. |

---

## 15 — Page: Project · Board · Ops

The machine half — work orders, gates, leases and dispatch, all owned by Surge. The sub-tab hint reads "work orders · gates · dispatch".

### Board

| Region | Behaviour |
| --- | --- |
| PHASE BANNER | Above the columns: phase title, a done/total task count, a wave chip row coloured by state (done · active · pending), the integration contract in words ("Spans join on run_id; coordinator emits the root span before dispatch."), and the two operational facts — dispatch queue policy ("priority, then wave · max 3 parallel") and wave budget ("wave budget $12.00 · $7.41 spent"). |
| COLUMNS | Six, fixed: Eligible · In-flight · Leased · Retrying · Blocked · Done. Each has its own empty copy ("no eligible issues", "nothing in flight", "no leases held", "no retries", "nothing blocked", "nothing merged yet"), which becomes "no match" under a filter. Eligible is ordered by priority. |
| CARD | Issue id, a red warning glyph on hash mismatch (and a red card border), the title, then a meta line combining wave, "· integration" for wave-assembly issues, priority and non-default disposition, plus a status pill. A footer appears when there is a lease or retries: "lease · worker-2" and, in error red, "2/3 retries". |
| FILTERS | Free-text filter, a wave segmented control built from the waves actually present, and a visible/total count. |
| PENDING | Before the taskgraph is approved, only Wave 1 exists and a banner says so: "Taskgraph is still a draft, so Wave 2 and Wave 3 issues haven't been generated. Only Wave 1 is on the board." with **Review taskgraph**. |

### Issue drawer — *440px, three tabs*

| Tab | Behaviour |
| --- | --- |
| WORKORDER | Path · revision · hash, then the body as an editable document. The note underneath states the model: "Versioned in Surge, rendered to the runtime as a file. Saving bumps the revision and clears the Gate-2 review." **Save revision** appears only when dirty. On a hash mismatch a red refusal opens the tab: "Refused: work_order.md changed after this issue was generated. Save a revision here to adopt the change and re-queue Gate-2 review." |
| GATE-2 | Four checkboxes, fixed: acceptance criteria are testable · interfaces/contracts specified · no hidden dependencies on unmerged work · rollback/verification steps included. Below, "Reviewed by @priya" once recorded. |
| ORCHESTRATION | Machine-owned above, human-owned below, and the boundary is stated: "Stored beside orchestration status, never inside it. Priority orders the dispatch queue." Machine: lease owner, retry count, and the live lease note. Human: **Disposition** (active · deferred · blocked · cut) and **Priority** (— · P0 · P1 · P2). Then run history — id, status pill, age — each opening that run in the Observatory. |
| FOOTER | Refresh projection · **Mark Gate-2 reviewed** · Open COE. The review button is disabled on hash mismatch, when already reviewed, or when any checkbox is unticked — three separate reasons, one disabled state. |

---

## 16 — Page: Project · Observatory

Three columns: a run list, a run detail built around the span waterfall, and a metrics rail. Everything measured is measured here, and everything provisional says so.

| Region | Behaviour |
| --- | --- |
| RUN LIST | 270px. Search by run id, a status segmented filter (All · Success · Running · Error), then rows of run id with a status pill and `v14 · 18m ago · 2m 04s`. Two empty copies: "This project has not dispatched a run yet." and "No run matches the current filter." |
| RUN HEADER | Run id, status pill, dispatch kind (interactive session \| headless `claude -p`) and cost against the run budget. **Abort run** shows only while running; **Attach to COE** always. After an abort, a banner explains the semantics rather than pretending it was instant: "Abort requested — takes effect at the executor's next tool call. If heartbeats stop, the lease reclaims at TTL." |
| WATERFALL | One row per span: role, label indented by depth, a proportional bar coloured by status, the duration as a number (never inferred from bar width), a status pill, and an arrow that jumps to the node that emitted it. Clicking a row expands its policy decision — the string that says why something was retried, escalated or refused. A time axis runs above at five ticks. Jumping to a node that no longer exists is handled honestly: "That span was emitted by a node that no longer exists in this pipeline version." |
| COE | Cause-of-error records for the project, each with its context, age, text and a Remove. **Suggest ratchet** reads the text and proposes one concrete tightening — a routing fallback, a verifier acceptance criterion, a blocking guard hook, or a required gate for one wave — which **Apply** records against the next pipeline version. Empty: "No cause-of-error records for this project. Attach one from a failed run or a blocked issue." |
| AUDIT | Policy & audit log — action, subject, actor, when — six most recent with a total count, closing with "Approvals, Gate-2 reviews, gate unlocks, imports, aborts, and token changes land here. COEs cite it — one trail." |
| METRICS RAIL | 264px. Opens with the honesty note: "Quality metrics are provisional — there is no label source yet. COE verdicts will calibrate them; status, latency, and cost are measured." Then decomposition quality, pass@3 and pass^3 side by side with their one-line definitions, verifier false-positive rate, and cost by role as a stacked bar plus three figures. At the bottom, **Node replay / playground** and "Not used for calibration." |

---

## 17 — Page: Settings, both levels

Two settings surfaces with a clean split: the instance owns the machine and the credentials, the project owns the binding and the policy.

### Instance settings — *640px column · six cards*

| Card | Behaviour |
| --- | --- |
| APPEARANCE | Theme (Light \| Dark, "Dark keeps the single-accent palette") and Text size (Small · Default · Large · Larger, "Scales the whole interface, not just labels"). Scoped to this machine. |
| SUBAGENTS | The full roster with description, tools and a per-subagent model select — the one place to re-model everything at once. Model options are provider-qualified from the MODEL PROVIDERS card (`anthropic/claude-opus-4`, `deepseek/deepseek-chat`). The card states where the rest lives: "Tools and prompts are edited on the pipeline node that uses them." Open library sits in the corner. |
| MODEL PROVIDERS | The provider registry (§23-Twenty-One). One row per provider: name, kind badge (`anthropic` built-in · `anthropic-compatible` · `openai-compatible via proxy`), base URL, key state (set \| missing — never the key itself), declared models as chips, and Test connection. **+ Add provider** opens name · kind · base URL · API key · model list. The card's subtitle is the rule: "Keys are Surge-side only — injected into workers at spawn, never written to a repo, a compiled file, or a backup (INV-AUTH-6). Provider hosts appear on the egress line of every capability report." Removing a provider that any subagent or fallback references is refused, naming them. |
| API TOKENS | The trust boundary, in the UI: one human session token with full control and one runtime token per project scoped to fetch · claim lease · heartbeat · append spans · status poll. Each rotatable. The card's subtitle is the rule (§04). |
| CREDENTIALS | GitHub (connect) and Linear (connected/disconnect, or disconnected/connect). "Surge-side only. Never written into a repo." Disconnecting Linear is what produces the degraded-tracker banner on every project bound to it. |
| BACKUP | A freshness pill, last push time, Force backup push and Restore…, the scope line ("docs, pipelines, library items, boards, and work orders. Credentials never leave the machine; span bodies are opt-in"), and Span retention (30 · 60 · 90 days) with the retention rule: raw model and tool I/O compacts after the window, timings and cost are kept forever. |

### Project settings — *640px column · six cards*

| Card | Behaviour |
| --- | --- |
| REPO BINDING | Path, surge.yaml badge, repo fingerprint, mount scope, and the adoption rule: "A cloned repo claiming this workspace id with a different fingerprint prompts for explicit adoption. Only the mount path is visible to pipeline shell scripts." Regenerate surge.yaml sits below. |
| TRACKER | Kind select (Linear · GitHub Issues · Built-in) plus Test connection. |
| BRANCH FORMAT | A template field with a live preview — `surge/{issue-id}-{slug}` renders as `surge/sur-151-wire-span-emitter`. |
| MODEL ROUTING | Primary models come from the pipeline's roles; this card is only about failure. A fallback model (provider-qualified, from the instance MODEL PROVIDERS registry) used after two failed retries on 429/5xx, and per-run cost caps for coordinator, worker and verifier. The consequence is stated: "A cap breach pauses the wave and queues a required gate — the same path as the budget breaker." |
| EGRESS | An allowlist of hosts with add and remove. "Hooks and shell stages run with no network unless the host is listed here. Checked at dispatch and shown in the compile capability report." Every edit is audited. |
| HOOKS | Read-only projection of every hook node in the pipeline — event, matcher, command, scope, blocking badge — with the note that they compile into `.claude/settings.json` and are edited on the pipeline. Each row jumps to its node. |

---

## 18 — Dialogs & overlays

Seven modals. Each is an approval point, not a form for its own sake.

**Bind a project** — *from Registry*
Repo path, tracker, pipeline. Opens with the promise: "Point Surge at a repo. It writes a surge.yaml at the root and reads nothing else until you assign a pipeline." Three validations, each with its own message (§21). Success creates the project with no runs, an empty board, and the chosen pipeline already assigned.

**Assign harness to a project** — *from pipeline detail*
A list of every project with its current pipeline named, and the already-assigned one marked. Picking one reassigns immediately; the toast states when it takes effect — "fetched at the next session start".

**Import into the library** — *from Library*
Source select — Claude Code skills, Claude Code subagents, Cursor rules — each with a note on how its fields map, and a scan line naming the path and repo. Candidates are checkable rows with name, description, file and a *view source* toggle that reveals the raw body or frontmatter. Already-imported items are disabled and badged; Cursor rules carry a warning badge naming what will be dropped. The footer counts selections and restates the rule: "imports land untrusted until reviewed".

**Materialization diff (Compile)** — *from the pipeline editor*
Previous hash versus new hash, a one-line structural summary that names an unlocked required gate if there is one, the capability report (§04), and the signature line. Confirm is disabled while any referenced import is unreviewed, and the red box names them.

**Replay** — *720px · from the canvas, the debugger, or the metrics rail*
Two panes: the recorded input as editable JSON, and the output. Before running, the right pane shows the *recorded* output with the note that replaying replaces it here so you can compare before pinning. Running validates the JSON, takes about a second, then reports a delta line — how many keys differ, latency, cost. Then **Pin to span** and **Replay downstream**, which queues a re-run of every node after it. Both header and footer repeat: replays are recorded but never used for calibration.

**Attach cause-of-error** — *from a run or an issue*
Names what it is attaching to, then one field — "What went wrong, and why" — and a prompt toward the ratchet: tighten a verifier, or add a gate.

**Taskgraph amendment** — *from the doc drawer*
The diff against the live board, in three lines: what inserts, what closes as cut, and what is untouched. The explanation sits above it, and the button says Apply amendment — not "re-approve".

---

## 19 — Critical flows

Thirteen paths that cross more than one surface. Each is walkable in the prototype.

**A · First run — bind a repo to a dispatched pipeline**
Registry → + Bind project → path · tracker · pipeline → surge.yaml written → project Overview → Pipeline → Compile → capability report accepted → Board · Ops → dispatch

**B · The doc chain to a board**
Docs → Research → Spec → Invariants (*gate*) → Phase carve → Feature design → Cross-spec audit → Taskgraph (*gate*) → Approve → 6 work orders generated → board expands Wave 2 + Wave 3

> Approving the taskgraph is the single highest-consequence click in the product. It is gated, audited, and the only action whose toast names the exact downstream effect.

**C · Unlock a required gate**
Registry "Needs you" → blocked issue → Pipeline → gated edge → lock button → Required gate box → unlock switch → audit entry + versioned state → next compile summary names the unlocked gate

**D · Work-order revision and re-review**
Board · Ops → issue → WorkOrder → edit → Save revision → rev bumps, Gate-2 review cleared → Gate-2 tab → four checks → Mark Gate-2 reviewed → issue becomes eligible

**E · Import, review, compile**
Library → Import… → source → view source → select → Import (*untrusted*) → attach to a node → Compile *refused* → Library → read body → Mark reviewed → Compile succeeds

**F · Fork a blessed template and roll it out**
Pipelines → detail → Fork → My forks → Open builder → edit → Compare v13 (diff overlay) → Compile → Assign → pick project → picked up at next session start

**G · Debug a live run**
Project → Pipeline → arm breakpoint (node dot or cmd-B) → Debug live run (*audited*) → paused before node → edit incoming state → Stage edit (*audited*) → Step over | Resume | Replay node → Detach

**H · Diagnose from a span**
Observatory → failed run → span row → expand policy decision → arrow → the node that emitted it → Data in / out → Replay this node → edit input → Replay → Pin to span | Replay downstream

**I · Failure to fix — COE and ratchet**
failed run | blocked issue → Attach to COE → describe → Suggest ratchet → routing fallback | verifier criterion | guard hook | required gate → Apply → recorded against the next pipeline version → verify with a run

**J · Stale materialization recovery**
dispatch refused → run with one span carrying the reason → Registry "Needs you" → project Pipeline → red stale banner → Recompile → new materialization signed → re-dispatch

**K · Author a pipeline as text**
Pipeline → bottom pane → Code → YAML | JSON | Mermaid → paste or edit → Apply to pipeline → validated → graph rebuilt, auto-laid-out by dependency depth, view refit

**L · Turn a pattern into a reusable block**
shift-select nodes → Group → inspector → Expose parameters (subagent · fanout · writes · command · event · gate) → Publish to library → palette → insert instance → set per-instance values

**M · Plan to Ops and back**
Board · Plan → issue card → work-order id ↗ → Board · Ops issue drawer → run history → Observatory run. In reverse, an Ops issue names its parent phase and wave, and the Plan drawer names its Surge work order.

---

## 20 — Empty, degraded & refusal states

A refusal always states what happened and, where it can, why. Nothing in this product fails with "Something went wrong."

| CONDITION | WHAT THE INTERFACE DOES |
| --- | --- |
| no projects | Registry empty state with a single primary action. Nothing else in the app is hidden. |
| stale materialization | Red banner on the canvas with Recompile, a red box on Overview, a warning badge on the pipeline card and in the registry, and dispatch refused with a run that records the reason. |
| untrusted import | Red banner on the library item, warning badge on its card, and Compile hard-blocked with the offending items named. |
| missing reference | Composition row renders red with "not in the library", and a banner counts them: compiling will fail until they are replaced. |
| hash mismatch | Red card border and warning glyph on the board, a refusal box on the WorkOrder tab, and Mark Gate-2 reviewed disabled until a revision is saved. |
| taskgraph draft | Board shows Wave 1 only, with a banner explaining why and a jump to review it. The Registry lists it under "Needs you". |
| tracker offline | Accent banner on every tab of every affected project, naming the consequence — history splits across two trackers — with a reconnect action. Work continues against the built-in tracker. |
| no issue mirror | Plan sub-tab shows a configure state; Ops is unaffected. The two halves of the board fail independently. |
| lease lost | Issue moves to Retrying, the lease note explains the reclaim and the queued retry, and the retry count shows in error red on the card. |
| deleted node, live span | The Observatory jump lands on the canvas with nothing selected and says the node no longer exists in this pipeline version. |
| invalid code / JSON | Apply refuses with the specific fault — parse error, missing id, duplicate id, dangling edge. Breakpoint state edits and replay inputs are validated as JSON before they are accepted. |

---

## 21 — System copy inventory

Every confirmation and refusal string, verbatim. They carry most of the product's teaching, so they belong in the spec rather than only in the code.

### Confirmations that name a consequence

- "Bound *path* — surge.yaml written at the repo root."
- "Assigned to *project* — fetched at the next session start."
- "Forked to *name* — the blessed template is untouched."
- "Compiled — new materialization a92f1c9 ready, signed by this instance."
- "Taskgraph approved — 6 work orders generated, board expanded to Wave 2 and Wave 3."
- "Amendment applied — 1 issue inserted, 1 closed as cut; done and in-flight work untouched."
- "*ID* rev 2 saved — Gate-2 review cleared, re-review required."
- "Published — pipelines stay pinned to the version they reference until bumped."
- "Reviewed — this skill can now materialize." / "…this subagent can now materialize."
- "Imported 2 skills as untrusted — review before first materialization. 1 had Cursor-only fields dropped."
- "Grouped 3 nodes into a block — publish it to reuse the pattern."
- "Published "*name*" v1 with 2 exposed parameters — it is now in the pipeline palette, versioned like any library item."
- "Framed 4 nodes. A frame labels and moves them together — it changes nothing about execution."
- "Applied — pipeline rebuilt from YAML (11 nodes, 13 edges)."
- "Edit staged — the node runs against the edited payload, and the edit is in the audit log."
- "Replay output pinned to the span. Replays are recorded but never used for calibration."
- "Ratchet recorded against the next pipeline version — verify it with a run before publishing."
- "Human session token rotated — other browsers are signed out." / "Runtime token rotated — picked up at the next session start."
- "Linear reconnected — projection resumes." / "Linear disconnected — projecting to the built-in tracker until it reconnects."
- "*Repo* synced — mirror is current. Read-only: Surge never writes back."
- "Scope is now session-wide — the step link was detached. Switching back to "One step" re-links *node*."
- "Restore pulls surge-state.git, rebinds repos by fingerprint, and lists anything unresolved for manual adoption."

### Refusals

- "Enter a repo path, e.g. acme/new-service." / "Path must read owner/repo." / "That repo is already bound."
- "Compile refused — *item* imported but not yet reviewed. Review in the library first."
- "Materialization is stale. Dispatch is refused until recompiled."
- "Refused: work_order.md changed after this issue was generated. Save a revision here to adopt the change and re-queue Gate-2 review."
- "*N* reference(s) point at library items that no longer exist. Compiling will fail until they are replaced."
- "State is not valid JSON: *message*." / "Replay input is not valid JSON: *message*."
- "Parse error: *message*" · "No nodes found — check the format." · "Node 3 has no id." · "Duplicate node id: *id*" · "Edge 2 references a missing node (*a* → *b*)."
- "Select at least two nodes (shift-click or shift-drag) to group." / "…to frame them."
- "Select a node first — breakpoints attach to a node, not to the graph."
- "That span was emitted by a node that no longer exists in this pipeline version."
- "Nothing to simulate — the graph is empty." / "Nothing to debug — the graph is empty."

In-span decision strings, written by the engine, not the UI: "escalate_on_repeat_failure: retried with a stronger verifier model after 1 failed attempt." · "worker exited: model returned a 429 after 3 retries." · "Refused: materialization is stale (pipeline v12, recompiled since last dispatch). Recompile before retrying."

---

## 22 — Keyboard & cross-surface links

### Canvas shortcuts

| Keys | Action |
| --- | --- |
| ⌘Z / ⌘⇧Z | undo · redo (⌘Y also redoes) |
| ⌘C / ⌘V | copy · paste selection with its internal edges |
| ⌘D | duplicate in place, offset 48px |
| ⌘G | group into a composite block |
| ⌘B | toggle a breakpoint on the selection |
| ⌫ / ⌦ | delete selected nodes, then edges |
| shift-click | extend the selection |
| shift-drag | marquee-select |

All suppressed while a text field has focus, and only active on a canvas surface.

### Escape, in priority order

any open dialog → an in-progress link → the replay dialog → the debugger → the dry run → a multi-selection → the project menu → the issue drawer → the doc drawer → the node or edge selection

One key, one layer at a time, outermost first — so Escape is always safe to press.

### Every cross-surface jump

Registry attention row → Docs · Board issue · Pipeline · Observatory run · Registry card → project Overview · Overview hook row → that hook node · Overview pipeline card → Pipelines detail · Overview docs card → Docs · Overview run row → Observatory · Docs "written by" → that doc node · Docs banner → Board · Ops · Board · Plan card ↗ → Board · Ops issue · Board · Ops run history → Observatory run · Board · Ops → Attach COE · Observatory span arrow → pipeline node · Observatory → Replay dialog · Pipelines composition row → Library item · Library attachment row → that node on that project's canvas · Pipeline inspector "Edit script" / "Edit skill" → Library item · Project settings hook row → that hook node · Project canvas "Edit template" → builder.

---

## 23 — Fidelity & open questions

What the prototype decides, what it fakes, and what is still open. Keeping these separate is the difference between a spec and a screenshot.

> **Decided and demonstrated**
>
> The full graph editor including multi-select, grouping, blocks with exposed parameters, frames, stickies, undo/redo and two-way code sync. The four canvas modes. The hook model — event, matcher, scope, blocking, payload, and the exact file each scope compiles to. Library versioning, drafts, publishing, trust states and lossy-import disclosure. The doc chain with gates, parent-change badges and amendment-by-diff. The board's Plan/Ops split and the two-way work-order link. The waterfall with policy decisions, node jumps and replay. Every refusal and confirmation string.

> **Simulated, on purpose**
>
> Runs, spans and metrics are fixtures — nothing executes. The debugger steps a topological order rather than attaching to a real process, and its payloads are generated from a node-id hash where no fixture exists. Node evals produce deterministic pseudo-random matrices from the prompt text, so the numbers move when the prompt does but mean nothing. Replay waits ~820ms and returns a mutated recording. Compile always produces the same new hash. Backup, restore, tracker tests and GitHub OAuth are toasts.

> **Designed but not wired**
>
> "Connect issue mirror" is a stub — mirror setup has no screen yet. Instance-level model role bindings and the project-level model override exist in state and have no surface. Frames and stickies are not yet carried into the code round-trip, so applying a pasted graph drops them along with block groupings. Version-history "View diff" shows one fixed diff for every row. The block palette carries no delete or version-bump path once published.

> **Open questions worth resolving before build**
>
> **One.** Pushing a project-canvas edit back to the template has no path — the scope note promises it, and nothing implements it. That is the largest gap in the model.
>
> **Two.** Bumping a pipeline's pinned library versions is described everywhere and has no control. Where does "bump to v5" live — the composition table, the node inspector, or a dedicated upgrade review?
>
> **Three.** Approving a non-taskgraph doc is currently local state with no downstream effect. Should approving Invariants unlock its gated edge automatically, or are gate unlocks always a separate deliberate act?
>
> **Four.** Plan and Ops both hold a notion of status. The rule is that neither writes the other, but nothing surfaces a contradiction — an issue closed in git while its work order is still in-flight shows two truths with no reconciliation.
>
> **Five.** Retention compacts span bodies after 30–90 days, but replay and the data-in/out pane read those bodies. What those surfaces show for a compacted span is undefined.

> **Resolutions — 2026-08-08**
>
> **One — resolved: cut template push-back for v1.** Project-canvas edits never flow back to a template. A single **Promote to fork** action snapshots the project's materialized graph as a new named fork under "My forks," reusing existing fork provenance and version history. Template push-back may return later as "diff against upstream template" on a fork.
>
> **Two — resolved: a dedicated upgrade review dialog, launched from the composition table.** It shows the library item's vN→vN+1 diff, lists every affected node, and confirms by producing a new pipeline version — the same amend-by-diff pattern the taskgraph already uses.
>
> **Three — resolved: approval and gate unlock are always two separate acts.** Auto-unlocking would erase who deliberately opened the gate from the audit trail. The approval toast may offer a one-click "Unlock `<edge>` now?" shortcut; it is still its own logged act.
>
> **Four — resolved: surface divergence, never reconcile.** At mirror-sync time, each linked issue↔work-order pair gets a divergence check (e.g., tracker issue closed while the work order is in-flight). Divergent pairs are badged **diverged** on both board halves and appear in the "Needs you" queue; a human resolves by acting on whichever side is wrong.
>
> **Five — resolved: compaction drops bodies, never structure.** Role, timings, status, cost and policy-decision strings are retained forever. The data-in/out pane renders an explicit "body compacted · metadata preserved" placeholder; replay is refused for runs containing compacted spans, with a refusal string in house style.

> **Resolutions — 2026-08-12** *(from the concept audit: four gaps where the model did not close)*
>
> **Six — resolved: Surge spawns headless workers; the "Surge doesn't execute" non-goal is narrowed, not kept.** §06 promised queue policy, max-parallel, automatic retries and wave budgets, but nothing launched a runtime — leases were claimed by workers that appeared from nowhere, and §16's dispatch kind (*interactive session | headless `claude -p`*) already leaked the answer. Resolution: the binary gains a **runtime supervisor** that spawns headless `claude -p` processes, one per dispatched issue holding a lease, up to the parallelism cap; each spawn records run id, materialization hash and work-order hash (INV-EXEC-1). Interactive sessions remain human-launched and simply claim leases; the supervisor never drives them. The non-goal is restated as: Surge never performs the *creative* work — it compiles, dispatches, supervises processes, and observes.
>
> **Seven — resolved: a closed read path from the bound repo, and doc canonicity settled.** Three mechanisms required reads the one-way `compiler → repo` arrow could not supply: the Docs drawer shows document text, work-order dispatch hash-checks `work_orders/`, and wave integration rebases branches. Resolution: the binary gains a **repo I/O** component owning exactly three reads (INV-DATA-6): declared doc paths, `work_orders/`, and git state. Doc canonicity: after a doc node's run, the *repo file* is canonical; repo I/O ingests and hashes it, and Surge's stored copy is the projection the drawer and gates read. This also settles what the runtime-token *fetch pipeline* endpoint is for: the compiled `.claude/` files on disk are the pipeline as far as the runtime's tooling is concerned — the fetch endpoint's real payload is the session's work order, lease assignment and materialization hash, which is what Phase 0 must prove.
>
> **Eight — resolved: hash inputs are defined by rule (INV-ID-2).** Semantic content only — nodes, edges, prompts, gates, fanout, pinned references. Positions, frames, stickies and collapse state never enter the hash; the known-lossy annotation round-trip therefore cannot break canvas↔code hash fidelity.
>
> **Nine — resolved: project-local edits get an identity the moment they exist (INV-ID-3).** Between an edit and a promote-to-fork, the project's graph was an unnamed divergence while the UI still claimed "v14". Resolution: the first local edit creates a project-local revision with its own content hash; the assignment line reads `v14 + local rev 9c4e…`, runs record that hash, and promote-to-fork adopts it as the fork's v1 provenance.

> **Resolutions — 2026-08-12, second audit** *(mechanics assumed but unowned: isolation, credentials, git policy, trust of self-reported facts)*
>
> **Ten — resolved: one worktree per lease (INV-EXEC-2).** "Max 3 parallel" workers in one repo would clobber a shared working directory, and wave integration had no defined place to rebase *from*. Resolution: claiming a lease creates a git worktree on the issue's task branch; the supervisor spawns the worker inside it, the materialization is compiled into it, and the worktree is reaped when the lease ends (merged, aborted, or reclaimed). Wave integration rebases the task branches those worktrees produced.
>
> **Eleven — resolved: token delivery is spawn-injection or `surge auth`, never the repo (INV-AUTH-4).** `surge.yaml` is "never secrets," so the runtime token had no path to the runtime. Resolution: headless workers receive it as an environment variable at spawn; interactive sessions run `surge auth` once, which stores it in machine-local config outside any repo. No token ever appears in a bound repo or a compiled file.
>
> **Twelve — resolved: three git policies for the four write kinds (INV-DATA-7).** Committed or ignored was unstated, and the wrong uniform answer breaks either wave rebase (generated-file churn) or fresh worktrees (no materialization). Resolution: `surge.yaml` is committed (it is the binding); pipeline-declared docs are committed (they are the deliverable); compiled `.claude/` files and `work_orders/` are **gitignored** — reproducible from the materialization hash, compiled into each worktree (Ten), never merge-conflict material. The compiler maintains the ignore entries inside a marked surge-managed block.
>
> **Thirteen — resolved: orchestration transitions come only from Surge-observed facts (INV-EXEC-3).** Verify pass/fail and cost arrived as spans appended under the same token the worker holds — the machinery branched on data the supervised side wrote. Resolution: state transitions (verified, failed, retry, budget breach) derive from what Surge observes — process exit codes, deterministic stage results, supervisor-side metering. Span content is observability, never control flow. A run whose cost cannot be metered is treated as over-cap: paused behind the same required gate as a budget breach, refused loudly.
>
> **Fourteen — resolved: two run kinds, one supervisor.** §06's lifecycle starts at the approved taskgraph, but doc nodes also execute ("Run the pipeline", §13) with no issue, wave or budget behind them. Resolution: **doc runs** (human-triggered, single node or chain segment, per-run cap only) and **work-order runs** (issue-backed, the full §06 lifecycle) are named kinds on the Run entity. Both go through the supervisor, hold leases, and appear in the Observatory; eligibility, waves and budgets apply to work-order runs only.
>
> **Fifteen — resolved: backup is the third external write path, named on the diagram.** `surge-state.git` appeared in copy but on no diagram, with unnamed credentials. Resolution: the backup remote is an operator-configured git remote, credentialed Surge-side like the trackers; it appears in the architecture as an explicit edge. Tokens are never included in a backup; restore re-mints every runtime token and requires a fresh human session claim (Sixteen).
>
> **Sixteen — resolved: the session token has a birth story (INV-AUTH-5).** Rotation was specified everywhere, issuance nowhere — and on loopback, "whoever curls first" included spawned workers. Resolution: first launch (and every restore or rotation-to-zero) prints a one-time claim URL to the terminal; visiting it binds that browser and mints the session token. Reaching the port never grants auth by itself.
>
> **Seventeen — resolved: "fork, never edit in place" means published versions are immutable.** Blessed templates carry version history (v13→v14), which contradicted a literal reading. Restated (INV-DATA-3 amended): a published pipeline *version* is immutable; the pipeline advances by publishing vN+1 from the builder; forking is divergence without advancing the original. Ratchets apply against the next published version of whichever pipeline they were recorded on.
>
> **Eighteen — planned: a Claude Code plugin, speaking MCP, is the primary runtime integration.** The integration story was raw HTTP calls from compiled hook scripts — workable but fragile (shell glue, no typed contract). Resolution: Surge ships a Claude Code plugin (`integrations/claude-plugin/`) bundling an MCP server that exposes the five runtime-token capabilities as typed MCP tools (fetch work order/lease · claim · heartbeat · append spans · status poll), plus the hooks that emit spans and enforce guards. The compiled `.claude/settings.json` registers the plugin's MCP server instead of bespoke curl glue; the hook-script path remains the fallback for runtimes without MCP. This is also the template for the post-V3 Cursor/Codex adapters — same MCP server, different host registration.

> **Resolutions — 2026-08-23, feature-coverage audit** *(two mechanisms the surfaces assumed but no section owned)*
>
> **Nineteen — resolved: egress enforcement is two-tier, and the capability report says which tier each line is.** INV-DEPLOY-1 promised "no network unless allowlisted, checked at dispatch," but checking a list does not deny a socket. Resolution: **enforced** — processes Surge itself spawns (stage commands, headless workers) run inside an OS-level network sandbox permitting only loopback `:7420` plus the allowlist (macOS Seatbelt profile first; where no sandbox facility exists the tier degrades *visibly* to declared). **Declared** — hook scripts execute inside the runtime's process tree where Surge cannot interpose; their egress posture is compiled into the runtime's own permission settings and audited, not enforced by Surge. The §04 capability report labels each line `enforced` or `declared` rather than implying a uniform guarantee.
>
> **Twenty — resolved: the built-in tracker is writable Plan, not a mirror of nothing.** Plan is a "read-only mirror of the repo's tracker" — but when the tracker *is* Surge's built-in one, no surface anywhere could create an issue. Resolution: with tracker = built-in, issues live in Surge's own store and Board·Plan gains create/edit for them; the sync chip reads `built-in · live`. INV-DATA-5 survives intact: read-only-mirror is a rule about *external* trackers, and the Plan↔Ops boundary is about *status* — editing a built-in issue writes a tracker record, never orchestration status, and the divergence check applies unchanged.

> **Twenty-One — planned (2026-08-23): a model provider registry for custom APIs — OpenAI-compatible, DeepSeek, and friends.** Every model reference in the product (subagent model select, routing fallback, role bindings) silently assumed Anthropic. Resolution: instance settings gain a **MODEL PROVIDERS** card — named providers with kind, base URL, Surge-side API key, and declared models; every model reference becomes provider-qualified (`anthropic/…`, `deepseek/…`). The honesty constraint: **Surge never calls a model itself — the runtime does** — so a provider works only if the runtime can speak to it. Three kinds, stated on the row: `anthropic` (built-in default), `anthropic-compatible` (providers exposing an Anthropic-format endpoint, e.g. DeepSeek — wired by injecting base-URL + key env vars at worker spawn), and `openai-compatible via proxy` (OpenAI-format APIs reached through a local translation proxy such as LiteLLM, which the provider row points at; Surge does not ship the proxy in v1). Keys follow the runtime-token discipline (INV-AUTH-6): spawn-time env injection, never a repo, compiled file, or backup. Provider hosts are auto-appended to the *worker* sandbox allowlist and shown on the capability report's egress line — a model provider is egress, and the report must say so. Landing: registry data model + spawn injection + provider-qualified routing in Phase 2 (with the routing-fallback behaviour); the settings card in Phase 3; per-provider cost normalization stays post-V3 (metering treats unpriced providers as unmeterable — the INV-EXEC-3 over-cap path, not a silent $0).

*Local-only · single-user · token-scoped loopback API · specified against Surge App v9*
