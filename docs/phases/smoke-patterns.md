# Smoke recurring-patterns table (cumulative)

| First seen | Pattern | Guard candidate |
|---|---|---|
| 2026-08-25 (phase-0, F1/F3) | Relative default paths in config resolve against different bases per consumer (server cwd vs `git -C` repo); e2e tests masked it by always overriding with absolute tempdirs | Absolutize all path config at construction; keep one e2e on true defaults |
| 2026-08-25 (phase-0, F2) | External CLI invocation shape assumed, never validated against the real binary version (variadic flag swallowed a positional) | Any spawn of a third-party binary gets one manual/smoke validation of the exact default argv |
| 2026-08-25 (phase-0, F4) | Child stderr discarded → root cause unobservable, violating "refusals are data" in spirit | Pipe + tail child stderr into the failure span by default |
| 2026-08-25 (phase-0, F5/F6) | Checklist lines referencing surfaces/behaviour the phase never shipped | prd-readiness/taskgraph step: every Done-when line names its exercisable surface |
| 2026-08-25 (browser pass, bind seam) | Parallel agents split across an API seam: the UI was written against the endpoint set that existed at dispatch time while the endpoint it needed landed concurrently; both test suites passed because neither crossed the seam | When waves split UI from API, one post-merge test (or walk) must exercise the seam end to end — per-agent green is not integration green |
| 2026-08-25 (browser pass, span labels) | An observability surface rendered ids where humans needed content, with the content in a tooltip — invisible to every non-interactive check | Any list of records shows its human-readable field first; curl-only verification cannot catch this |
| 2026-08-25 (walk 3, N1/N6/N13) | A fix applied to one of two sibling code paths — the same refusal/terminalization reached by a different token kind or run kind | Refusal and terminalization records are built by ONE shared function both the human and runtime APIs call; never hand-write the second copy |
| 2026-08-25 (walk 4, S1) | A "makes a fresh clone work" fix shipped as a single tracked empty file — invisible to every existing checkout and silently deletable by an unrelated `git add -A` | Such fixes get a structural guard (build.rs, generated dir) plus a clean-clone build check — never rely on an empty file surviving |
| 2026-08-25 (walk 4, S2) | A credential minted per-run but stored per-project outlived every lifecycle path that released its lease, because nothing bound it to the run | Whatever ends a unit of work revokes its credential in the same place; bind credentials to the thing they authorize |
| 2026-08-25 (walk 4, S5) | A checklist line describing capability the phase never built survived four walks by being adjacent to something that did work | Every Done-when line names the exercisable surface that proves it; a line unproven twice gets amended or scheduled, never re-walked unchanged |

