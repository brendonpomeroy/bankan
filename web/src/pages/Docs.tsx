import { useEffect, useState } from "react";
import { NavLink, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, Doc, enc } from "../api";
import { Chip, ConflictNotice, Markdown, Modal, pressable, TagInput, timeAgo, toast } from "../ui";
import { useProject } from "./ProjectLayout";
import * as I from "../icons";

function slugify(s: string) {
  return s.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-+|-+$/g, "") || "untitled";
}

function NewDocModal({ projectKey, folders, onClose }: { projectKey: string; folders: string[]; onClose: () => void }) {
  const [title, setTitle] = useState("");
  const [folder, setFolder] = useState("");
  const navigate = useNavigate();
  const qc = useQueryClient();
  const create = useMutation({
    mutationFn: () => api.createDoc(projectKey, { title, path: [folder.trim().replace(/^\/+|\/+$/g, ""), slugify(title)].filter(Boolean).join("/"), body: `# ${title}\n\n` }),
    onSuccess: (d) => {
      qc.invalidateQueries({ queryKey: ["docs", projectKey] });
      onClose();
      navigate(`/p/${projectKey}/docs/${enc(d.path)}?edit=1`);
    },
    meta: { quiet: true },
  });
  const submit = () => title.trim() && !create.isPending && create.mutate();
  return (
    <Modal onClose={onClose}>
      <h2>New doc</h2>
      <label className="field">
        Title
        <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      </label>
      <label className="field">
        Folder (optional)
        <input list="doc-folders" placeholder="e.g. guides or architecture/storage" value={folder} onChange={(e) => setFolder(e.target.value)} />
        <datalist id="doc-folders">
          {folders.map((f) => (
            <option key={f} value={f} />
          ))}
        </datalist>
      </label>
      {title && (
        <span className="faint small mono">
          {[folder.trim(), slugify(title)].filter(Boolean).join("/")}.md
        </span>
      )}
      {create.error && <div className="error">{create.error.message}</div>}
      <div className="row">
        <span className="spacer" />
        <button className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button className="primary" disabled={!title.trim() || create.isPending} onClick={submit}>
          Create
        </button>
      </div>
    </Modal>
  );
}

function DocEditor({ projectKey, doc, onDone }: { projectKey: string; doc: Doc; onDone: (path?: string) => void }) {
  const qc = useQueryClient();
  const [title, setTitle] = useState(doc.title);
  const [path, setPath] = useState(doc.path);
  const [tags, setTags] = useState(doc.tags ?? []);
  const [body, setBody] = useState(doc.body);
  // The server version editing started from; a newer one arriving means someone else saved meanwhile.
  const [base, setBase] = useState(doc.updated);
  const save = useMutation({
    mutationFn: () => api.saveDoc(projectKey, doc.path, { title, body, tags, new_path: path !== doc.path ? path : undefined }),
    onSuccess: (d) => {
      qc.invalidateQueries({ queryKey: ["docs", projectKey] });
      qc.invalidateQueries({ queryKey: ["doc", projectKey] });
      toast("Saved");
      onDone(d.path);
    },
    meta: { quiet: true },
  });
  const submit = () => !save.isPending && save.mutate();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        submit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  return (
    <div className="col" style={{ gap: 12 }}>
      <div className="row">
        <input className="title-input grow" value={title} onChange={(e) => setTitle(e.target.value)} />
        <button className="ghost" onClick={() => onDone()}>
          Cancel
        </button>
        <button className="primary" onClick={submit} disabled={save.isPending}>
          Save <span className="kbd" style={{ background: "transparent", color: "inherit", borderColor: "rgba(255,255,255,.4)" }}>⌘S</span>
        </button>
      </div>
      <div className="row" style={{ alignItems: "flex-start" }}>
        <label className="field" style={{ width: 320 }}>
          Path
          <input className="mono" value={path} onChange={(e) => setPath(e.target.value)} />
        </label>
        <label className="field grow">
          Tags
          <TagInput values={tags} onChange={setTags} placeholder="Add tag…" />
        </label>
      </div>
      {save.error && <div className="error">{save.error.message}</div>}
      {doc.updated !== base && (
        <ConflictNotice
          onKeep={() => setBase(doc.updated)}
          onDiscard={() => {
            setTitle(doc.title);
            setPath(doc.path);
            setTags(doc.tags ?? []);
            setBody(doc.body);
            setBase(doc.updated);
          }}
        />
      )}
      <div className="editor-split">
        <textarea className="code" value={body} onChange={(e) => setBody(e.target.value)} autoFocus />
        <div className="preview">
          <Markdown text={body} />
        </div>
      </div>
    </div>
  );
}

