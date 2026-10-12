# ENGAGEMENT.md

The operational decisions this project runs on. Settled once at setup (`/halfcycle:engagement-setup`); every later runbook assumes them. One line of rationale each — link out for detail, never duplicate a policy stated elsewhere.

## Decisions

| # | Decision | Choice | Rationale |
|---|---|---|---|
| 1 | Project type | greenfield | Empty repo; only `Design.pdf` and `docs/design.md` (V3 product spec) exist. Concept is directionally set but not complete in docs. |
| 2 | Repo structure | monorepo (workspaces) | Surge is a local service + UI (+ likely CLI); explicit packages from day one avoids a painful later split. |
| 3 | Document canonical home | repo-canonical | Everything under `docs/` in this repo; the vault holds only a project overview/pointer. One source of truth. |
| 4 | Push policy | hybrid | Docs and trivial fixes direct-to-main; agent task work lands via PR so a fresh reviewer gates code. |
| 5 | Branch policy | per-task worktree — `BASE_BRANCH=main` | Default for orchestration; workers get isolated worktrees off main. |
| 6 | CI strategy | no CI yet | Establish in phase 0.1; current verification is local build/typecheck/tests. Agent scope: broad allowlist inside the repo, no network installs mid-task, 45-min worker budget. |
| 7 | Ceremony tier | Tier 0 | Solo operator (1 person); no approval gates, phase-close checkpoint as a personal habit. |

## Approval gates in force (per tier)

Tier 0 — none. Phase-close checkpoint is a personal habit: before closing a phase, re-read the phase checklist and record a dated note below.

## Tracker

GitHub remote exists (`traitbound/surge`); no dedicated task tracker is configured. Labels to scaffold when adopting a tracker:
`area:*` (one per code-map area) · `phase:*` · `role:mechanical` / `role:critical` · `blocked-on-human` · `needs-input`.

## Notes

- 2026-08-08 — Engagement settled at setup. Repo git-initialized (`main`). Concept doc (`docs/design.md`) is V3 page-by-page spec derived from the V9 prototype; open questions live in its §23. CI decision to be revisited at first phase close.

- 2026-10-12 — Roadmap replanned from current code/evidence; see `docs/phases/README.md`. Use bounded slices by default (`mode: quick`) and escalate only where a new subsystem needs architecture. Retain Tier 0 and the existing push policy; phase acceptance requires observed evidence, not draft status. CI remains unbuilt and is owned by 0.1. No new deadline or external write authorization is implied.

- 2026-10-12 — Baseline CI workflow drafted at `.github/workflows/checks.yml`; local checks have run, but the final source and hosted CI are not yet verified. Runtime investigation checkpoint: `docs/phases/phase-0.1/investigation-2026-10-12.md`. Phase 0.1 remains open.
