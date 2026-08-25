#!/bin/sh
# Surge abort guard (PreToolUse) — this hook is HOW an abort lands at the
# worker's next tool call (§06, INV-AUTH-1 capability 5). It also heartbeats
# the lease while it's here, so ordinary tool activity keeps the lease alive.
# Loopback to Surge is always allowed (INV-DEPLOY-1 exemption).
set -eu
[ -n "${SURGE_RUN_ID:-}" ] || exit 0        # not a Surge-supervised session
API="${SURGE_API:-http://127.0.0.1:7420}"
AUTH="Authorization: Bearer ${SURGE_RUNTIME_TOKEN:-}"

if [ -n "${SURGE_ISSUE_ID:-}" ]; then
  curl -sf -X POST -H "$AUTH" "$API/runtime/issues/$SURGE_ISSUE_ID/heartbeat" >/dev/null || true
fi

status=$(curl -sf -H "$AUTH" "$API/runtime/runs/$SURGE_RUN_ID" | sed -n 's/.*"status":"\([a-z_]*\)".*/\1/p')
if [ "$status" = "aborted" ]; then
  echo "Surge: this run was aborted — stop all work immediately." >&2
  exit 2   # blocks the tool call and surfaces the reason
fi
exit 0
