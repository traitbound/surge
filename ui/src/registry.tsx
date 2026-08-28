// Registry — the default landing surface (design §08): one card per bound
// project plus phase 0's per-project actions (compile, fixture-issue
// create/dispatch). Compile success opens the §04 capability report dialog —
// accepting it is the approval of what the pipeline can do.

import { useState } from "react";
import type { Project } from "./generated/Project";
import type { CapabilityReport } from "./generated/CapabilityReport";
import { api, ApiError, fixtureIssueId } from "./api";
import { useLayers } from "./layers";

const FIXTURE_PIPELINE_ID = "pl_two_node_v1";

function CapabilityReportDialog({
  report,
  hash,
  files,
  onClose,
}: {
  report: CapabilityReport;
  hash: string;
  files: string[];
  onClose: () => void;
}) {
  return (
    <>
      <h2>Materialization compiled</h2>
      <p className="sub">
        Capability report — accepting it is the approval of what this pipeline can do.
      </p>
      <div className="cap-report">
        <div className="cap-line">
          <span className="key">Writes</span>
          <span className="val">{report.writes.length ? report.writes.join(", ") : "none"}</span>
        </div>
        <div className="cap-line">
          <span className="key">Shell</span>
          <span className="val">
            {report.shell_count === 0
              ? "none"
              : `${report.shell_count} command${report.shell_count === 1 ? "" : "s"}` +
                (report.shell_first.length ? ` — ${report.shell_first.join(", ")}` : "")}
          </span>
        </div>
        <div className="cap-line">
          <span className="key">Network</span>
          <span className="val">{report.network.length ? report.network.join(", ") : "none"}</span>
        </div>
        <div className="cap-line">
          <span className="key">Egress</span>
          <span className="val">{report.egress}</span>
        </div>
        <div className="cap-line">
          <span className="key">Hash</span>
          <span className="val">{hash}</span>
        </div>
        <div className="cap-line">
          <span className="key">Files</span>
          <span className="val">{files.join(", ")}</span>
        </div>
      </div>
      <div className="buttons">
        <button className="primary" onClick={onClose}>
          Accept
        </button>
      </div>
    </>
  );
}

function BindProjectDialog({ onDone }: { onDone: (created: boolean) => void }) {
  const { toast } = useLayers();
  const [name, setName] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const slug = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    if (!slug || !repoPath.trim()) {
      toast("error", "Name and repo path are both required.");
      return;
    }
    setBusy(true);
    try {
      const created = await api.createProject({
        id: `prj_${slug}`,
        name: name.trim(),
        repo_path: repoPath.trim(),
      });
      // Registering the row is only half of it: binding is what writes
      // surge.yaml into the repo (INV-DATA-1). Reporting "bound" without
      // this call was a lie the badge quietly contradicted.
      await api.bindProject(created.id);
      toast("success", `Project "${name.trim()}" bound.`);
      onDone(true);
    } catch (e) {
      toast("error", e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <>
      <h2>Bind project</h2>
      <p className="sub">
        Registers the project and writes surge.yaml into the repo — the only file
        binding writes (closed write list, INV-DATA-1).
      </p>
      <div className="field">
        <label>Name</label>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="my-service" autoFocus />
      </div>
      <div className="field">
        <label>Repo path</label>
        <input
          value={repoPath}
          onChange={(e) => setRepoPath(e.target.value)}
          placeholder="/absolute/path/to/repo"
        />
      </div>
      <div className="buttons">
        <button onClick={() => onDone(false)}>Cancel</button>
        <button className="primary" onClick={submit} disabled={busy}>
          Bind
        </button>
      </div>
    </>
  );
}

function CompileDialog({
  project,
  onDone,
}: {
  project: Project;
  onDone: (report: { report: CapabilityReport; hash: string; files: string[] } | null) => void;
}) {
  const { toast } = useLayers();
  const [pipelineId, setPipelineId] = useState(
    project.assigned_pipeline?.pipeline_id ?? FIXTURE_PIPELINE_ID,
  );
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      const res = await api.compile(project.id, pipelineId.trim());
      onDone({
        report: res.capability_report,
        hash: res.materialization.content_hash,
        files: res.files,
      });
    } catch (e) {
      // 409 (trust block) / 422 (compile refusal): the reason is a toast.
      toast("error", e instanceof ApiError ? `Compile refused — ${e.message}` : String(e));
      onDone(null);
    }
  };

  return (
    <>
      <h2>Compile materialization</h2>
      <p className="sub">
        pipeline × {project.name} → compiled runtime files in <span className="mono">{project.repo_path}</span>
      </p>
      <div className="field">
        <label>Pipeline id</label>
        <input value={pipelineId} onChange={(e) => setPipelineId(e.target.value)} autoFocus />
      </div>
      <div className="buttons">
        <button onClick={() => onDone(null)}>Cancel</button>
        <button className="primary" onClick={submit} disabled={busy || !pipelineId.trim()}>
          Compile
        </button>
      </div>
    </>
  );
}

