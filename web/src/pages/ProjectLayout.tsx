import { NavLink, Outlet, useLocation, useNavigate, useOutletContext, useParams, useSearchParams } from "react-router-dom";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, isClosed, Priority, Project, ProjectSummary, Ticket } from "../api";
import { PrioritySelect, toast, useLocalState } from "../ui";
import TicketPanel from "../components/TicketPanel";
import NewTicketModal from "../components/NewTicketModal";
import * as I from "../icons";

export interface Filters {
  q: string;
  type: string;
  priority: "" | Priority;
  label: string;
  assignee: string;
  milestone: string;
  showClosed: boolean;
}

const EMPTY: Filters = { q: "", type: "", priority: "", label: "", assignee: "", milestone: "", showClosed: true };

export interface ProjectCtx {
  project: ProjectSummary;
  tickets: Ticket[];
  visible: Ticket[];
  filters: Filters;
  openTicket: (id: string) => void;
  newTicket: (status?: string) => void;
}

export const useProject = () => useOutletContext<ProjectCtx>();

export function categoryOf(project: Project, status: string) {
  return project.statuses.find((s) => s.name === status)?.category;
}

function applyFilters(tickets: Ticket[], f: Filters, project: ProjectSummary) {
  const q = f.q.trim().toLowerCase();
  return tickets.filter(
    (t) =>
      (!q || t.id.toLowerCase().includes(q) || t.title.toLowerCase().includes(q) || t.description.toLowerCase().includes(q)) &&
      (!f.type || t.type === f.type) &&
      (!f.priority || t.priority === f.priority) &&
      (!f.label || t.labels?.includes(f.label)) &&
      (!f.assignee || (f.assignee === "-" ? !t.assignee : t.assignee === f.assignee)) &&
      (!f.milestone || t.milestone === f.milestone) &&
      (f.showClosed || !isClosed(categoryOf(project, t.status))),
  );
}

export default function ProjectLayout() {
  const { key = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const [filters, setFilters] = useLocalState<Filters>(`bankan.filters.${key}`, EMPTY);
  const project = useQuery({ queryKey: ["project", key], queryFn: () => api.project(key) });
  const tickets = useQuery({ queryKey: ["tickets", key], queryFn: () => api.tickets(key) });

  // undefined = closed; null = open with the default status; string = open in that column.
  const [creating, setCreating] = useState<string | null | undefined>(undefined);

  const openTicket = (id: string) => {
    const next = new URLSearchParams(params);
    next.set("ticket", id);
    setParams(next);
  };
  const closeTicket = () => {
    const next = new URLSearchParams(params);
    next.delete("ticket");
    setParams(next);
  };

  if (project.error) {
    return (
      <div className="page">
        <h1>Project {key} not found</h1>
        <p className="muted">{String(project.error)}</p>
        <button onClick={() => navigate("/")}>Go home</button>
      </div>
    );
  }
  if (!project.data || !tickets.data) return <div className="page faint">Loading…</div>;

  const p = project.data;
  const showFilters = /\/(board|list)$/.test(location.pathname);
  const set = (patch: Partial<Filters>) => setFilters({ ...filters, ...patch });
  const active = JSON.stringify({ ...filters, showClosed: true }) !== JSON.stringify(EMPTY) || !filters.showClosed;
  const assignees = [...new Set(tickets.data.map((t) => t.assignee).filter(Boolean) as string[])].sort();
  const ctx: ProjectCtx = {
    project: p,
    tickets: tickets.data,
    visible: applyFilters(tickets.data, filters, p),
    filters,
    openTicket,
    newTicket: (status) => setCreating(status ?? null),
  };
  const ticketId = params.get("ticket");

  return (
    <>
      <div className="topbar">
        <span className="project-key" style={{ background: p.color ?? undefined }}>
          {p.key}
        </span>
        <h2>{p.name}</h2>
        {p.archived && <span className="chip">archived</span>}
        <nav className="tabs">
          {(
            [
              ["board", "Board", I.Board],
              ["list", "List", I.List],
              ["docs", "Docs", I.Doc],
              ["proposals", "Proposals", I.Proposal],
              ["settings", "Settings", I.Sliders],
            ] as const
          ).map(([to, label, Icon]) => (
            <NavLink key={to} to={to} className={({ isActive }) => `tab${isActive ? " active" : ""}`}>
              <Icon size={14} />
              {label}
            </NavLink>
          ))}
        </nav>
        <span className="spacer" />
        <button className="primary" onClick={() => setCreating(null)}>
          <I.Plus size={14} /> New ticket
        </button>
      </div>
      {showFilters && (
        <div className="filters">
          <label className="search-box">
            <I.Search size={14} />
            <input type="search" placeholder="Filter…" value={filters.q} onChange={(e) => set({ q: e.target.value })} />
          </label>
          <select className={`pill${filters.type ? " set" : ""}`} value={filters.type} onChange={(e) => set({ type: e.target.value })}>
            <option value="">Type</option>
            {p.types.map((t) => (
              <option key={t.name}>{t.name}</option>
            ))}
          </select>
          <PrioritySelect className={`pill${filters.priority ? " set" : ""}`} value={filters.priority} onChange={(priority) => set({ priority })} emptyLabel="Priority" />
          {p.labels.length > 0 && (
            <select className={`pill${filters.label ? " set" : ""}`} value={filters.label} onChange={(e) => set({ label: e.target.value })}>
              <option value="">Label</option>
              {p.labels.map((l) => (
                <option key={l.name}>{l.name}</option>
              ))}
            </select>
          )}
          <select className={`pill${filters.assignee ? " set" : ""}`} value={filters.assignee} onChange={(e) => set({ assignee: e.target.value })}>
            <option value="">Assignee</option>
            <option value="-">Unassigned</option>
            {assignees.map((a) => (
              <option key={a}>{a}</option>
            ))}
          </select>
          {p.milestones.length > 0 && (
            <select className={`pill${filters.milestone ? " set" : ""}`} value={filters.milestone} onChange={(e) => set({ milestone: e.target.value })}>
              <option value="">Milestone</option>
              {p.milestones.map((m) => (
                <option key={m.name}>{m.name}</option>
              ))}
            </select>
          )}
          {active && (
            <button className="ghost small" onClick={() => setFilters(EMPTY)}>
              <I.X size={13} /> Clear
            </button>
          )}
          <span className="spacer" />
          <label className="toggle">
            <input type="checkbox" checked={filters.showClosed} onChange={(e) => set({ showClosed: e.target.checked })} />
            Show closed
          </label>
          <span className="faint small" style={{ fontVariantNumeric: "tabular-nums" }}>
            {ctx.visible.length === tickets.data.length ? `${tickets.data.length} tickets` : `${ctx.visible.length} of ${tickets.data.length}`}
          </span>
        </div>
      )}
      {/* Keyed so per-project page state (board columns, settings form, editors) never leaks between projects. */}
      <Outlet key={p.key} context={ctx} />
      {creating !== undefined && (
        <NewTicketModal
          project={p}
          status={creating ?? undefined}
          onClose={() => setCreating(undefined)}
          onCreated={(t, open) => {
            toast(`Created ${t.id}`);
            if (open) {
              setCreating(undefined);
              openTicket(t.id);
            }
          }}
        />
      )}
      {ticketId && <TicketPanel id={ticketId} project={p} onClose={closeTicket} onOpen={openTicket} />}
    </>
  );
}
