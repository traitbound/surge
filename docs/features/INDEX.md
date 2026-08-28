# Feature index

One row per Layer 4 feature spec.

**Status vocabulary:** `built · no spec` — shipped in the tree, authored code-first without a Layer 4 spec (Phase 0 predates the spec sprint) · `planned` — anticipated in a phase doc, spec not yet authored · `ready` — spec authored and fresh-checked · `shipped` — spec authored and its task work merged.

*(2026-08-28, `/halfcycle:prd-readiness`: backfilled. Phase 0 was built code-first, so this index was empty and Phase 1's spec sprint had no record of what it builds on. Rows below are the two phases' anticipated-spec lists, reconciled against the tree. No specs were written by this backfill — the `Spec` column is empty on purpose.)*

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

## Phase 1 — Author: canvas & library

**Split into three epics 2026-08-28** (`/halfcycle:phase-rescope`; three of four diagnostic questions fired — see `docs/phases/phase-1/phase.md`). Spec sprint not yet run; it runs per epic, starting with phase-1.1.

| Feature | Spec | Phase | Status |
|---|---|---|---|
| canvas-editor | [draft](canvas-editor.md) | phase-1.1 | draft — 13 open blockers, rewrite pending |
| code-roundtrip | — | phase-1.1 | planned |
| pipeline-assignment | — | phase-1.1 | planned |
| pipeline-revisions | — | phase-1.1 | planned |
| promote-to-fork | — | phase-1.1 | planned |
| library-store | — | phase-1.2 | planned |
| trust-and-import | — | phase-1.2 | planned |
| compile-dialog | — | phase-1.2 | planned |
| upgrade-review | — | phase-1.2 | planned |
| default-library | — | phase-1.2 | planned |
| pipelines-pages | — | phase-1.3 | planned |
| project-overview | — | phase-1.3 | planned |
| canvas-modes | — | phase-1.3 | planned |
| blocks-and-groups | — | phase-1.3 | planned |

## Phase 2 · Phase 3

Anticipated-spec lists live in `docs/phases/phase-2/phase.md` and `docs/phases/phase-3/phase.md`. Rows land here when their phase reaches its spec sprint.
