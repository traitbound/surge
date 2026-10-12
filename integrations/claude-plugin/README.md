# Surge runtime integration (Codex primary runtime)

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

The Surge supervisor spawns headless `codex exec` workers in a task worktree
for issue runs or in the bound repo for document runs. The compiled
`.codex/config.toml` registers this directory's required MCP server through
`$SURGE_PLUGIN_DIR`; `.codex/hooks.json` runs the abort and span hooks. Surge
rematerializes those approved files just before launch. Codex loads the
project configuration from a trusted project path in a private per-worker
`CODEX_HOME`. The supervisor owns that temporary home through worker exit and
removes it afterward, including any copied login cache.

An operator who uses a cached Codex login supplies its path to the server as
`--codex-auth-file /path/to/auth.json`. Surge copies it into each private
worker home with owner-only permissions. It never writes the source credential
to a bound repo, compiled file, database, or backup. An absent or invalid
credential fails the real worker visibly. Node.js, git, Codex, and shell/curl
hook utilities must be installed on the worker host; the embedded plugin does
not bundle their interpreters.

Runtime scope arrives as spawn-time env (INV-AUTH-4):

| Env | Meaning |
| --- | --- |
| `SURGE_API` | Surge's loopback base URL |
| `SURGE_RUN_ID` | this worker's run |
| `SURGE_ISSUE_ID` | the leased issue (absent for doc runs) |
| `SURGE_RUNTIME_TOKEN` | per-run runtime token — env-only, never on disk |
| `SURGE_PLUGIN_DIR` | where this directory lives |
| `CODEX_HOME` | private worker trust/config directory, removed after exit |

## Interactive sessions

The directory retains its historical name and Claude plugin metadata, but
Codex is the primary supervised runtime. Interactive sessions are a separate
path: `surge auth` stores a machine-local session token (INV-AUTH-4), and an
interactive claim-lease tool is planned for Phase 2.2. Surge does not spawn
interactive sessions (INV-EXEC-1).

## Fallback: MCP-less runtimes

`hooks/poll-abort.sh` and `hooks/emit-span.sh` are plain POSIX shell + curl
against the runtime API — the documented raw-HTTP glue. The abort guard is
load-bearing either way: it is how an abort lands at the next tool call (§06).
