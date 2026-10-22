import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, ApiError, Category, CATEGORIES, FieldKind, Milestone, Named, Project, Status } from "../api";
import { StatusDot, toast } from "../ui";
import { useProject } from "./ProjectLayout";

// Each editable row has a stable id (for React keys) and remembers its original name so renames
// can be applied to existing tickets.
type Row<T> = T & { id: string; orig?: string };
type FieldRow = Row<{ name: string; kind: FieldKind; opts: string }>;

let nextId = 0;
const rowId = () => `r${++nextId}`;

function withOrig<T extends { name: string }>(items: T[]): Row<T>[] {
  return items.map((i, n) => ({ ...i, id: `o${n}`, orig: i.name }));
}

function renamesOf(rows: { name: string; orig?: string }[]) {
  return Object.fromEntries(rows.filter((r) => r.orig && r.orig !== r.name.trim()).map((r) => [r.orig!, r.name.trim()]));
}

function strip<T extends { name: string }>(rows: Row<T>[]): T[] {
  return rows.map(({ orig: _orig, id: _id, ...rest }) => ({ ...(rest as unknown as T), name: rest.name.trim() }));
}

function toForm(p: Project) {
  return {
    name: p.name,
    description: p.description,
    color: p.color ?? "#5b6cf9",
    statuses: withOrig(p.statuses),
    types: withOrig(p.types),
    labels: withOrig(p.labels),
    milestones: withOrig(p.milestones),
    fields: p.fields.map((f, n): FieldRow => ({ id: `o${n}`, orig: f.name, name: f.name, kind: f.kind, opts: (f.options ?? []).join(", ") })),
  };
}
type Form = ReturnType<typeof toForm>;