function ProjectCard({
  project,
  lastRunPill,
  onChanged,
  onOpenObservatory,
}: {
  project: Project;
  lastRunPill: { cls: string; text: string } | null;
  onChanged: () => void;
  onOpenObservatory: () => void;
}) {
  const { toast, openDialog, closeDialog } = useLayers();
  const [busy, setBusy] = useState<string | null>(null);

  const compile = () => {
    openDialog(
      <CompileDialog
        project={project}
        onDone={(result) => {
          if (result) {
            onChanged();
            openDialog(
              <CapabilityReportDialog
                report={result.report}
                hash={result.hash}
                files={result.files}
                onClose={closeDialog}
              />,
            );
          } else {
            closeDialog();
          }
        }}
      />,
    );
  };

  const createFixtureIssue = async () => {
    setBusy("issue");
    try {
      await api.createIssue({
        id: fixtureIssueId(project.id),
        project_id: project.id,
        title: "Fixture issue",
        wave: 1,
        phase: "phase-0",
      });
      toast("success", "Fixture issue created — ready to dispatch.");
    } catch (e) {
      toast("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const dispatch = async () => {
    setBusy("dispatch");
    try {
      const res = await api.dispatchIssue(fixtureIssueId(project.id));
      toast("success", `Dispatched — run ${res.run_id}.`);
      onOpenObservatory();
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        // The server's reason already starts with "dispatch refused — ";
        // prefixing our own produced "Dispatch refused — dispatch refused — …".
        const reason = e.message.replace(/^dispatch refused\s*—\s*/i, "");
        toast("error", `Dispatch refused — ${reason}`);
      } else {
        toast("error", e instanceof Error ? e.message : String(e));
      }
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="project-card">
      <div className="title-row">
        <span className="name">{project.name}</span>
        {project.pipeline_status === "not_compiled" ? (
          // Not "stale": the server derives this from materialization
          // freshness, and the only way to have none is never to have compiled
          // (ESC-3). Dispatch is refused in this state (INV-ID-1).
          <span className="pill warn">not compiled</span>
        ) : (
          <span className="badge">{project.surge_yaml_written ? "surge.yaml" : "unbound repo"}</span>
        )}
      </div>
      <div className="repo">{project.repo_path}</div>
      <div className="stats">
        <div className="row">
          <span>Pipeline</span>
          <span className="mono">
            {project.assigned_pipeline
              ? `${project.assigned_pipeline.name} · v${project.assigned_pipeline.version}`
              : "not assigned"}
          </span>
        </div>
        <div className="row">
          <span>Last run</span>
          {lastRunPill ? (
            <span className={`pill ${lastRunPill.cls}`}>{lastRunPill.text}</span>
          ) : (
            <span className="faint">none yet</span>
          )}
        </div>
      </div>
      <div className="actions">
        <button className="primary" onClick={compile}>
          Compile
        </button>
        <button onClick={createFixtureIssue} disabled={busy !== null}>
          Create fixture issue
        </button>
        <button onClick={dispatch} disabled={busy !== null}>
          Dispatch
        </button>
      </div>
    </div>
  );
}

export function Registry({
  projects,
  lastRunPills,
  onChanged,
  onOpenObservatory,
}: {
  projects: Project[];
  lastRunPills: Map<string, { cls: string; text: string }>;
  onChanged: () => void;
  onOpenObservatory: (projectId: string) => void;
}) {
  const { openDialog, closeDialog } = useLayers();

  const bind = () => {
    openDialog(
      <BindProjectDialog
        onDone={(created) => {
          closeDialog();
          if (created) onChanged();
        }}
      />,
    );
  };

  return (
    <div>
      <div className="page-header">
        <h1>Registry</h1>
        <span className="sub">every bound project</span>
        <span className="spacer" />
        <button className="primary" onClick={bind}>
          + Bind project
        </button>
      </div>
      {projects.length === 0 ? (
        <div className="empty-state">
          <div className="tile">▤</div>
          <h2>No projects yet</h2>
          <p>
            Bind a repo to give it a home in Surge — config, docs, board, pipelines, and
            observability all in one place.
          </p>
          <button className="primary" onClick={bind}>
            Bind your first project
          </button>
        </div>
      ) : (
        <div className="card-grid">
          {projects.map((p) => (
            <ProjectCard
              key={p.id}
              project={p}
              lastRunPill={lastRunPills.get(p.id) ?? null}
              onChanged={onChanged}
              onOpenObservatory={() => onOpenObservatory(p.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
