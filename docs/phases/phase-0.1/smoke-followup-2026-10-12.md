# Phase 0.1 real Codex follow-up — 2026-10-12

**Verdict: evidence gathered; final clean-checkout and hosted-CI gate pending.** This continues the [investigation checkpoint](investigation-2026-10-12.md) and supersedes its list of pending runtime walks. The original [NO-GO walk](smoke-2026-10-12.md) remains the record of the earlier CLI startup failure.

## Source and environment

- Real runtime: `codex-cli 0.160.0`, Node.js, git and the installed shell/curl hook utilities. The fixture is `/tmp/surge-acceptance-20261012-lragtw3k`, with project `prj_disposable-acceptance` and disposable Git repo `repo`. The listener command was checked before each server switch; only that server was stopped. The normal embedded plugin was restored after the failing-MCP case.
- The real runs used the working-tree source based on `210b2114a4b10bad7236431864724ce78eb02434`. A source snapshot was copied byte-for-byte to task worktree `codex/phase-0-1-runtime`; the later review fixes described below are still only in the original checkout because staging/syncing that managed worktree was rejected by the app's automatic approval review after its usage limit was reached. No commit or hosted CI result is claimed here yet.
- The operator explicitly supplied the existing cached Codex login through `--codex-auth-file`. The source credential stayed outside the fixture and repo. The supervisor created owner-only temporary homes, then removed them when the worker exited or was killed. No token or login contents are retained in committed evidence.

## Reviewed fixes and local verification

The auth-home guard now belongs to the run monitor, so early worker stderr closure cannot delete a live worker's login, and a descendant holding stderr open cannot retain it after worker exit. Both regressions failed before the fix and passed afterward. A document run now snapshots declared file **contents after** Surge rematerializes its runtime, rejects symlinks and Surge-owned paths, and requires changed content from the worker. An unchanged file with a new timestamp and a symlink to compiler-owned `AGENTS.md` both reproduced false success before the fix; they now fail. Token mint and all document preparation failures enter the same terminal cleanup path; a mint-failure regression also failed before the change and passed afterward.

Independent read-only review then found two more cleanup paths. Both audit-write failure tests failed before the fix and now pass: a spawned worker is stopped, its run terminalized, its runtime token revoked, and a work-order lease and worktree cleared. A boot-reconciliation test also failed before the fix and now passes: a stale isolated Codex home is deleted, while a locked home owned by another live server is preserved. The lock is held through the run monitor and released on process death. Follow-up review caught the older, lock-free home layout; boot now removes an auth-bearing home with that exact legacy layout too. These crash-recovery fixes were not part of the paid runtime walks above.

`SQLX_OFFLINE=true cargo test --locked --workspace`, the installed Codex CLI parser check, and the macOS task-sandbox check passed before those last two review fixes. Afterward, the Rust workspace build, UI production build, domain/compiler/store suites, server unit tests, the three new cleanup regressions, both auth-home lifetime regressions, and `git diff --check` passed. The full workspace suite still needs rerun on the final source. A synthetic clean source snapshot passed the Rust workspace build, domain/compiler/store tests, and UI build using existing local dependencies. Its `npm ci` could not reach the package registry, and it was not a checkout of a committed task branch. Repository-wide `cargo fmt --check` already fails on 361 pre-existing locations, as recorded in the handoff; this work does not reformat unrelated code.

## Real runtime results

| Case | Observed result |
| --- | --- |
| Work order commit | Earlier `run_aceede6f2539` succeeded with scoped fetch/span/heartbeat and hook tool spans. Commit `d2cf567756f3065149ccfe3545894b104115e3a2` contains `acceptance.txt` with `verified`; disposable main remained `6446d525f2badcf7f137f1b01f16db63d1aff973`. |
| Timed abort | Earlier `run_db575aa3d5e7` was aborted after readiness, released its lease, left the forbidden file absent, and refused runtime-token replay with HTTP 401. |
| Document output | `run_98160ef50d3f` succeeded and wrote the declared `docs/summary.md` (2,918 bytes; SHA-256 in [sanitized results](evidence/2026-10-12/followup-results.json)). The file has the four sections required by the compiled `write-summary` skill, cites existing fixture paths and runtime commands, and was not committed. Disposable main stayed at the same commit and the run token was revoked. Worker spans show direct Surge MCP calls, Pre/Post tool hook spans, and progress through the compiled doc-writer and skill instructions. This proves the instructions were read and applied; the run did not invoke a separate named Codex subagent. |
| Silent lease expiry | `run_e844aeaffeb4` appended a readiness span. The exact worker PID was suspended, then **only this disposable issue's** lease deadline was accelerated from `1791797186199` to `1791796585573` ms. The run failed, lease cleared, token replay returned HTTP 401, temporary auth home disappeared, and the forbidden file was absent. This is an accelerated expiry test, not a ten-minute wall-clock wait. |
| Required MCP startup failure | With a disposable plugin copy whose `mcp/server.mjs` exited immediately, `run_eae0bad6c37c` failed in under one second. Its coordinator error says `required MCP servers failed to initialize: surge`. Lease and token drained and no forbidden output appeared. The normal plugin was restored afterward. |

The [sanitized results](evidence/2026-10-12/followup-results.json) retain identifiers and outcomes without credentials. Raw database, temporary auth and fixture-only scripts remain outside the repository.

## Browser walk and exceptions

The in-app browser showed the original claim/bind/compile/dispatch loop, stale-materialization refusal, truthful network report (`not enforced`; worker network enabled), and `cost n/m` in the [initial walk](smoke-2026-10-12.md). This follow-up showed the Observatory list with successful work-order and document runs, an aborted run, silent-expiry and MCP failures, the older refusal, and `cost n/m` on each. Expanding the successful document displayed Surge MCP calls, hook-generated tool spans and the worker's survey/draft/write progress. Expanding the MCP failure displayed the required-server error. A later attempt to expand the lease row was rejected by the browser tool's automatic approval review with HTTP 429, so its browser detail view is not claimed; the list showed `FAILED` and the persisted run/lease/token records were checked separately.

The initial claim redirect navigation warning remains an explicit browser exception: the following authenticated Registry view and bind/compile/dispatch operations worked, but seamless redirect navigation has not been re-proven. Browser views do not prove token revocation, committed bytes or output authorship by themselves; the fixture checks above supply those facts.

## Remaining gate

Commit the task branch, run the documented checks from a fresh checkout of that commit, observe hosted CI, and record the final verdict in this phase and the manifest. Phase 1.1 remains blocked until that gate passes or a specific exception is accepted with evidence.
