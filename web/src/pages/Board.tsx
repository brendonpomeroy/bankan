import { KeyboardEvent, useEffect, useMemo, useState } from "react";
import {
  closestCorners,
  DndContext,
  DragEndEvent,
  DragOverEvent,
  DragOverlay,
  DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { arrayMove, SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, isClosed, ProjectSummary, Status, Ticket } from "../api";
import { pressable, StatusDot, useLocalState } from "../ui";
import TicketCard from "../components/TicketCard";
import * as I from "../icons";
import { useProject } from "./ProjectLayout";

type Columns = Record<string, string[]>;

function buildColumns(statuses: Status[], tickets: Ticket[]): Columns {
  const cols: Columns = Object.fromEntries(statuses.map((s) => [s.name, []]));
  for (const t of [...tickets].sort((a, b) => a.order - b.order)) cols[t.status]?.push(t.id);
  return cols;
}

function SortableCard({ ticket, project, closed, onOpen }: { ticket: Ticket; project: ProjectSummary; closed: boolean; onOpen: () => void }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: ticket.id });
  // Space picks the card up (dnd-kit KeyboardSensor); Enter opens it.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    listeners?.onKeyDown?.(e);
    if (e.key === "Enter" && !isDragging && e.target === e.currentTarget) onOpen();
  };
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      {...attributes}
      {...listeners}
      onKeyDown={onKeyDown}
      aria-label={`${ticket.id}: ${ticket.title}`}
    >
      <TicketCard ticket={ticket} project={project} closed={closed} dragging={isDragging} onClick={onOpen} />
    </div>
  );
}

function Column({ status, ids, byId, project, collapsed, onToggle, onOpen, onAdd }: {
  status: Status;
  ids: string[];
  byId: Map<string, Ticket>;
  project: ProjectSummary;
  collapsed: boolean;
  onToggle: () => void;
  onOpen: (id: string) => void;
  onAdd: () => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `col:${status.name}` });
  const closed = isClosed(status.category);
  if (collapsed) {
    return (
      <div ref={setNodeRef} className={`column collapsed${isOver ? " over" : ""}`} {...pressable(onToggle)} title={`Expand ${status.name}`} aria-label={`Expand ${status.name} (${ids.length})`}>
        <StatusDot category={status.category} color={status.color} />
        <span className="faint small">{ids.length}</span>
        <span className="col-title-vertical">{status.name}</span>
      </div>
    );
  }
  return (
    <div className={`column${isOver ? " over" : ""}`}>
      <div className="col-head">
        <StatusDot category={status.category} color={status.color} />
        {status.name}
        <span className="count">{ids.length}</span>
        <span className="spacer" />
        <button className="ghost small icon" title="Collapse column" onClick={onToggle}>
          <I.ChevronLeft size={14} />
        </button>
        <button className="ghost small icon" title={`New ticket in ${status.name}`} onClick={onAdd}>
          <I.Plus size={14} />
        </button>
      </div>
      <SortableContext id={`col:${status.name}`} items={ids} strategy={verticalListSortingStrategy}>
        <div ref={setNodeRef} className="col-body">
          {ids.map((id) => {
            const t = byId.get(id);
            return t ? <SortableCard key={id} ticket={t} project={project} closed={closed} onOpen={() => onOpen(id)} /> : null;
          })}
          {ids.length === 0 && <div className="col-empty">No tickets</div>}
        </div>
      </SortableContext>
    </div>
  );
}

