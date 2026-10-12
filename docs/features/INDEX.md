# Feature index

**Updated 2026-10-12.** [Roadmap](../phases/README.md) owns milestone order; the manifest projects phase status. `built · no spec` means existing code without a Layer 4 feature spec; `planned` means owning phase identified with no authored feature spec; `draft` means authored, review still required; `ready` means reviewed and buildable; `shipped` requires implementation evidence. A status never implies runtime acceptance.

Phase 0 has historical Claude acceptance. Codex acceptance is pending in 0.1. ESC-1 through ESC-5 are merged substrate fixes, not completed canvas/assignment features. Their ledger entries are reconciled with verified PR/Git evidence.

## Phase 0 — The materialization loop

**Accepted 2026-08-28** (smoke walk 7, GO, SHA `285b54f`, with exceptions — see the phase doc). Built code-first; `docs/phases/phase-0/phase.md` is the authority on which parts exist.

| Feature | Spec | Phase | Status |
|---|---|---|---|
| workspace-scaffold | — | phase-0 | built · no spec |
| domain-model | — | phase-0 | built · no spec |
| store-layer | — | phase-0 | built · no spec (commit-broadcast deferred — ADR-3, phase-2) |
| token-boundary | — | phase-0 | built · no spec |
| project-binding | — | phase-0 | built · no spec |
| compiler-core | — | phase-0 | built · no spec (egress tiering + project allowlist deferred to phase-2) |
| runtime-api | — | phase-0 | built · no spec (four of five capabilities; claim-lease is phase-2) |
| claude-plugin-mcp | — | phase-0 | built · no spec (claim-lease tool is phase-2) |
| supervisor-minimal | — | phase-0 | built · no spec (cost metering unwritten — INV-EXEC-3, phase-2) |
| cli-thin | — | phase-0 | built · no spec |
| minimal-shell-ui | — | phase-0 | built · no spec |
| default-library-seed | — | phase-0 | built · no spec (full 7·6·7 set is phase-1) |

## Active roadmap features

### [phase-0.1 — Revalidate the Codex foundation](../phases/phase-0.1/phase.md)

| Feature | Spec | Status |
|---|---|---|
| codex-runtime-acceptance | — | planned |
| foundation-browser-acceptance | — | planned |
| baseline-checks | — | planned |

### [phase-1.1 — Minimal faithful authoring](../phases/phase-1.1/phase.md)

| Feature | Spec | Status |
|---|---|---|
| pipeline-assignment | [pipeline-assignment](pipeline-assignment.md) | draft — replan contract amended; review required |
| pipeline-revisions | — | planned |
| promote-to-fork | — | planned |
| canvas-editor | [canvas-editor](canvas-editor.md) | draft — replan contract amended; review required |
| code-roundtrip | — | planned |

### [phase-1.2 — One graph-driven Codex run](../phases/phase-1.2/phase.md)

| Feature | Spec | Status |
|---|---|---|
| graph-execution-contract | — | planned |
| node-execution | — | planned |
| node-attribution | — | planned |
| sequential-control | — | planned |
| refusal-integrity | — | planned |

### [phase-1.3 — Library, trust and compile governance](../phases/phase-1.3/phase.md)

| Feature | Spec | Status |
|---|---|---|
| library-store | — | planned |
| trust-and-import | — | planned |
| compile-dialog | — | planned |
| upgrade-review | — | planned |
| default-library | — | planned |

### [phase-2.1 — Approved taskgraph and Ops](../phases/phase-2.1/phase.md)

| Feature | Spec | Status |
|---|---|---|
| repo-io | — | planned |
| taskgraph-to-issues | — | planned |
| work-orders | — | planned |
| board-ops | — | planned |

### [phase-2.2 — Bounded scheduling and live state](../phases/phase-2.2/phase.md)

| Feature | Spec | Status |
|---|---|---|
| dispatch-queue | — | planned |
| lease-recovery | — | planned |
| budgets-metering | — | planned |
| sse-streaming | — | planned |
| runtime-policy | — | planned |

### [phase-2.3 — Wave integration](../phases/phase-2.3/phase.md)

| Feature | Spec | Status |
|---|---|---|
| wave-assembly | — | planned |
| integration-recovery | — | planned |
| wave-pr | — | planned |

### [phase-3.1 — Authoring surfaces](../phases/phase-3.1/phase.md)

| Feature | Spec | Status |
|---|---|---|
| pipelines-pages | — | planned |
| project-overview | — | planned |
| canvas-modes | — | planned |
| blocks-and-groups | — | planned |

### [phase-3.2 — Observatory and improvement loop](../phases/phase-3.2/phase.md)

| Feature | Spec | Status |
|---|---|---|
| observatory-waterfall | — | planned |
| coe-and-ratchet | — | planned |
| docs-surface | — | planned |
| replay-debugger | — | planned |

### [phase-3.3 — Operations and retention](../phases/phase-3.3/phase.md)

| Feature | Spec | Status |
|---|---|---|
| retention | — | planned |
| metrics | — | planned |
| settings-operations | — | planned |
| state-copy-pass | — | planned |

### [phase-3.4 — Plan board and external mirrors](../phases/phase-3.4/phase.md)

| Feature | Spec | Status |
|---|---|---|
| built-in-plan | — | planned |
| tracker-mirror | — | planned |
| plan-ops-divergence | — | planned |

## Deferred scope

EVAL execution/context-budget bars, calibrated quality metrics, additional runtime adapters, proxy provider bridges, Linear implementation, full frames/stickies text fidelity, scheduled dispatch and OTel export remain recorded backlog rather than milestones blocking the usable pipeline. See design §23 and the product non-goals. The old combined pipeline-versioning spec remains superseded; its active owners are assignment, revisions and promote-to-fork in 1.1.
