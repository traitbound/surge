# Browser pass — phase 0 UI — 2026-08-25

Closes **F7**, owed since walk 1 (the UI had only ever been verified by curl). Driven in a real browser against a fresh instance at `5d024a5` + UI fixes; a real `claude -p` worker throughout.

## Verdict

The shell, Registry and Observatory are **GO** once three defects found here are fixed (all fixed in `fdc2c75`). No UI-side blocker remains. Two product-level gaps the UI merely surfaces (N3, N6) belong to the supervisor/compiler and are tracked in the walk-3 report.

## Verified in a real browser

| Step | Result |
|---|---|
| Cold open with no session | Claim screen, not the project list (INV-AUTH-5) — copy explains the terminal claim URL |
| Visit claim URL | Session minted, shell renders |
| Global shell (design §07) | 236px card sidebar with right border, project switcher, later-phase entries present but dimmed and phase-badged, toast + dialog layers, `127.0.0.1:7420 · local` footer |
| Vocabulary (INV-NAME-1) | Registry · Pipelines · Library · Observatory · Board — no synonyms |
| Bind | Dialog → `surge.yaml` on disk, `surge_yaml_written=1`, `project.bound` audited, badge flips to `surge.yaml` |
| Compile | Capability report dialog with all four §04 lines + hash + file list, framed as the approval; Accept closes |
| Dispatch | Run appears in Observatory with status pill, kind, hash, cost, Abort |
| Span tree | Live 2s polling; rows show role, body, timing, cost, status; run-level duration correct (1m 33s) |
| Refusal | Refused run rendered with its own pill; graceful "No spans recorded for this run." empty state |
| Abort | Run → ABORTED, §16 semantics banner, toast; worker process exited, lease released, worktree residue zero |
| Dialog dismissal | Escape closes |

## Defects found and fixed (`fdc2c75`)

1. **P1 — the bind dialog never bound.** It called `POST /api/projects` and reported "Project bound", but binding is the second call: no `surge.yaml` reached the repo, `surge_yaml_written` stayed 0, no `project.bound` audit row. The card's own "unbound repo" badge contradicted the success toast. **Cause: a parallel-wave seam** — the UI was authored against the API as it existed while the bind endpoint landed concurrently from another agent; both agents' tests passed independently because neither spanned the gap.
2. **P2 — span rows were unreadable.** Rows led with `node_id ?? id`; since `node_id` is never emitted (walk 3, N3), every row showed an opaque hash while the body sat in a hover tooltip. Rows now lead with the body.
3. **P3 — doubled refusal copy**: "Dispatch refused — dispatch refused — …".

## Open, recorded (not UI blockers)

- **B1 (P3) — no URL routing.** `/observatory` serves the app but lands on Registry; refresh and browser back/forward do not restore the surface. Navigation is state-only. TanStack Router is in the decided stack; this is Phase 1 work, not a defect.
- **B2 (P3) — "Pipeline: not assigned" persists after a successful compile.** Honest (assignment is a separate field phase 0 never sets) but confusing: a project can carry a fresh materialization while reading "not assigned". Decide in Phase 1 whether compile assigns, or the card shows the materialization instead.
- **N3 visible**: every span row shows `—` for duration.
- **N4 visible**: a worker "start" span sits `RUNNING` forever on a succeeded run.
- **N6 visible**: the not-eligible refusal shows "No spans recorded for this run" — the operator gets no reason at all.
