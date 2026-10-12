# Phase 0.1 investigation and restart handoff — 2026-10-12

**Verdict: IN PROGRESS / NOT ACCEPTED.** Resume here before running more paid model calls. This supersedes the original startup diagnosis, while preserving [the initial NO-GO walk](smoke-2026-10-12.md) as historical evidence. The user requested saving findings for a new chat; investigation stopped at this checkpoint.

## Current state

Base HEAD: `210b2114a4b10bad7236431864724ce78eb02434`. Roadmap revisions, runtime fixes, regression tests and CI workflow are **uncommitted working-tree changes**. Do not reset or discard them. Current active phase is 0.1; 1.1 implementation must wait for acceptance. No commit, push, hosted CI success or phase closure is claimed.

The initial CLI failure concealed several independent integration defects. Fixing only the argument order was insufficient.

| Finding | Working-tree change | Verification / limit |
|---|---|---|
| Codex 0.160.0 rejects approval flags after `exec` | Global flags precede `exec`; stdin prompt is final `-` | Installed CLI help compatibility test passed |
| Isolated `CODEX_HOME` has no operator login | Optional `--codex-auth-file` copies explicitly supplied cached auth into an owner-only temporary home | Real unauthenticated request failed 401; explicit auth enabled real execution; source credentials untouched |
| macOS `/tmp` alias does not match canonical project trust | Canonicalize trusted project path | Real MCP inventory differed between alias and canonical paths; canonical path loaded Surge |
| MCP `auto` approval conflicts with headless `never` | Compiler emits Surge MCP approval mode `approve` | Real worker reached scoped tools; compiler regression passed |
| Workspace sandbox cannot write shared worktree Git metadata | Named `surge` permissions extend read-only, permit worker files and necessary task Git paths | Real sandbox task commit passed; main branch ref write denied; egress remains unenforced |
| Hook trust is independent of project trust | Rematerialize approved runtime before launch; use documented automation hook trust bypass | Real Pre/Post tool spans and heartbeat observed; hook replacement regression passed |
| Doc worker was not told declared output; exit 0 could mean no document | Prompt includes declared paths/agent/skill; monitor checks file modification | Regression added; **real doc rerun still pending and review found guard weaknesses below** |

Primary code: `crates/server/src/supervisor.rs`, `crates/server/src/main.rs`, `crates/compiler/src/emit.rs`. `tempfile` is now a server runtime dependency. New optional installed-runtime tests: `crates/server/tests/codex_cli.rs` and supervisor sandbox test. CI draft: `.github/workflows/checks.yml` (offline Rust + UI, no credentials/model calls).

## Verified real-runtime results

- Work order `run_aceede6f2539` succeeded using Codex 0.160.0. Scoped fetch/span/heartbeat and hook tool spans were observed. Task commit: `d2cf567756f3065149ccfe3545894b104115e3a2`, containing `acceptance.txt` with `verified`. Disposable main remained `6446d525f2badcf7f137f1b01f16db63d1aff973`.
- Timed abort `run_db575aa3d5e7` passed: waited for readiness, aborted before a delayed forbidden write, released lease, verified forbidden file absent, and replayed its runtime token returned HTTP 401. Sanitized result is retained in [evidence/2026-10-12/abort-result.json](evidence/2026-10-12/abort-result.json).
- Missing credentials failed visibly with HTTP 401; earlier argument parse and sandbox failures also produced inspectable failed runs. Initial failed-startup drain and stale-materialization refusal passed.
- `run_b62967ffe976` is **not doc acceptance**: it was marked succeeded but produced no declared output. It triggered the current doc fix.
- `run_4f6ece598c56` finished before the attempted abort. It is **not abort evidence**; use the timed case above.
- Original browser walk covered claim/bind/compile/dispatch, visible failure and stale refusal. Browser success/abort/doc follow-up and full exception discharge remain pending.

## Review findings — fix these first

Independent read-only reviewer completed; no reviewer files or model calls were made.

