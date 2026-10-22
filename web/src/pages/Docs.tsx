import { useEffect, useMemo, useRef, useState } from "react";
import { NavLink, useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, Doc, enc } from "../api";
import { Chip, ConflictNotice, Markdown, Modal, pressable, TagInput, timeAgo, toast, useLocalState } from "../ui";
import { useProject } from "./ProjectLayout";
import * as I from "../icons";

function slugify(s: string) {
  return s.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-+|-+$/g, "") || "untitled";
}

interface Folder { path: string; name: string; docs: Doc[]; folders: Folder[] }

// Numeric-aware path order keeps a numbered series (00-…, 01-…, 10-…) in reading order; a folder's readme/index leads.
const byPath = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
const isIndex = (d: Doc) => /(^|\/)(readme|index)$/i.test(d.path);

function buildTree(list: Doc[]): Folder {
  const root: Folder = { path: "", name: "", docs: [], folders: [] };
  for (const d of list) {
    let f = root;
    for (const name of d.path.split("/").slice(0, -1)) {
      const path = f.path ? `${f.path}/${name}` : name;
      let next = f.folders.find((x) => x.path === path);
      if (!next) f.folders.push((next = { path, name, docs: [], folders: [] }));
      f = next;
    }
    f.docs.push(d);
  }
  const sort = (f: Folder) => {
    f.docs.sort((a, b) => Number(isIndex(b)) - Number(isIndex(a)) || byPath(a.path, b.path));
    f.folders.sort((a, b) => byPath(a.name, b.name)).forEach(sort);
  };
  sort(root);
  return root;
}

/** One folder's docs then its subfolders, which collapse. */
function TreeLevel({ folder, depth, projectKey, open, toggle }: { folder: Folder; depth: number; projectKey: string; open: string[]; toggle: (path: string) => void }) {
  const pad = { paddingLeft: 8 + depth * 14 };
  return (
    <>
      {folder.docs.map((d) => (
        <NavLink key={d.path} to={`/p/${projectKey}/docs/${enc(d.path)}`} className={({ isActive }) => `nav-item${isActive ? " active" : ""}`} style={pad} title={d.path}>
          <I.Doc size={14} />
          <span className="ellipsis">{d.title}</span>
        </NavLink>
      ))}
      {folder.folders.map((f) => {
        const isOpen = open.includes(f.path);
        return (
          <div key={f.path}>
            <button className="nav-item folder" style={pad} aria-expanded={isOpen} onClick={() => toggle(f.path)}>
              <I.ChevronDown size={13} className={`chev${isOpen ? "" : " closed"}`} />
              <span className="ellipsis">{f.name}</span>
            </button>
            {isOpen && <TreeLevel folder={f} depth={depth + 1} projectKey={projectKey} open={open} toggle={toggle} />}
          </div>
        );
      })}
    </>
  );
}

interface Heading { id: string; text: string; level: number }

/** Sticky "On this page" list of the article's h2/h3s, highlighting the one scrolled to. */
function Outline({ headings, scroller }: { headings: Heading[]; scroller: HTMLElement | null }) {
  const location = useLocation();
  const navigate = useNavigate();
  const [active, setActive] = useState("");
  useEffect(() => {
    if (!scroller) return;
    const onScroll = () => {
      const top = scroller.getBoundingClientRect().top + 90;
      let current = headings[0]?.id ?? "";
      for (const h of headings) {
        const el = document.getElementById(h.id);
        if (el && el.getBoundingClientRect().top <= top) current = h.id;
      }
      setActive(current);
    };
    onScroll();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  }, [headings, scroller]);
  return (
    <aside className="doc-outline">
      <div className="doc-outline-title">On this page</div>
      {headings.map((h) => (
        <a
          key={h.id}
          href={`#${h.id}`}
          className={`${h.level === 3 ? "sub" : ""}${h.id === active ? " active" : ""}`}
          onClick={(e) => {
            e.preventDefault();
            document.getElementById(h.id)?.scrollIntoView();
            navigate({ search: location.search, hash: h.id }, { replace: true });
          }}
        >
          {h.text}
        </a>
      ))}
    </aside>
  );
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
          <Markdown text={body} projectKey={projectKey} docPath={path} newTab />
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
  const tree = useMemo(() => buildTree(docs.data ?? []), [docs.data]);

  // Open folders persist per project; the current doc's folders open automatically.
  const [open, setOpen] = useLocalState<string[]>(`bankan.docs.open.${project.key}`, []);
  const toggle = (f: string) => setOpen(open.includes(f) ? open.filter((x) => x !== f) : [...open, f]);
  useEffect(() => {
    const parts = path.split("/").slice(0, -1);
    const ancestors = parts.map((_, i) => parts.slice(0, i + 1).join("/"));
    if (ancestors.some((a) => !open.includes(a))) setOpen([...new Set([...open, ...ancestors])]);
  }, [path]);

  // Headings for the outline, read from the rendered article so ids match exactly.
  const location = useLocation();
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null);
  const articleRef = useRef<HTMLElement>(null);
  const [headings, setHeadings] = useState<Heading[]>([]);
  const body = !editing ? doc.data?.body : undefined;
  useEffect(() => {
    const els = articleRef.current?.querySelectorAll<HTMLElement>(".md h2[id], .md h3[id]") ?? [];
    const text = (el: HTMLElement) => [...el.childNodes].filter((n) => !(n instanceof HTMLElement && n.classList.contains("heading-anchor"))).map((n) => n.textContent).join("").trim();
    setHeadings([...els].map((el) => ({ id: el.id, level: el.tagName === "H3" ? 3 : 2, text: text(el) })));
  }, [body]);
  // A new doc starts at the top; the URL's #section is scrolled to once it renders and whenever the hash changes.
  const loaded = body !== undefined;
  useEffect(() => scroller?.scrollTo(0, 0), [path, scroller]);
  useEffect(() => {
    const id = decodeURIComponent(location.hash.slice(1));
    if (id && loaded) document.getElementById(id)?.scrollIntoView();
  }, [location.hash, path, loaded]);

  return (
    <div className="docs-layout">
      <nav className="docs-tree">
        <button className="small" style={{ width: "100%", marginBottom: 6 }} onClick={() => setCreating(true)}>
          <I.Plus size={13} /> New doc
        </button>
        {list.length === 0 && <div className="faint small" style={{ padding: 8 }}>No docs yet.</div>}
        <TreeLevel folder={tree} depth={0} projectKey={project.key} open={open} toggle={toggle} />
      </nav>
      <div className="docs-content" ref={setScroller}>
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
          <div className="doc-read">
            <article ref={articleRef}>
              <div className="row" style={{ marginBottom: 6 }}>
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
              {/^(\s*>.*\n|\s*\n)*# /.test(doc.data.body) ? null : <h1 style={{ marginBottom: 16 }}>{doc.data.title}</h1>}
              {doc.data.body.trim() ? <Markdown text={doc.data.body} projectKey={project.key} docPath={doc.data.path} headingLinks /> : <div className="empty-md" {...pressable(() => setEditing(true))}>Empty doc. Click to write.</div>}
            </article>
            {headings.length >= 3 && <Outline headings={headings} scroller={scroller} />}
          </div>
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
