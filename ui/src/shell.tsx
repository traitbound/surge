// The global shell's sidebar (design §07): 236px fixed card surface with a
// right border — wordmark, instance nav, project switcher, project nav,
// footer. Phase 0 mounts Registry and Observatory; every other surface is
// present but disabled (they are later phases).

import { useState } from "react";
import type { Project } from "./generated/Project";

export type Surface = "registry" | "observatory";

interface NavEntry {
  icon: string;
  label: string;
  surface?: Surface;
  phase?: string;
}

const INSTANCE_NAV: NavEntry[] = [
  { icon: "▤", label: "Registry", surface: "registry" },
  { icon: "◇", label: "Pipelines", phase: "1" },
  { icon: "❏", label: "Library", phase: "1" },
  { icon: "⚙", label: "Settings", phase: "3" },
];

const PROJECT_NAV: NavEntry[] = [
  { icon: "▣", label: "Overview", phase: "1" },
  { icon: "≡", label: "Docs", phase: "2" },
  { icon: "☰", label: "Board", phase: "2" },
  { icon: "◇", label: "Pipeline", phase: "1" },
  { icon: "◎", label: "Observatory", surface: "observatory" },
  { icon: "⚙", label: "Settings", phase: "3" },
];

interface SidebarProps {
  surface: Surface;
  onNavigate: (surface: Surface) => void;
  projects: Project[];
  activeProject: Project | null;
  onSelectProject: (id: string | null) => void;
}

function NavRow({
  entry,
  active,
  onNavigate,
}: {
  entry: NavEntry;
  active: boolean;
  onNavigate: (surface: Surface) => void;
}) {
  const enabled = entry.surface !== undefined;
  return (
    <button
      className={`nav-row${active ? " active" : ""}`}
      disabled={!enabled}
      onClick={() => entry.surface && onNavigate(entry.surface)}
      title={enabled ? undefined : `Lands in phase ${entry.phase}`}
    >
      <span className="icon">{entry.icon}</span>
      {entry.label}
      {!enabled && <span className="soon">p{entry.phase}</span>}
    </button>
  );
}

export function Sidebar({
  surface,
  onNavigate,
  projects,
  activeProject,
  onSelectProject,
}: SidebarProps) {
  const [switcherOpen, setSwitcherOpen] = useState(false);

  return (
    <nav className="sidebar">
      <div className="wordmark" onClick={() => onNavigate("registry")}>
        Surge
      </div>

      <div className="nav-heading">Instance</div>
      {INSTANCE_NAV.map((e) => (
        <NavRow
          key={e.label}
          entry={e}
          active={e.surface === surface}
          onNavigate={onNavigate}
        />
      ))}

      <div className="switcher">
        <button className="switcher-card" onClick={() => setSwitcherOpen((o) => !o)}>
          <span className="names">
            <span className="name">{activeProject ? activeProject.name : "All projects"}</span>
            <span className="path">
              {activeProject ? activeProject.repo_path : `${projects.length} bound`}
            </span>
          </span>
          <span className="chevron">▾</span>
        </button>
        {switcherOpen && (
          <>
            <div className="click-catcher" onClick={() => setSwitcherOpen(false)} />
            <div className="switcher-menu">
              {projects.map((p) => (
                <button
                  key={p.id}
                  onClick={() => {
                    onSelectProject(p.id);
                    setSwitcherOpen(false);
                  }}
                >
                  <span>{p.name}</span>
                  <span className="faint mono">
                    {p.assigned_pipeline ? `v${p.assigned_pipeline.version}` : "—"}
                  </span>
                </button>
              ))}
              {projects.length === 0 && (
                <button disabled className="faint">
                  No projects bound yet
                </button>
              )}
              <button
                className="all"
                onClick={() => {
                  onSelectProject(null);
                  setSwitcherOpen(false);
                }}
              >
                All projects
              </button>
            </div>
          </>
        )}
      </div>

      <div className="nav-heading">Project</div>
      {PROJECT_NAV.map((e) => (
        <NavRow
          key={e.label}
          entry={e}
          active={e.surface === surface}
          onNavigate={onNavigate}
        />
      ))}

      <div className="sidebar-footer">
        <span className="dot" />
        127.0.0.1:7420 · local
      </div>
    </nav>
  );
}