1. **P1: temporary auth ownership follows stderr EOF, not child lifetime.** `spawn_worker` moves `CodexHome` into a detached stderr drain (around supervisor.rs:744). Early fd 2 closure deletes runtime config/auth while the worker is alive; descendants retaining stderr preserve credentials after worker exit/kill. Reproduction: `sh -c 'sleep 2 & exit 0'` exits its parent immediately while stderr remains open. Return the home guard alongside child/tail, let `monitor` own it through child exit/kill, and let the stderr task own only stderr. First add a regression where the worker closes stderr early and then needs its home; also verify descendant-held stderr cannot delay home cleanup. **Not fixed at checkpoint.**
2. **P2: doc snapshot precedes supervisor rematerialization.** Declared paths such as `AGENTS.md`, `.gitignore`, or `surge.yaml` can satisfy the timestamp guard from supervisor writes alone. Snapshot after rematerialization and/or reject compiler-owned outputs. Timestamp-only also accepts touching an unchanged existing document; choose and document a stronger output evidence contract, with regression coverage. **Not fixed at checkpoint.**
3. Additional pre-existing gap: doc token mint uses `?` after inserting a running run. Mint failure can leave that run running. New path validation also occurs after run/token creation; ensure all preparation failures unwind. Reviewer did not elevate the pre-existing issue as a patch finding, but do not claim comprehensive preparation cleanup before addressing it.

## Shortest route to phase closure

1. Fix the two review findings with focused red/green tests; make doc preparation cleanup reliable.
2. Run fresh complete offline Rust checks and UI build against final source. Last full suite log `/tmp/surge-final-runtime-tests.log` has 121 passed, 2 ignored, 0 failed, but **predates the latest doc changes**. The baseline before runtime fixes was 117. Do not present either as final verification.
3. Restart only the disposable acceptance server with the newly built binary. Verify the listening process command and DB path before stopping it; do not kill unrelated processes.
4. Real doc run: verify declared output exists, sensible content, main HEAD unchanged and runtime token drained. Inspect skills/agent loading evidence, not just exit 0.
5. Real silent lease expiry: get worker readiness, suspend the exact test worker to prevent heartbeats, accelerate only its disposable DB lease deadline, verify failed run, lease release, token 401, no forbidden output and temporary auth cleanup. Record acceleration explicitly.
6. Real MCP startup failure: disposable plugin copy with a failing MCP entry, valid explicit auth, required server failure visible, token/lease drain. Restore normal plugin after the case.
7. Browser follow-up for success/doc/abort/spans, cost n/m, refusals and truthful network report. Discharge or retain prior browser exceptions explicitly.
8. Complete clean-source Rust/UI checks; CI workflow exists but has not run remotely. Update acceptance report/ledger/manifest only when the checklist is actually satisfied. Then 1.1 is next.

## Disposable fixture and process handoff

Fixture: `/tmp/surge-acceptance-20261012-lragtw3k` (macOS canonical `/private/tmp/...`). DB: `state.db`; project: `prj_disposable-acceptance`; pipeline: `pl_two_node_v1`; Git repo: `repo`. Tables are singular (`run`, `issue`, `span`, `token`, `audit_entry`). Latest six run rows are terminal at checkpoint.

A test server may remain listening on `127.0.0.1:7420`, launched with this fixture DB and explicit operator auth file. Its `verified-runtime.log` binary **predates the latest source changes**. Check live PID/command before reuse or restart. Operator auth is read only for runtime use; never print/copy it into evidence, Git or reports.

Fixture API helper `api.py` loads fixture-only human auth from private `api-session`; loopback calls may need sandbox escalation. Do not print tokens. Abort harness `abort-walk.py` and sanitized `abort-result.json` remain in the fixture. It held runtime replay credentials only in memory. Do not copy raw worker logs into repo without sanitizing.

Diagnostic `sandbox-probe` worktree/branch remains in the fixture and should be removed after verification; main was restored to the initial fixture commit. Old probe worktree was removed. Temporary clean source snapshot path is stored in `/tmp/surge-clean-check-path`; this snapshot predates latest edits. UI clean-source build passed after locked dependency install, including generated TS types; `/tmp/surge-clean-ui-build.log`. This was a source export, not a clean committed checkout. Resync final source before fresh checks.

Repo-wide `cargo fmt --check` has a pre-existing baseline failure (361 locations); `/tmp/surge-fmt-baseline.log`. Do not reformat unrelated code to make this phase appear green. Run relevant formatting checks on new changes.

## Commands for final verification

```sh
SQLX_OFFLINE=true cargo test --locked --workspace
SQLX_OFFLINE=true cargo build --locked --workspace
npm run build --prefix ui
SQLX_OFFLINE=true cargo test -p surge-server --test codex_cli -- --ignored
SQLX_OFFLINE=true cargo test -p surge-server task_sandbox_can_commit_but_cannot_write_main -- --ignored
```

The last two invoke the installed CLI/sandbox without paid model requests; the sandbox check is macOS-specific. Paid real acceptance runs remain separate from CI. Existing scopes still apply: no new graph scheduler, canvas or egress enforcement is implemented by these fixes.
