import { useEffect, useState } from "react";
import { NavLink, Navigate, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./api";
import { useLocalState } from "./ui";
import * as I from "./icons";
import Home from "./pages/Home";
import ProjectLayout from "./pages/ProjectLayout";
import Board from "./pages/Board";
import ListView from "./pages/ListView";
import Docs from "./pages/Docs";
import Proposals from "./pages/Proposals";
import ProjectSettings from "./pages/ProjectSettings";
import Settings from "./pages/Settings";
import SearchPalette from "./components/SearchPalette";
import NewProjectModal from "./components/NewProjectModal";

/** Subscribes to the daemon's change feed and refreshes cached data when anything changes (e.g. an agent moves a card). */
function useLiveUpdates() {
  const qc = useQueryClient();
  const [live, setLive] = useState(true);
  useEffect(() => {
    let timer: number | undefined;
    let lost = false;
    const es = new EventSource("/api/events");
    es.onopen = () => {
      setLive(true);
      // Events may have been missed while disconnected, so refetch everything.
      if (lost) qc.invalidateQueries();
      lost = false;
    };
    es.onerror = () => {
      lost = true;
      setLive(false);
    };
    es.onmessage = () => {
      clearTimeout(timer);
      timer = window.setTimeout(() => qc.invalidateQueries(), 120);
    };
    return () => {
      clearTimeout(timer);
      es.close();
    };
  }, [qc]);
  return live;
}

function useTheme() {
  const [theme, setTheme] = useLocalState<"system" | "light" | "dark">("bankan.theme", "system");
  useEffect(() => {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);
  return [theme, setTheme] as const;
}

function Sidebar({ live, onSearch, onNewProject }: { live: boolean; onSearch: () => void; onNewProject: () => void }) {
  const { data: projects } = useQuery({ queryKey: ["projects"], queryFn: () => api.projects() });
  const [theme, setTheme] = useTheme();
  const next = { system: "light", light: "dark", dark: "system" } as const;
  const ThemeIcon = { system: I.Monitor, light: I.Sun, dark: I.Moon }[theme];
  return (
    <aside className="sidebar">
      <div className="brand">
        <img src="/favicon.svg" alt="" />
        BanKan
        <span className="spacer" />
        <span className={`live-dot${live ? "" : " off"}`} title={live ? "Live: changes from agents appear automatically" : "Disconnected from the BanKan daemon"} />
      </div>
      <button className="nav-item" onClick={onSearch}>
        <I.Search /> Search <span className="count kbd">⌘K</span>
      </button>
      <NavLink to="/" end className="nav-item">
        <I.Home /> Home
      </NavLink>
      <div className="nav-section">
        Projects
        <span className="spacer" />
        <button className="ghost small icon" title="New project" onClick={onNewProject}>
          <I.Plus size={14} />
        </button>
      </div>
      {projects?.map((p) => (
        <NavLink key={p.key} to={`/p/${p.key}`} className={({ isActive }) => `nav-item${isActive ? " active" : ""}`}>
          <span className="project-key" style={{ background: p.color ?? undefined }}>
            {p.key}
          </span>
          <span className="ellipsis">{p.name}</span>
          <span className="count">{p.counts.unstarted + p.counts.started + p.counts.backlog || ""}</span>
        </NavLink>
      ))}
      {projects?.length === 0 && (
        <button className="nav-item faint" onClick={onNewProject}>
          <I.Plus /> Create a project
        </button>
      )}
      <div className="spacer" />
      <NavLink to="/settings" className="nav-item">
        <I.Gear /> Settings
      </NavLink>
      <button className="nav-item" onClick={() => setTheme(next[theme])} title="Switch theme">
        <ThemeIcon /> <span style={{ textTransform: "capitalize" }}>{theme} theme</span>
      </button>
    </aside>
  );
}

export default function App() {
  const live = useLiveUpdates();
  const [searchOpen, setSearchOpen] = useState(false);
  const [newProject, setNewProject] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearchOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app">
      <Sidebar live={live} onSearch={() => setSearchOpen(true)} onNewProject={() => setNewProject(true)} />
      <main className="main">
        <Routes>
          <Route path="/" element={<Home onNewProject={() => setNewProject(true)} />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="/t/:id" element={<TicketRedirect />} />
          <Route path="/p/:key" element={<ProjectLayout />}>
            <Route index element={<Navigate to="board" replace />} />
            <Route path="board" element={<Board />} />
            <Route path="list" element={<ListView />} />
            <Route path="docs/*" element={<Docs />} />
            <Route path="proposals" element={<Proposals />} />
            <Route path="proposals/:id" element={<Proposals />} />
            <Route path="settings" element={<ProjectSettings />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>
      {searchOpen && <SearchPalette onClose={() => setSearchOpen(false)} />}
      {newProject && (
        <NewProjectModal
          onClose={() => setNewProject(false)}
          onCreated={(key) => {
            setNewProject(false);
            navigate(`/p/${key}/board`);
          }}
        />
      )}
    </div>
  );
}

/** /t/RES-12 → the project's board with the ticket open. */
function TicketRedirect() {
  const id = useParams().id ?? "";
  const key = id.split("-").slice(0, -1).join("-").toUpperCase();
  return <Navigate to={`/p/${key}/board?ticket=${id.toUpperCase()}`} replace />;
}
