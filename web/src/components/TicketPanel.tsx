import { useEffect, useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ProjectSummary, safeUrl, Ticket } from "../api";
import { Avatar, fmtDate, InlineInput, Markdown, MarkdownField, PrioritySelect, StatusDot, TagInput, TicketRefLink, timeAgo, toast } from "../ui";
import * as I from "../icons";

const BAD_URL = "Links must start with http://, https:// or mailto:";

/** What a PATCH will make the ticket look like, for optimistic updates. `fields` merges; null clears. */
function applyPatch(t: Ticket, patch: Record<string, unknown>): Ticket {
  const next: Record<string, unknown> = { ...t };
  for (const [k, v] of Object.entries(patch)) {
    if (k === "fields") {
      const fields = { ...t.fields };
      for (const [name, val] of Object.entries(v as Record<string, unknown>)) {
        if (val == null) delete fields[name];
        else fields[name] = val;
      }
      next.fields = fields;
    } else next[k] = v ?? undefined;
  }
  return next as unknown as Ticket;
}

function Prop({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="prop">
      <span>{label}</span>
      <div>{children}</div>
    </div>
  );
}

/** A list of ticket ids shown as chips, removable, plus an input to add more. */
function TicketRefs({ ids, onChange, onOpen }: { ids: string[]; onChange?: (ids: string[]) => void; onOpen: (id: string) => void }) {
  const [draft, setDraft] = useState("");
  return (
    <div className="prop-stack">
      {ids.length > 0 && (
        <div className="chips">
          {ids.map((id) => (
            <span key={id} className="chip">
              <TicketRefLink id={id} onOpen={onOpen} />
              {onChange && (
                <button onClick={() => onChange(ids.filter((x) => x !== id))} aria-label={`Remove ${id}`}>
                  <I.X size={11} />
                </button>
              )}
            </span>
          ))}
        </div>
      )}
      {onChange && (
        <input
          placeholder="Add ticket…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            const v = draft.trim().toUpperCase();
            if (e.key === "Enter" && v) {
              if (!ids.includes(v)) onChange([...ids, v]);
              setDraft("");
            }
          }}
        />
      )}
    </div>
  );
}