function move<T>(list: T[], i: number, d: number): T[] {
  const j = i + d;
  if (j < 0 || j >= list.length) return list;
  const out = [...list];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

function RowControls({ i, n, onMove, onDelete }: { i: number; n: number; onMove: (d: number) => void; onDelete: () => void }) {
  return (
    <>
      <button className="ghost small icon" disabled={i === 0} onClick={() => onMove(-1)} title="Move up">↑</button>
      <button className="ghost small icon" disabled={i === n - 1} onClick={() => onMove(1)} title="Move down">↓</button>
      <button className="ghost small icon danger" onClick={onDelete} title="Remove">✕</button>
    </>
  );
}

export default function ProjectSettings() {
  const { project } = useProject();
  const qc = useQueryClient();
  const navigate = useNavigate();
  // `loaded` is the project version the form was filled from; saving sends its timestamp so the
  // server rejects (409) a save over changes made since, e.g. labels an agent added.
  const [loaded, setLoaded] = useState(() => ({ updated: project.updated, form: toForm(project) }));
  const [form, setForm] = useState<Form>(loaded.form);
  const { name, description, color, statuses, types, labels, milestones, fields } = form;
  const dirty = JSON.stringify(form) !== JSON.stringify(loaded.form);
  const reload = (p: Project) => {
    const f = toForm(p);
    setLoaded({ updated: p.updated, form: f });
    setForm(f);
  };
  const upd = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const setName = (name: string) => upd({ name });
  const setDescription = (description: string) => upd({ description });
  const setColor = (color: string) => upd({ color });
  const setStatuses = (statuses: Row<Status>[]) => upd({ statuses });
  const setTypes = (types: Row<Named>[]) => upd({ types });
  const setLabels = (labels: Row<Named>[]) => upd({ labels });
  const setMilestones = (milestones: Row<Milestone>[]) => upd({ milestones });
  const setFields = (fields: FieldRow[]) => upd({ fields });

  const save = useMutation({
    mutationFn: () =>
      api.updateProject(project.key, {
        name,
        description,
        color,
        statuses: strip(statuses),
        types: strip(types),
        labels: strip(labels),
        milestones: strip(milestones),
        fields: fields.map((f) => ({ name: f.name.trim(), kind: f.kind, options: f.kind === "select" ? f.opts.split(",").map((o) => o.trim()).filter(Boolean) : undefined })),
        renames: { statuses: renamesOf(statuses), types: renamesOf(types), labels: renamesOf(labels), milestones: renamesOf(milestones), fields: renamesOf(fields) },
        expected_updated: loaded.updated,
      }),
    onSuccess: (p) => {
      qc.invalidateQueries();
      reload(p);
      toast("Project saved");
    },
  });
  const conflict = save.error instanceof ApiError && save.error.status === 409;

  // Pick up outside changes automatically while there are no unsaved edits.
  useEffect(() => {
    if (project.updated !== loaded.updated && !dirty && !save.isPending) reload(project);
  }, [project, loaded.updated, dirty, save.isPending]);
  const archive = useMutation({
    mutationFn: (archived: boolean) => api.updateProject(project.key, { archived }),
    onSuccess: () => qc.invalidateQueries(),
  });
  const del = useMutation({
    mutationFn: () => api.deleteProject(project.key),
    onSuccess: () => {
      qc.invalidateQueries();
      navigate("/");
    },
  });

  const removedStatuses = project.statuses.filter((s) => !statuses.some((r) => r.orig === s.name)).map((s) => s.name);

  return (
    <div className="page narrow col" style={{ gap: 16 }}>
      <div className="row">
        <h1>Project settings</h1>
        <span className="spacer" />
        <button className="primary" onClick={() => save.mutate()} disabled={save.isPending || !dirty}>
          Save changes
        </button>
      </div>
      {conflict && (
        <div className="notice" role="alert">
          <span className="grow">The project was changed elsewhere (perhaps an agent added labels) since this form was loaded. Your changes weren't saved.</span>
          <button
            className="small"
            onClick={() => {
              reload(project);
              save.reset();
            }}
          >
            Reload latest (discard my edits)
          </button>
        </div>
      )}

      <section className="section">
        <h2>General</h2>
        <div className="row" style={{ alignItems: "flex-end" }}>
          <label className="field" style={{ width: 90 }}>
            Key
            <input value={project.key} disabled className="mono" />
          </label>
          <label className="field grow">
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="field">
            Colour
            <input type="color" value={color} onChange={(e) => setColor(e.target.value)} />
          </label>
        </div>
        <label className="field">
          Description
          <textarea rows={2} value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>Statuses</h2>
          <span className="faint small">Board columns, left to right. Each belongs to a category so agents and filters know what's open, active or closed.</span>
        </div>
        {statuses.map((s, i) => (
          <div key={s.id} className="editor-row">
            <StatusDot category={s.category} color={s.color} />
            <input type="text" value={s.name} onChange={(e) => setStatuses(statuses.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <select value={s.category} onChange={(e) => setStatuses(statuses.map((x, j) => (j === i ? { ...x, category: e.target.value as Category } : x)))}>
              {CATEGORIES.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </select>
            <input type="color" value={s.color ?? "#8b8d98"} onChange={(e) => setStatuses(statuses.map((x, j) => (j === i ? { ...x, color: e.target.value } : x)))} />
            <RowControls i={i} n={statuses.length} onMove={(d) => setStatuses(move(statuses, i, d))} onDelete={() => statuses.length > 1 && setStatuses(statuses.filter((_, j) => j !== i))} />
          </div>
        ))}
        {removedStatuses.length > 0 && (
          <div className="faint small">Tickets in {removedStatuses.join(", ")} will move to the first remaining status in the same category.</div>
        )}
        <div>
          <button className="small" onClick={() => setStatuses([...statuses, { id: rowId(), name: "New status", category: "started", color: "#8b8d98" }])}>
            + Add status
          </button>
        </div>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>Ticket types</h2>
          <span className="faint small">The first type is the default.</span>
        </div>
        {types.map((t, i) => (
          <div key={t.id} className="editor-row">
            <input type="text" value={t.name} onChange={(e) => setTypes(types.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <input type="color" value={t.color ?? "#8b8d98"} onChange={(e) => setTypes(types.map((x, j) => (j === i ? { ...x, color: e.target.value } : x)))} />
            <RowControls i={i} n={types.length} onMove={(d) => setTypes(move(types, i, d))} onDelete={() => types.length > 1 && setTypes(types.filter((_, j) => j !== i))} />
          </div>
        ))}
        <div>
          <button className="small" onClick={() => setTypes([...types, { id: rowId(), name: "New type", color: "#8b8d98" }])}>
            + Add type
          </button>
        </div>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>Labels</h2>
          <span className="faint small">Labels used on tickets are added here automatically. Removing one removes it from tickets.</span>
        </div>
        {labels.length === 0 && <span className="faint small">No labels yet.</span>}
        {labels.map((l, i) => (
          <div key={l.id} className="editor-row">
            <input type="text" value={l.name} onChange={(e) => setLabels(labels.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <input type="color" value={l.color ?? "#8b8d98"} onChange={(e) => setLabels(labels.map((x, j) => (j === i ? { ...x, color: e.target.value } : x)))} />
            <RowControls i={i} n={labels.length} onMove={(d) => setLabels(move(labels, i, d))} onDelete={() => setLabels(labels.filter((_, j) => j !== i))} />
          </div>
        ))}
        <div>
          <button className="small" onClick={() => setLabels([...labels, { id: rowId(), name: "new-label", color: "#8b8d98" }])}>
            + Add label
          </button>
        </div>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>Milestones</h2>
          <span className="faint small">Group tickets into releases or goals.</span>
        </div>
        {milestones.map((m, i) => (
          <div key={m.id} className="editor-row">
            <input type="text" value={m.name} onChange={(e) => setMilestones(milestones.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <input type="date" value={m.due ?? ""} onChange={(e) => setMilestones(milestones.map((x, j) => (j === i ? { ...x, due: e.target.value || undefined } : x)))} />
            <label className="row small muted" style={{ gap: 4 }}>
              <input type="checkbox" checked={m.done} onChange={(e) => setMilestones(milestones.map((x, j) => (j === i ? { ...x, done: e.target.checked } : x)))} />
              done
            </label>
            <RowControls i={i} n={milestones.length} onMove={(d) => setMilestones(move(milestones, i, d))} onDelete={() => setMilestones(milestones.filter((_, j) => j !== i))} />
          </div>
        ))}
        <div>
          <button className="small" onClick={() => setMilestones([...milestones, { id: rowId(), name: `v0.${milestones.length + 1}`, done: false }])}>
            + Add milestone
          </button>
        </div>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>Custom fields</h2>
          <span className="faint small">Extra properties on every ticket, e.g. Severity (select) or Customer (text).</span>
        </div>
        {fields.map((f, i) => (
          <div key={f.id} className="editor-row">
            <input type="text" value={f.name} placeholder="Field name" onChange={(e) => setFields(fields.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <select value={f.kind} onChange={(e) => setFields(fields.map((x, j) => (j === i ? { ...x, kind: e.target.value as FieldKind } : x)))}>
              {["text", "number", "select", "date", "url"].map((k) => (
                <option key={k}>{k}</option>
              ))}
            </select>
            {f.kind === "select" && (
              <input
                type="text"
                placeholder="Options, comma-separated"
                value={f.opts}
                onChange={(e) => setFields(fields.map((x, j) => (j === i ? { ...x, opts: e.target.value } : x)))}
              />
            )}
            <RowControls i={i} n={fields.length} onMove={(d) => setFields(move(fields, i, d))} onDelete={() => setFields(fields.filter((_, j) => j !== i))} />
          </div>
        ))}
        <div>
          <button className="small" onClick={() => setFields([...fields, { id: rowId(), name: "", kind: "text", opts: "" }])}>
            + Add field
          </button>
        </div>
      </section>

      <section className="section">
        <h2>Link a repo</h2>
        <p className="muted" style={{ margin: 0 }}>
          Run this in the repo so agents (via <code>bankan mcp</code>) default to this project. It writes a one-line <code>.bankan</code> file that never changes, so it can't cause merge conflicts.
        </p>
        <pre className="md" style={{ margin: 0 }}>
          <code>bankan link {project.key}</code>
        </pre>
      </section>

      <section className="section">
        <h2>Danger zone</h2>
        <div className="row">
          <button onClick={() => archive.mutate(!project.archived)}>{project.archived ? "Unarchive project" : "Archive project"}</button>
          <span className="faint small">Archived projects are hidden from the sidebar and search.</span>
        </div>
        <div className="row">
          <button
            className="danger"
            onClick={() => {
              const typed = prompt(`This deletes every ticket, doc and proposal in ${project.key}. Type the project key to confirm.`);
              if (typed?.toUpperCase() === project.key) del.mutate();
            }}
          >
            Delete project
          </button>
          <span className="faint small">Recoverable only from a git backup.</span>
        </div>
      </section>
    </div>
  );
}
