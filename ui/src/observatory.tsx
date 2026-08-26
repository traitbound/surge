// Observatory — phase 0's minimal runs list (design §16 subset): runs
// newest-first with status pills, an expandable span tree per run (role,
// timing, status, cost, policy string), Abort on a running run, and the
// refusal span shown prominently on refused runs. Read-only polling; the
// waterfall, COE and metrics rail land in phase 3.

import { useEffect, useState } from "react";
import type { Run } from "./generated/Run";
import type { Span } from "./generated/Span";
import { api, duration, relativeTime } from "./api";
import { useLayers } from "./layers";

export function runPill(status: Run["status"]): { cls: string; text: string } {
  switch (status) {
    case "running":
      return { cls: "run", text: "running" };
    case "succeeded":
      return { cls: "ok", text: "succeeded" };
    case "failed":
      return { cls: "err", text: "failed" };
    case "aborted":
      return { cls: "warn", text: "aborted" };
    case "refused":
      return { cls: "err", text: "refused" };
  }
}

function spanPill(status: Span["status"]): { cls: string; text: string } {
  switch (status) {
    case "running":
      return { cls: "run", text: "running" };
    case "ok":
      return { cls: "ok", text: "ok" };
    case "error":
      return { cls: "err", text: "error" };
    case "refused":
      return { cls: "err", text: "refused" };
  }
}

function SpanRow({ span }: { span: Span }) {
  const pill = spanPill(span.status);
  return (
    <>
      <div className="span-row" style={{ paddingLeft: Number(span.depth) * 18 }}>
        <span className="role">{span.role}</span>
        {span.node_id && <span className="node-tag">{span.node_id}</span>}
        <span className="label" title={span.id}>
          {/* The body is what an operator reads. Leading with an id turned
              every row into an opaque hash, since node_id is not yet emitted
              (smoke walk 3, N3) — the content sat unread in a tooltip. */}
          {span.body ?? span.node_id ?? span.id}
        </span>
        <span className="metrics">
          <span className="mono faint">{relativeTime(span.started_at)}</span>
          <span className="mono">
            {span.duration_ms !== null ? duration(Number(span.duration_ms)) : "—"}
          </span>
          <span className="mono faint">${span.cost.toFixed(3)}</span>
          <span className={`pill ${pill.cls}`}>{pill.text}</span>
        </span>
      </div>
      {span.policy_decision && (
        <div className="span-policy" style={{ marginLeft: Number(span.depth) * 18 }}>
          {span.policy_decision}
        </div>
      )}
    </>
  );
}

function RunRow({ run, onAborted }: { run: Run; onAborted: () => void }) {
  const { toast } = useLayers();
  const [expanded, setExpanded] = useState(false);
  const [spans, setSpans] = useState<Span[] | null>(null);
  const [aborting, setAborting] = useState(false);

  // Fetch the span tree on expand; keep polling it while the run is live so
  // new spans appear without SSE (phase 0 is polling-only).
  useEffect(() => {
    // Refused runs load their tree immediately so the reason span can show
    // prominently without expanding.
    if (!expanded && run.status !== "refused") return;
    let cancelled = false;
    const load = () =>
      api
        .runSpans(run.id)
        .then((s) => {
          if (!cancelled) setSpans(s);
        })
        .catch(() => {
          /* transient poll failure — the next tick retries */
        });
    void load();
    if (run.status !== "running") return () => void (cancelled = true);
    const t = window.setInterval(load, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [expanded, run.id, run.status]);

  const abort = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setAborting(true);
    try {
      await api.abortRun(run.id);
      toast("info", `Abort requested for ${run.id} — lands at the executor's next tool call.`);
      onAborted();
    } catch (err) {
      toast("error", err instanceof Error ? err.message : String(err));
    } finally {
      setAborting(false);
    }
  };

  const pill = runPill(run.status);
  const started = Number(run.started_at);
  const ended = run.ended_at === null ? null : Number(run.ended_at);
  const refusalSpan =
    run.status === "refused" && spans ? spans.find((s) => s.status === "refused") : undefined;

  return (
    <div className="run-row">
      <div className="head" onClick={() => setExpanded((x) => !x)}>
        <span className="caret">{expanded ? "▼" : "▶"}</span>
        <span className="run-id">{run.id}</span>
        <span className={`pill ${pill.cls}`}>{pill.text}</span>
        <span className="badge">{run.kind === "doc" ? "doc run" : "work order"}</span>
        <span className="meta">
          <span className="mono faint" title={run.materialization_hash}>
            {run.materialization_hash.slice(0, 14)}…
          </span>
          <span>{relativeTime(started)}</span>
          <span className="mono">{ended !== null ? duration(ended - started) : "—"}</span>
          <span className="mono faint">${run.cost.toFixed(3)}</span>
          {run.status === "running" && (
            <button className="danger" onClick={abort} disabled={aborting}>
              Abort run
            </button>
          )}
        </span>
      </div>
      {run.status === "aborted" && (
        <div className="abort-banner">
          Abort requested — takes effect at the executor's next tool call. If heartbeats stop, the
          lease reclaims at TTL.
        </div>
      )}
      {refusalSpan && (
        <div className="refusal-banner">
          Refused — {refusalSpan.policy_decision ?? refusalSpan.body ?? "no reason recorded"}
        </div>
      )}
      {expanded && (
        <div className="span-tree">
          {spans === null ? (
            <div className="faint" style={{ padding: "8px 0" }}>
              Loading spans…
            </div>
          ) : spans.length === 0 ? (
            <div className="faint" style={{ padding: "8px 0" }}>
              No spans recorded for this run.
            </div>
          ) : (
            spans.map((s) => <SpanRow key={s.id} span={s} />)
          )}
        </div>
      )}
    </div>
  );
}

export function Observatory({
  runs,
  scopeName,
  onChanged,
}: {
  runs: Run[];
  scopeName: string;
  onChanged: () => void;
}) {
  return (
    <div>
      <div className="page-header">
        <h1>Observatory</h1>
        <span className="sub">runs · {scopeName}</span>
      </div>
      {runs.length === 0 ? (
        <div className="empty-state">
          <div className="tile">◎</div>
          <h2>No runs yet</h2>
          <p>This project has not dispatched a run yet.</p>
        </div>
      ) : (
        <div className="run-list">
          {runs.map((r) => (
            <RunRow key={r.id} run={r} onAborted={onChanged} />
          ))}
        </div>
      )}
    </div>
  );
}