export default function TicketPanel({ id, project, onClose, onOpen }: { id: string; project: ProjectSummary; onClose: () => void; onOpen: (id: string) => void }) {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["ticket", id], queryFn: () => api.ticket(id) });
  const proposals = useQuery({ queryKey: ["proposals", project.key], queryFn: () => api.proposals(project.key) });
  const [comment, setComment] = useState("");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = ["INPUT", "TEXTAREA", "SELECT"].includes((e.target as HTMLElement).tagName);
      if (e.key === "Escape" && !typing) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["ticket"] });
    qc.invalidateQueries({ queryKey: ["tickets", project.key] });
    qc.invalidateQueries({ queryKey: ["projects"] });
    qc.invalidateQueries({ queryKey: ["project", project.key] });
  };
  // One mutation for every property edit. It's applied to the cached ticket immediately so a quick
  // second edit (e.g. adding two labels) builds on the first rather than on the last server response.
  const mutationKey = ["patchTicket", id];
  const update = useMutation({
    mutationKey,
    mutationFn: (patch: Record<string, unknown>) => api.updateTicket(id, patch),
    onMutate: async (patch) => {
      await qc.cancelQueries({ queryKey: ["ticket", id] });
      const prev = qc.getQueryData<Ticket>(["ticket", id]);
      qc.setQueryData<Ticket>(["ticket", id], (old) => old && applyPatch(old, patch));
      return { prev };
    },
    onError: (_e, _patch, ctx) => ctx?.prev && qc.setQueryData(["ticket", id], ctx.prev),
    onSuccess: (t) => {
      // Later edits still in flight already reflect this one; don't clobber them with this response.
      if (qc.isMutating({ mutationKey }) === 1) qc.setQueryData<Ticket>(["ticket", id], (old) => (old ? { ...old, ...t } : t));
    },
    onSettled: () => qc.isMutating({ mutationKey }) === 1 && refresh(),
  });
  const addComment = useMutation({
    mutationFn: () => api.comment(id, comment),
    onSuccess: () => {
      setComment("");
      refresh();
    },
  });
  const delComment = useMutation({ mutationFn: (cid: number) => api.deleteComment(id, cid), onSuccess: refresh });
  const del = useMutation({
    mutationFn: () => api.deleteTicket(id),
    onSuccess: () => {
      toast(`Deleted ${id}`);
      refresh();
      onClose();
    },
  });

  const set = (patch: Record<string, unknown>) => update.mutate(patch);
  const t = q.data;
  const status = t && project.statuses.find((s) => s.name === t.status);
  const sendComment = () => comment.trim() && !addComment.isPending && addComment.mutate();

  return (
    <>
      <div className="scrim" onClick={onClose} />
      <aside className="drawer" role="dialog" aria-label={`Ticket ${id}`}>
        <div className="drawer-head">
          <span className="project-key" style={{ background: project.color ?? undefined }}>
            {project.key}
          </span>
          <span className="mono muted small">{id}</span>
          {t && (
            <button
              className="ghost small icon"
              title="Copy ticket key"
              onClick={() => {
                navigator.clipboard?.writeText(id);
                toast(`Copied ${id}`);
              }}
            >
              <I.Copy size={13} />
            </button>
          )}
          <span className="spacer" />
          {t && <span className="faint small">Updated {timeAgo(t.updated)}</span>}
          {t && (
            <button className="ghost small icon" title="Delete ticket" onClick={() => confirm(`Delete ${id}? Only a git backup can bring it back.`) && del.mutate()}>
              <I.Trash size={14} />
            </button>
          )}
          <button className="ghost icon" onClick={onClose} title="Close (Esc)">
            <I.X size={16} />
          </button>
        </div>
        {q.error && <div className="drawer-main error">{q.error.message}</div>}
        {!t && !q.error && <div className="drawer-main faint">Loading…</div>}
        {t && (
          <div className="drawer-body">
            <div className="drawer-main">
              <InlineInput className="title-input" value={t.title} onCommit={(v) => v.trim() && set({ title: v })} />
              <div style={{ marginTop: 14 }}>
                <MarkdownField value={t.description} onSave={(v) => set({ description: v })} />
              </div>

              {!!(t.children?.length || t.blocked_by?.length) && (
                <div className="col" style={{ marginTop: 26, gap: 8 }}>
                  {!!t.children?.length && (
                    <div className="row small" style={{ alignItems: "flex-start" }}>
                      <span className="muted row" style={{ gap: 5, width: 96 }}>
                        <I.Subtask size={13} /> Sub-tickets
                      </span>
                      <TicketRefs ids={t.children} onOpen={onOpen} />
                    </div>
                  )}
                  {!!t.blocked_by?.length && (
                    <div className="row small" style={{ alignItems: "flex-start" }}>
                      <span className="row" style={{ gap: 5, width: 96, color: "var(--danger)" }}>
                        <I.Blocked size={13} /> Blocked by
                      </span>
                      <TicketRefs ids={t.blocked_by} onOpen={onOpen} />
                    </div>
                  )}
                </div>
              )}

              <hr style={{ margin: "30px 0 18px" }} />
              <h3 style={{ marginBottom: 14 }}>Activity</h3>
              <div className="col" style={{ gap: 14 }}>
                {t.comments?.map((c) => (
                  <div key={c.id} className="comment">
                    <Avatar name={c.author} />
                    <div className="comment-bubble">
                      <div className="comment-head">
                        <b>{c.author}</b>
                        <span title={new Date(c.at).toLocaleString()}>{timeAgo(c.at)}</span>
                        <span className="spacer" />
                        <button className="ghost small icon" title="Delete comment" onClick={() => confirm("Delete this comment?") && delComment.mutate(c.id)}>
                          <I.Trash size={12} />
                        </button>
                      </div>
                      <Markdown text={c.body} />
                    </div>
                  </div>
                ))}
                <div className="composer">
                  <textarea
                    rows={3}
                    placeholder="Leave a comment…"
                    value={comment}
                    onChange={(e) => setComment(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && sendComment()}
                  />
                  <div className="row">
                    <span className="faint small">Markdown supported</span>
                    <span className="spacer" />
                    <button className="primary small" disabled={!comment.trim() || addComment.isPending} onClick={sendComment}>
                      Comment <span className="faint" style={{ color: "rgba(255,255,255,.7)" }}>⌘↵</span>
                    </button>
                  </div>
                </div>
              </div>
              <div className="faint small" style={{ marginTop: 26 }}>
                Created {fmtDate(t.created)}
                {t.closed && ` · Closed ${fmtDate(t.closed)}`}
              </div>
            </div>

            <div className="drawer-side">
              <div className="side-title">Properties</div>
              <Prop label="Status">
                <div className="row" style={{ gap: 0 }}>
                  <span style={{ paddingLeft: 8, display: "flex" }}>
                    <StatusDot category={status?.category} color={status?.color} />
                  </span>
                  <select value={t.status} onChange={(e) => set({ status: e.target.value })}>
                    {project.statuses.map((s) => (
                      <option key={s.name}>{s.name}</option>
                    ))}
                  </select>
                </div>
              </Prop>
              <Prop label="Type">
                <select value={t.type} onChange={(e) => set({ type: e.target.value })}>
                  {project.types.map((x) => (
                    <option key={x.name}>{x.name}</option>
                  ))}
                </select>
              </Prop>
              <Prop label="Priority">
                <PrioritySelect value={t.priority} onChange={(priority) => set({ priority })} />
              </Prop>
              <Prop label="Assignee">
                <InlineInput value={t.assignee ?? ""} placeholder="Unassigned" onCommit={(v) => set({ assignee: v.trim() || null })} />
              </Prop>
              <Prop label="Labels">
                <TagInput
                  values={t.labels ?? []}
                  onChange={(labels) => set({ labels })}
                  suggestions={project.labels.map((l) => l.name)}
                  colorOf={(v) => project.labels.find((l) => l.name === v)?.color}
                />
              </Prop>
              {project.milestones.length > 0 && (
                <Prop label="Milestone">
                  <select value={t.milestone ?? ""} onChange={(e) => set({ milestone: e.target.value || null })}>
                    <option value="">None</option>
                    {project.milestones.map((m) => (
                      <option key={m.name}>{m.name}</option>
                    ))}
                  </select>
                </Prop>
              )}
              <Prop label="Estimate">
                <InlineInput type="number" value={t.estimate?.toString() ?? ""} placeholder="No estimate" onCommit={(v) => set({ estimate: v === "" ? null : Number(v) })} />
              </Prop>
              <Prop label="Due date">
                <input type="date" value={t.due ?? ""} onChange={(e) => set({ due: e.target.value || null })} />
              </Prop>

              {project.fields.length > 0 && <div className="side-title">Fields</div>}
              {project.fields.map((f) => {
                const v = t.fields?.[f.name];
                const commit = (val: unknown) => set({ fields: { [f.name]: val === "" ? null : val } });
                return (
                  <Prop key={f.name} label={f.name}>
                    {f.kind === "select" ? (
                      <select value={(v as string) ?? ""} onChange={(e) => commit(e.target.value)}>
                        <option value="">None</option>
                        {f.options?.map((o) => (
                          <option key={o}>{o}</option>
                        ))}
                      </select>
                    ) : f.kind === "date" ? (
                      <input type="date" value={(v as string) ?? ""} onChange={(e) => commit(e.target.value)} />
                    ) : f.kind === "number" ? (
                      <InlineInput type="number" value={v == null ? "" : String(v)} placeholder="Empty" onCommit={(x) => commit(x === "" ? null : Number(x))} />
                    ) : (
                      <div className="row" style={{ gap: 2 }}>
                        <InlineInput
                          value={(v as string) ?? ""}
                          placeholder="Empty"
                          onCommit={(x) => (f.kind === "url" && x.trim() && !safeUrl(x) ? toast(BAD_URL) : commit(x.trim()))}
                        />
                        {f.kind === "url" && safeUrl(v) && (
                          <a href={safeUrl(v)} target="_blank" rel="noreferrer" title="Open">
                            <I.External size={13} />
                          </a>
                        )}
                      </div>
                    )}
                  </Prop>
                );
              })}

              <div className="side-title">Relations</div>
              <Prop label="Parent">
                <InlineInput value={t.parent ?? ""} placeholder="None" onCommit={(v) => set({ parent: v.trim().toUpperCase() || null })} />
              </Prop>
              <Prop label="Blocks">
                <TicketRefs ids={t.blocks ?? []} onChange={(blocks) => set({ blocks })} onOpen={onOpen} />
              </Prop>
              <Prop label="Related">
                <TicketRefs ids={t.related ?? []} onChange={(related) => set({ related })} onOpen={onOpen} />
              </Prop>
              <Prop label="Duplicate of">
                <InlineInput value={t.duplicate_of ?? ""} placeholder="None" onCommit={(v) => set({ duplicate_of: v.trim().toUpperCase() || null })} />
              </Prop>

              <div className="side-title">Development</div>
              <Prop label="Proposal">
                <div className="row" style={{ gap: 2 }}>
                  <select value={t.proposal ?? ""} onChange={(e) => set({ proposal: e.target.value || null })}>
                    <option value="">None</option>
                    {proposals.data?.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.id} · {p.title}
                      </option>
                    ))}
                  </select>
                  {t.proposal && (
                    <RouterLink to={`/p/${project.key}/proposals/${t.proposal}`} title="Open proposal">
                      <I.External size={13} />
                    </RouterLink>
                  )}
                </div>
              </Prop>
              <Prop label="Branch">
                <InlineInput className="mono" value={t.branch ?? ""} placeholder="None" onCommit={(v) => set({ branch: v.trim() || null })} />
              </Prop>
              <Prop label="Links">
                <div className="prop-stack">
                  {t.links?.map((l, i) => (
                    <div key={i} className="row small" style={{ gap: 4, padding: "3px 0 3px 8px" }}>
                      <I.LinkIcon size={12} />
                      {safeUrl(l.url) ? (
                        <a href={safeUrl(l.url)} target="_blank" rel="noreferrer" className="grow ellipsis">
                          {l.title || l.url.replace(/^https?:\/\//, "")}
                        </a>
                      ) : (
                        <span className="grow ellipsis muted" title="Not an http(s) or mailto link, so it isn't clickable">
                          {l.title || l.url}
                        </span>
                      )}
                      <button className="ghost small icon" onClick={() => set({ links: t.links!.filter((_, j) => j !== i) })} aria-label="Remove link">
                        <I.X size={12} />
                      </button>
                    </div>
                  ))}
                  <input
                    placeholder="Paste a URL…"
                    onKeyDown={(e) => {
                      const v = (e.target as HTMLInputElement).value.trim();
                      if (e.key === "Enter" && v) {
                        if (!safeUrl(v)) return toast(BAD_URL);
                        set({ links: [...(t.links ?? []), { url: v }] });
                        (e.target as HTMLInputElement).value = "";
                      }
                    }}
                  />
                </div>
              </Prop>
            </div>
          </div>
        )}
      </aside>
    </>
  );
}
