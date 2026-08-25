// The global shell (design §07): 236px sidebar, content column, and the two
// floating layers (toasts, dialogs) from LayersProvider. The UI is a pure
// projection of API state polled every 2s — no SSE in phase 0. A 401 anywhere
// flips to the claim screen: reachability is never authorization (INV-AUTH-5).

import { useCallback, useEffect, useMemo, useState } from "react";
import type { Project } from "./generated/Project";
import type { Run } from "./generated/Run";
import { api, ApiError } from "./api";
import { LayersProvider } from "./layers";
import { Sidebar, type Surface } from "./shell";
import { Registry } from "./registry";
import { Observatory, runPill } from "./observatory";

const POLL_MS = 2000;

type AuthState = "checking" | "claimed" | "unclaimed";

function ClaimScreen() {
  return (
    <div className="claim-screen">
      <div className="claim-card">
        <div className="wordmark">Surge</div>
        <h2>Claim this instance</h2>
        <p>
          No session in this browser. Surge prints a one-time claim URL to the terminal it was
          started in — visit that URL here and only this browser will hold the session.
        </p>
        <p>
          Look for <code>http://127.0.0.1:7420/claim/…</code> in the server output (or run{" "}
          <code>surge auth</code>). If the link was already used, restart the server to mint a
          fresh one. This page starts working the moment the claim lands.
        </p>
      </div>
    </div>
  );
}

export function App() {
  const [auth, setAuth] = useState<AuthState>("checking");
  const [surface, setSurface] = useState<Surface>("registry");
  const [projects, setProjects] = useState<Project[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [pollTick, setPollTick] = useState(0);

  const refresh = useCallback(() => setPollTick((t) => t + 1), []);

  // One poll loop drives both surfaces: the project list scopes the sidebar
  // and Registry; the runs list feeds the Observatory and the cards' last-run
  // pills. Auth state falls out of the same requests.
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const [ps, rs] = await Promise.all([
          api.listProjects(),
          api.listRuns(activeProjectId),
        ]);
        if (cancelled) return;
        setProjects(ps);
        setRuns(rs);
        setAuth("claimed");
      } catch (e) {
        if (cancelled) return;
        if (e instanceof ApiError && e.status === 401) setAuth("unclaimed");
        // transient failures keep the last projection; the next tick retries
      }
    };
    void load();
    const t = window.setInterval(load, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [activeProjectId, pollTick]);

  const activeProject = useMemo(
    () => projects.find((p) => p.id === activeProjectId) ?? null,
    [projects, activeProjectId],
  );

  // Newest run per project (runs arrive newest-first) → Registry card pills.
  const lastRunPills = useMemo(() => {
    const m = new Map<string, { cls: string; text: string }>();
    for (const r of runs) {
      if (!m.has(r.project_id)) m.set(r.project_id, runPill(r.status));
    }
    return m;
  }, [runs]);

  if (auth === "unclaimed") return <ClaimScreen />;

  return (
    <LayersProvider>
      <div className="shell">
        <Sidebar
          surface={surface}
          onNavigate={setSurface}
          projects={projects}
          activeProject={activeProject}
          onSelectProject={(id) => {
            setActiveProjectId(id);
            if (id !== null) setSurface("observatory");
          }}
        />
        <main className="content">
          {auth === "checking" ? (
            <p className="dim">Connecting to 127.0.0.1:7420…</p>
          ) : surface === "registry" ? (
            <Registry
              projects={projects}
              lastRunPills={lastRunPills}
              onChanged={refresh}
              onOpenObservatory={(projectId) => {
                setActiveProjectId(projectId);
                setSurface("observatory");
              }}
            />
          ) : (
            <Observatory
              runs={runs}
              scopeName={activeProject ? activeProject.name : "all projects"}
              onChanged={refresh}
            />
          )}
        </main>
      </div>
    </LayersProvider>
  );
}
