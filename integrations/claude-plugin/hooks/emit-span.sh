#!/bin/sh
# Surge span emission (PostToolUse) — the raw-HTTP fallback glue for the MCP
# span tool (ADR-8: hook-script glue is the fallback for MCP-less runtimes).
# Reads the hook payload on stdin, appends a worker span naming the tool.
set -eu
[ -n "${SURGE_RUN_ID:-}" ] || exit 0
API="${SURGE_API:-http://127.0.0.1:7420}"
payload=$(cat)
tool=$(printf '%s' "$payload" | sed -n 's/.*"tool_name":"\([^"]*\)".*/\1/p')
[ -n "$tool" ] || tool="unknown"
span_id="sp_$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
now=$(($(date +%s) * 1000))
curl -sf -X POST \
  -H "Authorization: Bearer ${SURGE_RUNTIME_TOKEN:-}" \
  -H "Content-Type: application/json" \
  -d "{\"id\":\"$span_id\",\"run_id\":\"$SURGE_RUN_ID\",\"parent_span_id\":null,\"node_id\":null,\"role\":\"worker\",\"started_at\":$now,\"duration_ms\":null,\"status\":\"ok\",\"cost\":0,\"depth\":0,\"policy_decision\":null,\"body\":\"tool: $tool\"}" \
  "$API/runtime/runs/$SURGE_RUN_ID/spans" >/dev/null || true
exit 0
