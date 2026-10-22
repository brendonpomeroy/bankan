import { Link } from "react-router-dom";
import { useQueries, useQuery } from "@tanstack/react-query";
import { api, Ticket } from "../api";
import { PriorityIcon, timeAgo } from "../ui";
import * as I from "../icons";
import { categoryOf } from "./ProjectLayout";

export default function Home({ onNewProject }: { onNewProject: () => void }) {
  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api.projects() });
  const ticketQueries = useQueries({
    queries: (projects.data ?? []).map((p) => ({ queryKey: ["tickets", p.key], queryFn: () => api.tickets(p.key) })),
  });

  if (!projects.data) return <div className="page faint">{projects.error ? String(projects.error) : "Loading…"}</div>;

  const all = ticketQueries.flatMap((q, i) => (q.data ?? []).map((t) => ({ t, p: projects.data[i] })));
  const byUpdated = (a: Ticket, b: Ticket) => b.updated.localeCompare(a.updated);
  const inProgress = all.filter(({ t, p }) => p && categoryOf(p, t.status) === "started").map(({ t }) => t).sort(byUpdated);
  const recent = all.map(({ t }) => t).sort(byUpdated).slice(0, 12);

  if (projects.data.length === 0) {
    return (
      <div className="page narrow">
        <div className="empty-state">
          <div className="icon-wrap">
            <I.Board size={20} />
          </div>
        </div>
        <h1>Welcome to BanKan</h1>
        <p className="muted">Tickets, docs and proposals for all your projects, kept out of your repos and available to your AI agents over MCP.</p>
        <div className="section" style={{ marginTop: 20 }}>
          <h2>Get started</h2>
          <ol className="muted" style={{ margin: 0, paddingLeft: 18, lineHeight: 1.9 }}>
            <li>Create a project (it gets a short key like <code>RES</code>, used in ticket ids like <code>RES-12</code>).</li>
            <li>
              In the project's repo, run <code>bankan link RES</code> so agents know which project the repo belongs to.
            </li>
            <li>
              Connect Claude Code once: <code>claude mcp add bankan --scope user -- bankan mcp</code>
            </li>
          </ol>
          <div>
            <button className="primary" onClick={onNewProject}>
              <I.Plus size={14} /> Create a project
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="row" style={{ marginBottom: 18 }}>
        <h1>Projects</h1>
        <span className="spacer" />
        <button onClick={onNewProject}>
          <I.Plus size={14} /> New project
        </button>
      </div>
      <div className="grid-cards">
        {projects.data.map((p) => {
          const c = p.counts;
          const total = c.backlog + c.unstarted + c.started + c.completed + c.canceled;
          const pct = (n: number) => `${total ? (n / total) * 100 : 0}%`;
          return (
            <Link key={p.key} to={`/p/${p.key}/board`} className="tile">
              <div className="row">
                <span className="project-key" style={{ background: p.color ?? undefined }}>
                  {p.key}
                </span>
                <b>{p.name}</b>
              </div>
              {p.description && <div className="muted small ellipsis" style={{ marginTop: -6 }}>{p.description}</div>}
              <div className="stats">
                <div className="stat">
                  <b>{c.backlog + c.unstarted}</b>
                  <span>Open</span>
                </div>
                <div className="stat">
                  <b>{c.started}</b>
                  <span>In progress</span>
                </div>
                <div className="stat">
                  <b>{c.completed}</b>
                  <span>Done</span>
                </div>
              </div>
              <div className="bar" title={`${c.completed} done · ${c.started} in progress · ${c.unstarted + c.backlog} open`}>
                {c.completed > 0 && <div style={{ width: pct(c.completed), background: "var(--ok)" }} />}
                {c.started > 0 && <div style={{ width: pct(c.started), background: "var(--warn)" }} />}
                {c.unstarted + c.backlog > 0 && <div style={{ width: pct(c.unstarted + c.backlog), background: "var(--border-strong)" }} />}
              </div>
              <div className="row small faint" style={{ gap: 14 }}>
                <span className="row" style={{ gap: 5 }}>
                  <I.Doc size={13} /> {c.docs} docs
                </span>
                <span className="row" style={{ gap: 5 }}>
                  <I.Proposal size={13} /> {c.proposals} proposals
                </span>
              </div>
            </Link>
          );
        })}
      </div>

      <div className="row" style={{ gap: 14, marginTop: 30, alignItems: "flex-start", flexWrap: "wrap" }}>
        <TicketList title="In progress everywhere" tickets={inProgress} empty="Nothing in progress." />
        <TicketList title="Recently updated" tickets={recent} empty="No tickets yet." />
      </div>
    </div>
  );
}

function TicketList({ title, tickets, empty }: { title: string; tickets: Ticket[]; empty: string }) {
  return (
    <div className="section grow" style={{ minWidth: 340, gap: 6, padding: "16px 12px" }}>
      <h2 style={{ padding: "0 8px 6px" }}>{title}</h2>
      {tickets.length === 0 && <span className="faint" style={{ padding: "0 8px" }}>{empty}</span>}
      {tickets.map((t) => (
        <Link key={t.id} to={`/t/${t.id}`} className="feed-row">
          <span className="mono faint small" style={{ width: 58 }}>{t.id}</span>
          <span style={{ width: 16, display: "flex" }}>
            <PriorityIcon priority={t.priority} />
          </span>
          <span className="grow ellipsis">{t.title}</span>
          <span className="faint small">{t.status}</span>
          <span className="faint small" style={{ width: 58, textAlign: "right" }}>{timeAgo(t.updated)}</span>
        </Link>
      ))}
    </div>
  );
}
