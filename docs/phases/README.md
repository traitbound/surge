# Surge roadmap

**Replanned 2026-10-12.** Product scope remains local, single-user, one Rust service and one embedded SQLite store. Distribution embeds UI/plugin assets, while worker integration still needs Codex, Node.js, git and hook utilities. This roadmap replaces inherited week estimates and Phase 2's broad batch with independently demonstrable milestones. Phase documents own scope and acceptance; the manifest projects their order/status. Existing August acceptance evidence is preserved.

## Current evidence

- Phase 0 accepted on 2026-08-28 with recorded exceptions, using real Claude workers. The October Codex migration has not had equivalent real-runtime/browser acceptance.
- The code contains binding, compilation, single-worker supervision, scoped runtime tools and basic registry/run inspection. Assignment and canvas specs are drafts; preparatory ESC-1 through ESC-5 fixes are in Git, not shipped canvas functionality.
- The 2026-10-12 baseline review ran 117 Rust tests and the UI production build successfully. Actuator tests substitute shell workers, so that result does not establish Codex acceptance.
- The [2026-10-12 real Codex/browser walk](phase-0.1/smoke-2026-10-12.md) is NO-GO: the default worker argument order is rejected by Codex 0.160.0. Binding, compilation, visible failure, failed-startup drain and stale-materialization refusal passed; successful runtime cases remain unproven.
- Latest [investigation checkpoint](phase-0.1/investigation-2026-10-12.md): startup fixes enabled a real Codex task commit and timed abort/token revocation. Phase 0.1 is in progress; review findings, adverse walks and final checks remain open. The earlier CLI failure above is historical, not the current blocker.
- No calendar delivery commitment is implied. Estimate one acceptance slice after grounding it against code; report evidence and unresolved risk before scheduling the next.

## Execution order

| Phase | Observable outcome | Dependency |
|---|---|---|
| [0.1](phase-0.1/phase.md) | Real Codex and browser foundation revalidated; baseline checks established | historical 0 |
| [1.1](phase-1.1/phase.md) | Assign, mutate, round-trip, fork and compile one durable graph | 0.1 |
| [1.2](phase-1.2/phase.md) | Graph drives implement → verify → bounded retry, with node attribution | 1.1 |
| [1.3](phase-1.3/phase.md) | Reviewed reusable library and truthful compile governance | 1.2 |
| [2.1](phase-2.1/phase.md) | Approved taskgraph → reviewed work orders and minimal Ops | 1.3 |
| [2.2](phase-2.2/phase.md) | Bounded parallel scheduling, recovery, metering and live state | 2.1 |
| [2.3](phase-2.3/phase.md) | Recoverable wave integration and reviewable PR | 2.2 |
| [3.1](phase-3.1/phase.md) | Full authoring pages, graph modes and semantic Block UX | 2.3 |
| [3.2](phase-3.2/phase.md) | Observatory, Docs, COE/ratchet and replay/debugger | 3.1 |
| [3.3](phase-3.3/phase.md) | Retention, measured metrics, settings and backup/restore | 3.2 |
| [3.4](phase-3.4/phase.md) | Built-in Plan and read-only GitHub mirror/divergence | 3.3 |

## Binding contracts

1. Assignment first repairs legacy placeholder identity metadata with an audited idempotent migration that preserves graph and recorded run/materialization hashes, before exposing hash-bearing reads. Revisions owns graph mutation/DTO; canvas consumes it. Project edits commit on completed mutation. Semantic edits atomically create an identified revision, stale materialization and audit; presentation edits persist without rehashing or staling. Pending/failed save is explicit and prevents compile/dispatch; no successful saved state is implied until server acknowledgement. Undo/redo is another mutation, never history rewriting.
2. Blocks are semantic nodes: membership and exposed parameters are hashed; collapse/layout are not. Visual grouping is separate presentation state. An unchanged block round-trip preserves its hash; creating or changing a block need not. Unsupported Block execution is refused until expansion is proven.
3. Authorability is distinct from executability. Six-kind canvas support does not promise six-kind sequencing. The 1.2 executor validates and freezes its supported plan, attributes node attempts and chooses edges from observed outcomes.
4. Network and tool reports state actual enforcement. The current worker has network access enabled and no Surge allowlist enforcement. Library `tools:` frontmatter is declared metadata, not an enforced Codex restriction.
5. Metering absence is an explicit gate in new graph execution; arbitrary $0 and implicit unlimited automation are forbidden. Per-run proof comes in 1.2 before wave budgets in 2.2.
6. Codex is primary. Supported providers must be verified against that runtime. The old Anthropic-default/proxy plan is superseded; proxy providers, additional runtimes, Linear, calibrated evals and synthetic EVAL/context bars remain deferred.

## Ownership changes and history

Old library epic 1.2 becomes 1.3. Old authoring surfaces 1.3 become 3.1. Minimal graph execution, node attribution, authoritative work-order fetch and per-run cap proof move forward from Phase 2 into 1.2. Phase 2 splits into approval/Ops, scheduling/policy and integration. External mirrors move to 3.4. Full Docs, observatory and operations remain Phase 3 but have separate acceptance units.

Exact pre-replan docs are in [the archive](archive/2026-10-12/README.md). The four original ledger events are unchanged; reconciliation appends evidence-linked completion events for landed ESC fixes, without pretending the authoring phase is accepted. The reconciliation rows are timestamped when observed and carry the actual `merged_at` timestamps. The generic ledger rollup therefore includes reconciliation delay in its cycle-time figures; do not use those figures as development-duration measurements. Historical timing must use the recorded claim and verified merge times, with missing start evidence explicit. Old spec references to `pipeline-versioning` resolve to assignment, revisions or promote-to-fork; there is no active combined versioning feature.

## Next action

Execute [phase-0.1](phase-0.1/phase.md) as bounded slices. Record real Codex and browser evidence before starting 1.1 implementation. This replan creates acceptance requirements; it does not claim those future features or acceptance walks have happened.
