# Area: server

The local Surge service — API, database, execution lifecycle (runs, spans, leases, work orders), materialization compiler. Not yet implemented; see `docs/design.md` §03–§06 for the object model and lifecycle.

Append date+branch-stamped sections below. Never overwrite existing sections.

## 2026-10-12 · main · roadmap replan

Current scope/order and binding contracts: `docs/phases/README.md`; next is 0.1 runtime/browser acceptance. Old plans archived, ledger reconciled by append-only PR evidence. Semantic edits persist on completed mutation; Blocks remain semantic, groups presentation. Graph execution/attribution/per-run cap is 1.2; SSE/queue/network policy 2.2; full authoring pages 3.1. Existing UI still has no canvas. Current egress report says not enforced; declared capability metadata does not enforce worker permissions.

Packaging clarification: embedded UI/plugin files do not include the MCP interpreter. Current compiled MCP command requires Node.js; 0.1 inventories Codex/git/hook dependencies and makes no static-linking promise.

## 2026-10-12 · main · runtime investigation checkpoint

Resume from `docs/phases/phase-0.1/investigation-2026-10-12.md`. CLI ordering, explicit private auth, canonical trust, headless MCP approval, vetted hooks and scoped Git permissions have working-tree fixes. Real work-order commit and timed abort/token 401 passed. Unresolved: CodexHome guard follows stderr EOF instead of child lifetime; doc timestamp snapshot precedes supervisor writes and can falsely pass. Final suite and real doc/silence/MCP-failure/browser acceptance remain; 0.1 is in progress, not accepted.
