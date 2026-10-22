import { ProjectSummary, Ticket } from "../api";
import { Avatar, fmtDate, isOverdue, PriorityIcon } from "../ui";
import * as I from "../icons";

export default function TicketCard({ ticket: t, project, closed, overlay, dragging, onClick }: { ticket: Ticket; project: ProjectSummary; closed?: boolean; overlay?: boolean; dragging?: boolean; onClick?: () => void }) {
  const type = project.types.find((x) => x.name === t.type);
  const labelColor = (l: string) => project.labels.find((x) => x.name === l)?.color;
  const hasMeta = t.due || t.milestone || t.comments?.length || t.blocks?.length || t.parent || t.estimate != null || t.branch;
  return (
    <div className={`card${overlay ? " overlay" : ""}${dragging ? " dragging" : ""}${closed ? " closed" : ""}`} onClick={onClick}>
      <div className="card-top">
        <span className="type-dot" style={{ background: type?.color ?? "var(--text-3)" }} title={t.type} />
        <span className="mono">{t.id}</span>
        <span className="spacer" />
        <PriorityIcon priority={t.priority} />
        {t.assignee && <Avatar name={t.assignee} />}
      </div>
      <div className="title">{t.title}</div>
      {!!t.labels?.length && (
        <div className="labels">
          {t.labels.map((l) => (
            <span key={l} className="chip">
              <span className="swatch" style={{ background: labelColor(l) ?? "var(--text-3)" }} />
              {l}
            </span>
          ))}
        </div>
      )}
      {hasMeta && (
        <div className="meta">
          {t.milestone && (
            <span title="Milestone">
              <I.Diamond size={12} /> {t.milestone}
            </span>
          )}
          {t.due && (
            <span title="Due" style={{ color: isOverdue(t.due) && !closed ? "var(--danger)" : undefined }}>
              <I.Calendar size={12} /> {fmtDate(t.due)}
            </span>
          )}
          {t.parent && (
            <span title={`Sub-ticket of ${t.parent}`}>
              <I.Subtask size={12} /> {t.parent}
            </span>
          )}
          {!!t.blocks?.length && (
            <span title={`Blocks ${t.blocks.join(", ")}`} style={{ color: "var(--danger)" }}>
              <I.Blocked size={12} /> {t.blocks.length}
            </span>
          )}
          {t.branch && (
            <span title={t.branch}>
              <I.Branch size={12} />
            </span>
          )}
          {t.estimate != null && <span title="Estimate">{t.estimate} pt</span>}
          {!!t.comments?.length && (
            <span title="Comments">
              <I.Comment size={12} /> {t.comments.length}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