export default function Board() {
  const { project, tickets, visible, openTicket, newTicket } = useProject();
  const qc = useQueryClient();
  const [collapsed, setCollapsed] = useLocalState<string[]>(
    `bankan.collapsed.${project.key}`,
    project.statuses.filter((s) => s.category === "canceled").map((s) => s.name),
  );
  const byId = useMemo(() => new Map(tickets.map((t) => [t.id, t])), [tickets]);
  const [cols, setCols] = useState<Columns>(() => buildColumns(project.statuses, visible));
  const [activeId, setActiveId] = useState<string | null>(null);
  const [dropping, setDropping] = useState(false);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates, keyboardCodes: { start: ["Space"], cancel: ["Escape"], end: ["Space", "Enter"] } }),
  );

  const move = useMutation({
    mutationFn: ({ id, status, index }: { id: string; status: string; index: number; order: number }) => api.moveTicket(id, status, index),
    // Optimistic update (status and the order the server will assign) so the card doesn't jump while the request runs.
    onMutate: async ({ id, status, order }) => {
      await qc.cancelQueries({ queryKey: ["tickets", project.key] });
      qc.setQueryData<Ticket[]>(["tickets", project.key], (old) => old?.map((t) => (t.id === id ? { ...t, status, order } : t)));
    },
    onSettled: () => {
      setDropping(false);
      qc.invalidateQueries({ queryKey: ["tickets", project.key] });
      qc.invalidateQueries({ queryKey: ["projects"] });
    },
  });

  // Re-sync from the server unless a drag or drop is in progress.
  useEffect(() => {
    if (!activeId && !dropping) setCols(buildColumns(project.statuses, visible));
  }, [visible, project.statuses, activeId, dropping]);

  const containerOf = (id: string) => (id.startsWith("col:") ? id.slice(4) : Object.keys(cols).find((k) => cols[k].includes(id)));

  const onDragStart = (e: DragStartEvent) => setActiveId(String(e.active.id));

  const onDragOver = ({ active, over }: DragOverEvent) => {
    if (!over) return;
    const from = containerOf(String(active.id));
    const to = containerOf(String(over.id));
    if (!from || !to || from === to) return;
    setCols((prev) => {
      const source = prev[from].filter((x) => x !== active.id);
      const target = [...prev[to]];
      const overIndex = target.indexOf(String(over.id));
      target.splice(overIndex >= 0 ? overIndex : target.length, 0, String(active.id));
      return { ...prev, [from]: source, [to]: target };
    });
  };

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    const id = String(active.id);
    setActiveId(null);
    if (!over) return setCols(buildColumns(project.statuses, visible));
    const status = containerOf(id);
    if (!status) return;
    let list = cols[status];
    const overId = String(over.id);
    if (!overId.startsWith("col:") && list.includes(overId) && overId !== id) {
      list = arrayMove(list, list.indexOf(id), list.indexOf(overId));
    }
    setCols({ ...cols, [status]: list });

    // The board may be filtered, so translate the visible position into a
    // position among *all* tickets in that status (the server's index).
    const pos = list.indexOf(id);
    const fullTickets = tickets.filter((t) => t.status === status && t.id !== id).sort((a, b) => a.order - b.order);
    const full = fullTickets.map((t) => t.id);
    const next = list[pos + 1];
    const prev = list[pos - 1];
    const index = next ? full.indexOf(next) : prev ? full.indexOf(prev) + 1 : full.length;
    const before = byId.get(id);
    if (before && before.status === status) {
      const oldIndex = full.findIndex((x) => (byId.get(x)?.order ?? 0) > before.order);
      if ((oldIndex === -1 ? full.length : oldIndex) === index) return; // dropped where it was
    }
    // Same rule as the server's move_ticket: midpoint between neighbours, ±1000 at the ends.
    const b = fullTickets[index - 1]?.order;
    const a = fullTickets[index]?.order;
    const order = b == null ? (a == null ? 1000 : a - 1000) : a == null ? b + 1000 : (a + b) / 2;
    setDropping(true);
    move.mutate({ id, status, index, order });
  };

  const toggle = (name: string) => setCollapsed(collapsed.includes(name) ? collapsed.filter((c) => c !== name) : [...collapsed, name]);
  const active = activeId ? byId.get(activeId) : undefined;

  return (
    <DndContext sensors={sensors} collisionDetection={closestCorners} onDragStart={onDragStart} onDragOver={onDragOver} onDragEnd={onDragEnd} onDragCancel={() => setActiveId(null)}>
      <div className="board">
        {project.statuses.map((s) => (
          <Column
            key={s.name}
            status={s}
            ids={cols[s.name] ?? []}
            byId={byId}
            project={project}
            collapsed={collapsed.includes(s.name)}
            onToggle={() => toggle(s.name)}
            onOpen={openTicket}
            onAdd={() => newTicket(s.name)}
          />
        ))}
      </div>
      <DragOverlay>{active ? <TicketCard ticket={active} project={project} overlay /> : null}</DragOverlay>
    </DndContext>
  );
}
