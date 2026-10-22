import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, Priority, ProjectSummary, Ticket } from "../api";
import { Modal, PrioritySelect, TagInput } from "../ui";

export default function NewTicketModal({ project, status, onClose, onCreated }: { project: ProjectSummary; status?: string; onClose: () => void; onCreated: (t: Ticket, open: boolean) => void }) {
  const qc = useQueryClient();
  const defaultStatus = status ?? project.statuses.find((s) => s.category === "unstarted")?.name ?? project.statuses[0].name;
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [type, setType] = useState(project.types[0].name);
  const [st, setSt] = useState(defaultStatus);
  const [priority, setPriority] = useState<Priority>("none");
  const [labels, setLabels] = useState<string[]>([]);
  const [another, setAnother] = useState(false);

  const create = useMutation({
    mutationFn: () => api.createTicket(project.key, { title, description, type, status: st, priority, labels }),
    onSuccess: (t) => {
      qc.invalidateQueries({ queryKey: ["tickets", project.key] });
      qc.invalidateQueries({ queryKey: ["projects"] });
      if (another) {
        setTitle("");
        setDescription("");
        onCreated(t, false);
      } else onCreated(t, true);
    },
    meta: { quiet: true },
  });

  const submit = () => title.trim() && !create.isPending && create.mutate();

  return (
    <Modal onClose={onClose} wide>
      <div className="row">
        <span className="project-key">{project.key}</span>
        <h2>New ticket</h2>
      </div>
      <input
        autoFocus
        className="title-input"
        placeholder="Ticket title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && submit()}
      />
      <textarea
        className="code"
        rows={6}
        placeholder="Description (Markdown, optional)"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && submit()}
      />
      <div className="row" style={{ flexWrap: "wrap" }}>
        <select value={st} onChange={(e) => setSt(e.target.value)}>
          {project.statuses.map((s) => (
            <option key={s.name}>{s.name}</option>
          ))}
        </select>
        <select value={type} onChange={(e) => setType(e.target.value)}>
          {project.types.map((t) => (
            <option key={t.name}>{t.name}</option>
          ))}
        </select>
        <PrioritySelect value={priority} onChange={setPriority} />
      </div>
      <TagInput values={labels} onChange={setLabels} suggestions={project.labels.map((l) => l.name)} placeholder="Labels (Enter to add)" colorOf={(v) => project.labels.find((l) => l.name === v)?.color} />
      {create.error && <div className="error">{String(create.error.message)}</div>}
      <div className="row">
        <label className="row small muted" style={{ gap: 6 }}>
          <input type="checkbox" checked={another} onChange={(e) => setAnother(e.target.checked)} />
          Create another
        </label>
        <span className="spacer" />
        <button className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button className="primary" onClick={submit} disabled={!title.trim() || create.isPending}>
          Create ticket
        </button>
      </div>
    </Modal>
  );
}
