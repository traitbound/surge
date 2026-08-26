#!/usr/bin/env node
// Surge MCP server (ADR-8): four of INV-AUTH-1's five runtime capabilities as
// typed tools — fetch work order, heartbeat, append spans, poll own-run status.
// Zero dependencies: newline-delimited JSON-RPC over stdio, global fetch
// (Node >= 18).
//
// Configuration is spawn-time env, injected by the Surge supervisor
// (INV-AUTH-4): SURGE_API, SURGE_RUN_ID, SURGE_ISSUE_ID, SURGE_RUNTIME_TOKEN.
// The fifth capability — claim lease — has no tool here on purpose: leases are
// claimed by human-launched interactive sessions, never by a spawned worker
// (INV-EXEC-1), which is already holding the lease it was spawned for. Its tool
// lands in Phase 2 with the interactive-session surface.

import { createInterface } from "node:readline";

const API = process.env.SURGE_API ?? "http://127.0.0.1:7420";
const RUN_ID = process.env.SURGE_RUN_ID;
const ISSUE_ID = process.env.SURGE_ISSUE_ID;
const TOKEN = process.env.SURGE_RUNTIME_TOKEN;

const TOOLS = [
  {
    name: "surge_fetch_work_order",
    description:
      "Fetch the work order for the issue this session holds, with its lease and the materialization hash the run executes under (INV-ID-1). Call it first: it is the authoritative statement of the task. Scoped to this worker's own issue — no arguments, nothing else is reachable.",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "surge_append_span",
    description:
      "Append an observability span to this run. Spans are observability, never control flow (INV-EXEC-3) — report what happened; Surge derives state from observed facts.",
    inputSchema: {
      type: "object",
      properties: {
        role: { type: "string", enum: ["coordinator", "worker", "verifier"], description: "Defaults to worker." },
        status: { type: "string", enum: ["running", "ok", "error", "refused"], description: "Defaults to ok." },
        body: { type: "string", description: "What happened." },
        node_id: { type: "string", description: "Pipeline node this span belongs to, if known." },
        parent_span_id: { type: "string" },
        duration_ms: { type: "number" },
        cost: { type: "number" },
      },
      required: ["body"],
    },
  },
  {
    name: "surge_heartbeat",
    description:
      "Heartbeat the lease for the current issue. Call regularly during long work — a silent lease is reclaimed at TTL and the run is failed (§06).",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "surge_poll_run",
    description:
      "Poll this run's status. If it reports ABORTED, stop all work immediately and exit — the abort ledger has spoken (§06).",
    inputSchema: { type: "object", properties: {} },
  },
];

async function call(path, opts = {}) {
  const res = await fetch(`${API}${path}`, {
    ...opts,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
      ...(opts.headers ?? {}),
    },
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${res.status}: ${text}`);
  return text;
}

async function handleTool(name, args) {
  switch (name) {
    case "surge_fetch_work_order": {
      // Own-issue only: the id comes from spawn-time env, never from the
      // model, so there is no argument through which another issue could be
      // named. The server scopes the runtime token independently.
      if (!ISSUE_ID) {
        throw new Error(
          "SURGE_ISSUE_ID not set — no issue in scope (doc run); doc runs carry no work order",
        );
      }
      const wo = JSON.parse(await call(`/runtime/issues/${ISSUE_ID}/work-order`));
      const lease = wo.lease
        ? `${wo.lease.owner} (run ${wo.lease.run_id}, expires ${wo.lease.expires_at})`
        : "none";
      return [
        `issue: ${wo.issue_id}`,
        `work_order_hash: ${wo.work_order_hash}`,
        `materialization_hash: ${wo.materialization_hash ?? "none — compile first (INV-ID-1)"}`,
        `lease: ${lease}`,
        "",
        wo.work_order,
      ].join("\n");
    }
    case "surge_append_span": {
      if (!RUN_ID) throw new Error("SURGE_RUN_ID not set");
      const span = {
        id: `sp_${Math.random().toString(16).slice(2, 14)}`,
        run_id: RUN_ID,
        parent_span_id: args.parent_span_id ?? null,
        node_id: args.node_id ?? null,
        role: args.role ?? "worker",
        started_at: Date.now(),
        duration_ms: args.duration_ms ?? null,
        status: args.status ?? "ok",
        cost: args.cost ?? 0,
        depth: args.parent_span_id ? 1 : 0,
        policy_decision: null,
        body: args.body,
      };
      await call(`/runtime/runs/${RUN_ID}/spans`, { method: "POST", body: JSON.stringify(span) });
      return `span ${span.id} appended`;
    }
    case "surge_heartbeat": {
      if (!ISSUE_ID) return "no issue in scope (doc run) — heartbeat not required";
      const text = await call(`/runtime/issues/${ISSUE_ID}/heartbeat`, { method: "POST" });
      return `lease extended: ${text}`;
    }
    case "surge_poll_run": {
      if (!RUN_ID) throw new Error("SURGE_RUN_ID not set");
      const run = JSON.parse(await call(`/runtime/runs/${RUN_ID}`));
      return run.status === "aborted"
        ? "ABORTED — stop all work immediately and exit."
        : `run ${RUN_ID} is ${run.status}`;
    }
    default:
      throw new Error(`unknown tool: ${name}`);
  }
}

function reply(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\n");
}

function replyError(id, message) {
  process.stdout.write(
    JSON.stringify({ jsonrpc: "2.0", id, error: { code: -32000, message } }) + "\n",
  );
}

const rl = createInterface({ input: process.stdin });
rl.on("line", async (line) => {
  if (!line.trim()) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return; // not a JSON-RPC line; ignore
  }
  const { id, method, params } = msg;
  try {
    if (method === "initialize") {
      reply(id, {
        protocolVersion: params?.protocolVersion ?? "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "surge", version: "0.0.1" },
      });
    } else if (method === "notifications/initialized") {
      // notification — no response
    } else if (method === "ping") {
      reply(id, {});
    } else if (method === "tools/list") {
      reply(id, { tools: TOOLS });
    } else if (method === "tools/call") {
      try {
        const text = await handleTool(params.name, params.arguments ?? {});
        reply(id, { content: [{ type: "text", text }], isError: false });
      } catch (e) {
        reply(id, { content: [{ type: "text", text: String(e.message ?? e) }], isError: true });
      }
    } else if (id !== undefined) {
      replyError(id, `method not found: ${method}`);
    }
  } catch (e) {
    if (id !== undefined) replyError(id, String(e.message ?? e));
  }
});
