import { Fragment, useState } from "react";
import { isClosed, PRIORITIES, Ticket } from "../api";
import { Avatar, Chip, pressable, fmtDate, isOverdue, PriorityIcon, StatusDot, timeAgo, useLocalState } from "../ui";
import { categoryOf, useProject } from "./ProjectLayout";

type SortKey = "id" | "title" | "status" | "type" | "priority" | "assignee" | "due" | "updated";
const COLS: [SortKey, string][] = [
  ["id", "Key"],
  ["title", "Title"],
  ["status", "Status"],
  ["type", "Type"],
  ["priority", "Priority"],
  ["assignee", "Assignee"],
  ["due", "Due"],
  ["updated", "Updated"],
];

export default function ListView() {
  const { project, visible, openTicket } = useProject();
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: "status", dir: 1 });
  const [group, setGroup] = useLocalState<"none" | "status" | "type" | "assignee" | "milestone" | "component">(`bankan.group.${project.key}`, "status");

  const statusIndex = (s: string) => project.statuses.findIndex((x) => x.name === s);
  const value = (t: Ticket, k: SortKey): string | number => {
    switch (k) {
      case "id":
        return Number(t.id.split("-").pop());
      case "status":
        return statusIndex(t.status) * 1e7 + t.order;
      case "priority":
        return PRIORITIES.indexOf(t.priority);
      case "due":
        return t.due ?? "9999";
      default:
        return (t[k] ?? "").toString().toLowerCase();
    }
  };
  const sorted = [...visible].sort((a, b) => {
    const va = value(a, sort.key);
    const vb = value(b, sort.key);
    return (va < vb ? -1 : va > vb ? 1 : 0) * sort.dir;
  });

  const groupOf = (t: Ticket) => (group === "none" ? "" : group === "status" ? t.status : group === "type" ? t.type : (t[group] ?? `No ${group}`));
  const groups: [string, Ticket[]][] = [];
  for (const t of sorted) {
    const g = groupOf(t);
    const existing = groups.find(([name]) => name === g);
    if (existing) existing[1].push(t);
    else groups.push([g, [t]]);
  }
  if (group === "status") groups.sort((a, b) => statusIndex(a[0]) - statusIndex(b[0]));

  const header = (k: SortKey, label: string) => (
    <th key={k} {...pressable(() => setSort({ key: k, dir: sort.key === k ? ((-sort.dir) as 1 | -1) : 1 }))} aria-sort={sort.key === k ? (sort.dir === 1 ? "ascending" : "descending") : undefined}>
      {label} {sort.key === k ? (sort.dir === 1 ? "↑" : "↓") : ""}
    </th>
  );

  return (
    <div className="page" style={{ paddingTop: 12 }}>
      <div className="row" style={{ marginBottom: 10 }}>
        <span className="muted small">Group by</span>
        <select value={group} onChange={(e) => setGroup(e.target.value as typeof group)}>
          <option value="none">Nothing</option>
          <option value="status">Status</option>
          <option value="type">Type</option>
          <option value="assignee">Assignee</option>
          <option value="milestone">Milestone</option>
          <option value="component">Component</option>
        </select>
      </div>
      <table className="list">
        <thead>
          <tr>{COLS.map(([k, l]) => header(k, l))}</tr>
        </thead>
        <tbody>
          {groups.map(([name, rows]) => (
            <Fragment key={name || "all"}>{renderGroup(name, rows, group !== "none")}</Fragment>
          ))}
        </tbody>
      </table>
      {visible.length === 0 && <p className="faint" style={{ textAlign: "center", marginTop: 40 }}>No tickets match.</p>}
    </div>
  );

  function renderGroup(name: string, rows: Ticket[], showHeader: boolean) {
    const status = project.statuses.find((s) => s.name === name);
    return (
      <>
        {showHeader && (
          <tr className="group-row">
            <td colSpan={COLS.length}>
              <span className="row" style={{ gap: 6 }}>
                {group === "status" && <StatusDot category={status?.category} color={status?.color} />}
                {name} <span className="faint">{rows.length}</span>
              </span>
            </td>
          </tr>
        )}
        {rows.map((t) => {
          const s = project.statuses.find((x) => x.name === t.status);
          const closed = isClosed(categoryOf(project, t.status));
          return (
            <tr
              key={t.id}
              className="clickable"
              tabIndex={0}
              aria-label={`${t.id}: ${t.title}`}
              onClick={() => openTicket(t.id)}
              onKeyDown={(e) => e.key === "Enter" && e.target === e.currentTarget && openTicket(t.id)}
            >
              <td className="mono muted small">{t.id}</td>
              <td style={{ color: closed ? "var(--text-2)" : undefined }}>
                <span className="row" style={{ gap: 6, flexWrap: "wrap" }}>
                  {t.title}
                  {t.labels?.map((l) => <Chip key={l} label={l} color={project.labels.find((x) => x.name === l)?.color} />)}
                </span>
              </td>
              <td>
                <span className="row" style={{ gap: 6 }}>
                  <StatusDot category={s?.category} color={s?.color} />
                  {t.status}
                </span>
              </td>
              <td style={{ color: project.types.find((x) => x.name === t.type)?.color }}>{t.type}</td>
              <td>
                <PriorityIcon priority={t.priority} />
              </td>
              <td>{t.assignee && <span className="row" style={{ gap: 6 }}><Avatar name={t.assignee} />{t.assignee}</span>}</td>
              <td style={{ color: isOverdue(t.due) && !closed ? "var(--danger)" : undefined }}>{fmtDate(t.due)}</td>
              <td className="faint small">{timeAgo(t.updated)}</td>
            </tr>
          );
        })}
      </>
    );
  }
}
