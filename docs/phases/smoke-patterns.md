# Smoke recurring-patterns table (cumulative)

| First seen | Pattern | Guard candidate |
|---|---|---|
| 2026-08-25 (phase-0, F1/F3) | Relative default paths in config resolve against different bases per consumer (server cwd vs `git -C` repo); e2e tests masked it by always overriding with absolute tempdirs | Absolutize all path config at construction; keep one e2e on true defaults |
| 2026-08-25 (phase-0, F2) | External CLI invocation shape assumed, never validated against the real binary version (variadic flag swallowed a positional) | Any spawn of a third-party binary gets one manual/smoke validation of the exact default argv |
| 2026-08-25 (phase-0, F4) | Child stderr discarded → root cause unobservable, violating "refusals are data" in spirit | Pipe + tail child stderr into the failure span by default |
| 2026-08-25 (phase-0, F5/F6) | Checklist lines referencing surfaces/behaviour the phase never shipped | prd-readiness/taskgraph step: every Done-when line names its exercisable surface |
