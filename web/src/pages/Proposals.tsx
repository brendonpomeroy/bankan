import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, Proposal, PROPOSAL_STATUSES, ProposalStatus } from "../api";
import { fmtDate, InlineInput, MarkdownField, Modal, StatusDot, TagInput, TicketRefLink, timeAgo, toast } from "../ui";
import { useProject } from "./ProjectLayout";
import * as I from "../icons";

const GROUPS: { title: string; statuses: ProposalStatus[]; hint: string }[] = [
  { title: "Drafts", statuses: ["draft"], hint: "Being written" },
  { title: "In review", statuses: ["review"], hint: "Waiting for a decision" },
  { title: "Accepted", statuses: ["accepted", "implemented"], hint: "Agreed, being or been built" },
  { title: "Closed", statuses: ["rejected", "withdrawn", "superseded"], hint: "Kept for the record" },
];

function NewProposalModal({ projectKey, onClose }: { projectKey: string; onClose: () => void }) {
  const [title, setTitle] = useState("");
  const navigate = useNavigate();
  const qc = useQueryClient();
  const create = useMutation({
    mutationFn: () => api.createProposal(projectKey, { title, authors: ["me"] }),
    onSuccess: (p) => {
      qc.invalidateQueries({ queryKey: ["proposals", projectKey] });
      onClose();
      navigate(`/p/${projectKey}/proposals/${p.id}`);
    },
    meta: { quiet: true },
  });
  const submit = () => title.trim() && !create.isPending && create.mutate();
  return (
    <Modal onClose={onClose}>
      <h2>New proposal</h2>
      <p className="muted small" style={{ margin: 0 }}>
        Starts as a draft from the template: Summary, Motivation, Proposal, Alternatives, Risks, Open questions, Decision.
      </p>
      <input autoFocus placeholder="e.g. Move auth to OAuth device flow" value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      {create.error && <div className="error">{create.error.message}</div>}
      <div className="row">
        <span className="spacer" />
        <button className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button className="primary" disabled={!title.trim() || create.isPending} onClick={submit}>
          Create draft
        </button>
      </div>
    </Modal>
  );
}

function ProposalList({ proposals, projectKey }: { proposals: Proposal[]; projectKey: string }) {
  return (
    <div className="proposal-cols">
      {GROUPS.map((g) => {
        const items = proposals.filter((p) => g.statuses.includes(p.status));
        return (
          <div key={g.title} className="col" style={{ gap: 8 }}>
            <div className="row">
              <h3>{g.title}</h3>
              <span className="faint small">{items.length}</span>
            </div>
            <span className="faint small" style={{ marginTop: -6 }}>{g.hint}</span>
            {items.map((p) => (
              <Link key={p.id} to={`/p/${projectKey}/proposals/${p.id}`} className="tile" style={{ gap: 6, padding: "10px 12px" }}>
                <div className="row small">
                  <span className="mono faint">{[p.id, ...(p.aliases ?? [])].join(" · ")}</span>
                  <span className={`badge ${p.status}`}>{p.status}</span>
                </div>
                <b style={{ fontWeight: 500 }}>{p.title}</b>
                <div className="row faint small">
                  {!!p.tickets?.length && <span>{p.tickets.length} tickets</span>}
                  <span className="spacer" />
                  <span>{timeAgo(p.updated)}</span>
                </div>
              </Link>
            ))}
          </div>
        );
      })}
    </div>
  );
}

