# Surge Claude Code plugin (phase 0 — ADR-8)

The runtime integration recipe: an MCP server exposing the runtime-token
capabilities as typed tools, plus the two runtime-side hooks.

## Tools

Four of INV-AUTH-1's five runtime capabilities, in capability order:

| Tool | Capability | Arguments |
| --- | --- | --- |
| `surge_fetch_work_order` | fetch work order / lease / materialization hash for `$SURGE_ISSUE_ID` | none |
| `surge_append_span` | append spans (observability, never control flow — INV-EXEC-3) | `body` required |
| `surge_heartbeat` | heartbeat the lease for `$SURGE_ISSUE_ID` | none |
| `surge_poll_run` | poll own-run status, so an abort lands at the next tool call (§06) | none |

Every tool is scoped by spawn-time env, never by a model-supplied id: the
issue and run come from `SURGE_ISSUE_ID` / `SURGE_RUN_ID`, and the runtime
token is scoped to its own project and run server-side as well.

The fifth capability, **claim lease**, has no tool here on purpose. Leases are
claimed by human-launched interactive sessions (INV-EXEC-1); a spawned worker
is already holding the lease it was spawned for. Its tool lands in Phase 2
alongside the interactive-session surface.

## How a supervised worker gets it

The Surge supervisor spawns headless workers with `claude -p --mcp-config
.claude/mcp.json` inside the lease's worktree. That compiled `.claude/mcp.json`
(one of the closed-list writes, INV-DATA-1/7) registers this plugin's MCP
server via `$SURGE_PLUGIN_DIR`, and the compiled `.claude/settings.json` wires
the two hooks. All configuration arrives as spawn-time env (INV-AUTH-4):

| Env | Meaning |
| --- | --- |
| `SURGE_API` | Surge's loopback base URL |
| `SURGE_RUN_ID` | this worker's run |
| `SURGE_ISSUE_ID` | the leased issue (absent for doc runs) |
| `SURGE_RUNTIME_TOKEN` | per-project runtime token — env-only, never on disk |
| `SURGE_PLUGIN_DIR` | where this directory lives |

## Interactive sessions

Install as a Claude Code plugin (this directory is a standard plugin layout:
`.claude-plugin/plugin.json` + `mcp/` + `hooks/`); configure the token with
`surge auth` (machine-local config, INV-AUTH-4). Interactive sessions claim
leases; they are never spawned (INV-EXEC-1).

## Fallback: MCP-less runtimes

`hooks/poll-abort.sh` and `hooks/emit-span.sh` are plain POSIX shell + curl
against the runtime API — the documented raw-HTTP glue. The abort guard is
load-bearing either way: it is how an abort lands at the next tool call (§06).
