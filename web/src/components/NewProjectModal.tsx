import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import { Modal } from "../ui";

/** "Resure Platform" → "RP", "resure" → "RES" */
function suggestKey(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const key = words.length > 1 ? words.map((w) => w[0]).join("") : (words[0] ?? "").slice(0, 3);
  return key.toUpperCase().replace(/[^A-Z0-9]/g, "").replace(/^[0-9]+/, "").slice(0, 10);
}

export default function NewProjectModal({ onClose, onCreated }: { onClose: () => void; onCreated: (key: string) => void }) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [key, setKey] = useState("");
  const [keyTouched, setKeyTouched] = useState(false);
  const [description, setDescription] = useState("");
  const effectiveKey = keyTouched ? key : suggestKey(name);
  const valid = /^[A-Z][A-Z0-9]{1,9}$/.test(effectiveKey) && name.trim().length > 0;

  const create = useMutation({
    mutationFn: () => api.createProject({ key: effectiveKey, name, description }),
    onSuccess: (p) => {
      qc.invalidateQueries({ queryKey: ["projects"] });
      onCreated(p.key);
    },
    meta: { quiet: true },
  });
  const submit = () => valid && !create.isPending && create.mutate();

  return (
    <Modal onClose={onClose}>
      <h2>New project</h2>
      <label className="field">
        Name
        <input autoFocus placeholder="e.g. Resure" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      </label>
      <label className="field">
        Key
        <input
          className="mono"
          placeholder="RES"
          value={effectiveKey}
          onChange={(e) => {
            setKeyTouched(true);
            setKey(e.target.value.toUpperCase());
          }}
        />
        <span className="faint" style={{ fontWeight: 400 }}>
          Prefix for ticket ids ({effectiveKey || "RES"}-1, {effectiveKey || "RES"}-2…). 2–10 letters or digits, can't be changed later.
        </span>
      </label>
      <label className="field">
        Description (optional)
        <textarea rows={2} value={description} onChange={(e) => setDescription(e.target.value)} />
      </label>
      {create.error && <div className="error">{create.error.message}</div>}
      <div className="row">
        <span className="spacer" />
        <button className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button className="primary" disabled={!valid || create.isPending} onClick={submit}>
          Create project
        </button>
      </div>
    </Modal>
  );
}