function ProposalDetail({ id }: { id: string }) {
  const { project, tickets, openTicket } = useProject();
  const qc = useQueryClient();
  const navigate = useNavigate();
  // The route may name the proposal by an alias (/proposals/D17); writes go to its real key.
  const q = useQuery({ queryKey: ["proposal", id], queryFn: () => api.findProposal(project.key, id) });
  const pid = q.data?.id ?? id;
  const all = useQuery({ queryKey: ["proposals", project.key], queryFn: () => api.proposals(project.key) });
  const [newTicket, setNewTicket] = useState("");
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["proposal"] });
    qc.invalidateQueries({ queryKey: ["proposals", project.key] });
    qc.invalidateQueries({ queryKey: ["tickets", project.key] });
  };
  const update = useMutation({
    mutationFn: (patch: Record<string, unknown>) => api.updateProposal(pid, patch),
    onSuccess: refresh,
  });
  const createTicket = useMutation({
    mutationFn: () => api.createTicket(project.key, { title: newTicket, proposal: pid }),
    onSuccess: (t) => {
      setNewTicket("");
      toast(`Created ${t.id}`);
      refresh();
    },
  });
  const del = useMutation({
    mutationFn: () => api.deleteProposal(pid),
    onSuccess: () => {
      refresh();
      navigate(`/p/${project.key}/proposals`);
    },
  });

  const p = q.data;
  if (q.error) return <div className="error">{q.error.message}</div>;
  if (!p) return <div className="faint">Loading…</div>;
  const set = (patch: Record<string, unknown>) => update.mutate(patch);
  const others = (all.data ?? []).filter((x) => x.id !== p.id);

  return (
    <div className="row" style={{ alignItems: "flex-start", gap: 28 }}>
      <div className="grow" style={{ maxWidth: 820 }}>
        <div className="row small" style={{ marginBottom: 4 }}>
          <Link to={`/p/${project.key}/proposals`} className="row" style={{ gap: 4 }}>
            <I.ArrowLeft size={13} /> Proposals
          </Link>
          <span className="mono faint">{p.id}</span>
          {p.aliases?.map((a) => (
            <span key={a} className="mono faint">· {a}</span>
          ))}
        </div>
        <InlineInput className="title-input" value={p.title} onCommit={(v) => v.trim() && set({ title: v })} />
        <div style={{ marginTop: 14 }}>
          <MarkdownField value={p.body} onSave={(body) => set({ body })} minRows={24} placeholder="Write the proposal…" />
        </div>
      </div>
      <aside className="col" style={{ width: 300, flex: "none", gap: 14 }}>
        <div className="section" style={{ gap: 4, padding: "14px 12px" }}>
          <div className="prop">
            <span>Status</span>
            <select value={p.status} onChange={(e) => set({ status: e.target.value })}>
              {PROPOSAL_STATUSES.map((s) => (
                <option key={s}>{s}</option>
              ))}
            </select>
          </div>
          {p.status === "draft" && (
            <button className="primary small" style={{ margin: "4px 8px 8px" }} onClick={() => set({ status: "review" })}>
              Submit for review
            </button>
          )}
          {p.status === "review" && (
            <div className="row" style={{ padding: "4px 8px 8px" }}>
              <button className="primary small grow" onClick={() => set({ status: "accepted" })}>
                <I.Check size={13} /> Accept
              </button>
              <button className="small grow danger" onClick={() => set({ status: "rejected" })}>
                Reject
              </button>
            </div>
          )}
          {p.decided && <div className="faint small" style={{ padding: "0 8px 4px" }}>Decided {fmtDate(p.decided)}</div>}
          <div className="prop">
            <span>Aliases</span>
            <TagInput values={p.aliases ?? []} onChange={(aliases) => set({ aliases })} placeholder="e.g. D17" />
          </div>
          <div className="prop">
            <span>Authors</span>
            <TagInput values={p.authors ?? []} onChange={(authors) => set({ authors })} placeholder="Add author…" />
          </div>
          <div className="prop">
            <span>Labels</span>
            <TagInput values={p.labels ?? []} onChange={(labels) => set({ labels })} suggestions={project.labels.map((l) => l.name)} />
          </div>
          <div className="prop">
            <span>Supersedes</span>
            <select value={p.supersedes ?? ""} onChange={(e) => set({ supersedes: e.target.value || null })}>
              <option value="">—</option>
              {others.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.id} · {o.title}
                </option>
              ))}
            </select>
          </div>
          {p.superseded_by && (
            <div className="small">
              Superseded by <Link to={`/p/${project.key}/proposals/${p.superseded_by}`}>{p.superseded_by}</Link>
            </div>
          )}
        </div>
        <div className="section" style={{ gap: 8 }}>
          <h3>Tickets</h3>
          {(p.tickets ?? []).length === 0 && <span className="faint small">No tickets yet. Break the proposal into work below, or ask an agent to (the /bankan:plan prompt).</span>}
          {(p.tickets ?? []).map((tid) => {
            const t = tickets.find((x) => x.id === tid);
            const s = project.statuses.find((x) => x.name === t?.status);
            return (
              <div key={tid} className="row small" style={{ gap: 6 }}>
                <StatusDot category={s?.category} color={s?.color} />
                <TicketRefLink id={tid} onOpen={openTicket} />
                <span className="grow" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t?.title ?? ""}</span>
              </div>
            );
          })}
          <input
            placeholder="New ticket title, Enter to create"
            value={newTicket}
            onChange={(e) => setNewTicket(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && newTicket.trim() && !createTicket.isPending && createTicket.mutate()}
          />
        </div>
        <div className="faint small">
          Created {fmtDate(p.created)} · updated {timeAgo(p.updated)}
        </div>
        <div>
          <button className="ghost small danger" onClick={() => confirm(`Delete ${p.id}? Rejected proposals are usually worth keeping for the record.`) && del.mutate()}>
            <I.Trash size={13} /> Delete proposal
          </button>
        </div>
      </aside>
    </div>
  );
}

export default function Proposals() {
  const { project } = useProject();
  const { id } = useParams();
  const [creating, setCreating] = useState(false);
  const list = useQuery({ queryKey: ["proposals", project.key], queryFn: () => api.proposals(project.key) });

  return (
    <div className="page">
      {id ? (
        <ProposalDetail key={id} id={id} />
      ) : (
        <>
          <div className="row" style={{ marginBottom: 18 }}>
            <h1>Proposals</h1>
            <span className="faint small" style={{ marginLeft: 6 }}>Design docs that need a decision: draft → review → accepted or rejected</span>
            <span className="spacer" />
            <button className="primary" onClick={() => setCreating(true)}>
              <I.Plus size={14} /> New proposal
            </button>
          </div>
          {list.data && <ProposalList proposals={list.data} projectKey={project.key} />}
        </>
      )}
      {creating && <NewProposalModal projectKey={project.key} onClose={() => setCreating(false)} />}
    </div>
  );
}
