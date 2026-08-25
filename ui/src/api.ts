// Thin fetch layer over the human-token API. The session rides the
// `surge_session` cookie set by the one-time claim URL (INV-AUTH-5); a 401
// anywhere flips the app to the claim screen. Types come from the ts-rs seam
// (ADR-1) — never hand-write shapes the server already defines.

import type { Project } from "./generated/Project";
import type { Run } from "./generated/Run";
import type { Span } from "./generated/Span";
import type { Issue } from "./generated/Issue";
import type { Materialization } from "./generated/Materialization";
import type { CapabilityReport } from "./generated/CapabilityReport";

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: init?.body ? { "Content-Type": "application/json" } : undefined,
  });
  if (!res.ok) {
    let message = `${res.status}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) message = body.error;
    } catch {
      /* non-JSON error body — keep the status string */
    }
    throw new ApiError(res.status, message);
  }
  return (await res.json()) as T;
}

export const api = {
  listProjects: () => request<Project[]>("/api/projects"),

  createProject: (body: { id: string; name: string; repo_path: string }) =>
    request<Project>("/api/projects", { method: "POST", body: JSON.stringify(body) }),

  compile: (projectId: string, pipelineId: string) =>
    request<{
      materialization: Materialization;
      capability_report: CapabilityReport;
      files: string[];
    }>(`/api/projects/${encodeURIComponent(projectId)}/compile`, {
      method: "POST",
      body: JSON.stringify({ pipeline_id: pipelineId }),
    }),

  createIssue: (body: { id: string; project_id: string; title: string; wave: number; phase: string }) =>
    request<Issue>("/api/issues", { method: "POST", body: JSON.stringify(body) }),

  dispatchIssue: (issueId: string) =>
    request<{ run_id: string; refused: boolean }>(
      `/api/issues/${encodeURIComponent(issueId)}/dispatch`,
      { method: "POST" },
    ),

  listRuns: (projectId: string | null) =>
    request<Run[]>(
      projectId ? `/api/runs?project_id=${encodeURIComponent(projectId)}` : "/api/runs",
    ),

  runSpans: (runId: string) => request<Span[]>(`/api/runs/${encodeURIComponent(runId)}/spans`),

  abortRun: (runId: string) =>
    request<{ ok: boolean }>(`/api/runs/${encodeURIComponent(runId)}/abort`, { method: "POST" }),
};

/** Fixture-issue id convention for phase 0's single dispatchable issue. */
export function fixtureIssueId(projectId: string): string {
  return `iss_fixture_${projectId}`;
}

/** Timestamps cross the wire as JSON numbers; ts-rs types them `bigint`. */
export function ms(v: bigint | number | null): number | null {
  return v === null ? null : Number(v);
}

export function relativeTime(millis: bigint | number): string {
  const delta = Date.now() - Number(millis);
  if (delta < 5_000) return "just now";
  if (delta < 60_000) return `${Math.floor(delta / 1_000)}s ago`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`;
  return `${Math.floor(delta / 86_400_000)}d ago`;
}

export function duration(msTotal: number): string {
  if (msTotal < 1_000) return `${msTotal}ms`;
  const s = Math.floor(msTotal / 1_000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}