export default function Docs() {
  const { project } = useProject();
  const path = useParams()["*"] ?? "";
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [params] = useSearchParams();
  const [editing, setEditing] = useState(() => params.has("edit"));
  const docs = useQuery({ queryKey: ["docs", project.key], queryFn: () => api.docs(project.key) });
  const doc = useQuery({ queryKey: ["doc", project.key, path], queryFn: () => api.doc(project.key, path), enabled: !!path });
  const del = useMutation({
    mutationFn: () => api.deleteDoc(project.key, path),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["docs", project.key] });
      navigate(`/p/${project.key}/docs`);
    },
  });

  const editParam = params.has("edit");
  useEffect(() => setEditing(editParam), [path, editParam]);

  const list = docs.data ?? [];
  const folders = [...new Set(list.map((d) => d.path.split("/").slice(0, -1).join("/")))].sort();

  return (
    <div className="docs-layout">
      <nav className="docs-tree">
        <button className="small" style={{ width: "100%", marginBottom: 6 }} onClick={() => setCreating(true)}>
          <I.Plus size={13} /> New doc
        </button>
        {list.length === 0 && <div className="faint small" style={{ padding: 8 }}>No docs yet.</div>}
        {folders.map((folder) => (
          <div key={folder || "root"}>
            {folder && <div className="folder">{folder.split("/").join(" / ")}</div>}
            {list
              .filter((d) => d.path.split("/").slice(0, -1).join("/") === folder)
              .sort((a, b) => a.title.localeCompare(b.title))
              .map((d) => (
                <NavLink key={d.path} to={`/p/${project.key}/docs/${enc(d.path)}`} className={({ isActive }) => `nav-item${isActive ? " active" : ""}`}>
                  <I.Doc size={14} />
                  <span className="ellipsis">{d.title}</span>
                </NavLink>
              ))}
          </div>
        ))}
      </nav>
      <div className="docs-content">
        {!path && (
          <div className="empty-state">
            <div className="icon-wrap">
              <I.Doc size={20} />
            </div>
            <h1>Docs</h1>
            <p className="muted">
              Long-lived reference material for {project.name}: architecture notes, guides, decisions-in-practice. Docs are Markdown files under{" "}
              <code>
                ~/.bankan/data/projects/{project.key}/docs/
              </code>
              , and agents can read and write them over MCP.
            </p>
            <div>
              <button className="primary" onClick={() => setCreating(true)}>
                <I.Plus size={14} /> New doc
              </button>
            </div>
          </div>
        )}
        {path && doc.error && <div className="error">{doc.error.message}</div>}
        {path && doc.data && !editing && (
          <article>
            <div className="row" style={{ marginBottom: 6, maxWidth: 800 }}>
              <span className="faint small mono">{doc.data.path}</span>
              <span className="spacer" />
              <span className="faint small">Updated {timeAgo(doc.data.updated)}</span>
              <button className="small" onClick={() => setEditing(true)}>
                <I.Pencil size={13} /> Edit
              </button>
              <button className="ghost small icon" title="Delete doc" onClick={() => confirm(`Delete "${doc.data!.title}"?`) && del.mutate()}>
                <I.Trash size={14} />
              </button>
            </div>
            {!!doc.data.tags?.length && (
              <div className="row" style={{ gap: 4, marginBottom: 12 }}>
                {doc.data.tags.map((t) => (
                  <Chip key={t} label={t} />
                ))}
              </div>
            )}
            {doc.data.body.trim().startsWith("# ") ? null : <h1 style={{ marginBottom: 16 }}>{doc.data.title}</h1>}
            {doc.data.body.trim() ? <Markdown text={doc.data.body} /> : <div className="empty-md" {...pressable(() => setEditing(true))}>Empty doc. Click to write.</div>}
          </article>
        )}
        {path && doc.data && editing && (
          <DocEditor
            key={doc.data.path}
            projectKey={project.key}
            doc={doc.data}
            onDone={(newPath) => {
              setEditing(false);
              navigate(`/p/${project.key}/docs/${enc(newPath ?? path)}`, { replace: true });
            }}
          />
        )}
      </div>
      {creating && <NewDocModal projectKey={project.key} folders={folders.filter(Boolean)} onClose={() => setCreating(false)} />}
    </div>
  );
}
