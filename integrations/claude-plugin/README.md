# Surge Claude Code plugin (phase 0 skeleton — ADR-8)

The runtime integration recipe: an MCP server exposing the runtime-token
capabilities as typed tools, plus the two runtime-side hooks. Phase 0 ships
span-append, heartbeat and own-run status poll; fetch-work-order and
claim-lease tools land in Phase 2 with the full surface.

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
