import { KeyboardEvent as ReactKeyboardEvent, ReactNode, useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Category, Priority, PRIORITIES } from "./api";
import * as I from "./icons";

export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{ a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noreferrer" /> }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

/** Click + Enter/Space handlers and focusability for non-button elements that act as buttons. */
export function pressable(fn: () => void) {
  return {
    role: "button",
    tabIndex: 0,
    onClick: fn,
    onKeyDown: (e: ReactKeyboardEvent<HTMLElement>) => {
      if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        fn();
      }
    },
  };
}

/** Shown in an editor when the server copy changed after editing started. */
export function ConflictNotice({ onKeep, onDiscard }: { onKeep: () => void; onDiscard: () => void }) {
  return (
    <div className="notice" role="alert">
      <span className="grow">This was changed elsewhere since you started editing. Saving will overwrite those changes.</span>
      <button className="small" onClick={onKeep}>
        Keep editing
      </button>
      <button className="small" onClick={onDiscard}>
        Discard mine, load latest
      </button>
    </div>
  );
}

/** Rendered Markdown that turns into a textarea on click; saves on ⌘↵ or the Save button. */
export function MarkdownField({ value, onSave, placeholder = "Add a description…", minRows = 8 }: { value: string; onSave: (v: string) => void; placeholder?: string; minRows?: number }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  // The server text the draft was based on; if `value` moves away from it, someone else edited meanwhile.
  const [base, setBase] = useState(value);
  useEffect(() => {
    if (!editing) {
      setDraft(value);
      setBase(value);
    }
  }, [value, editing]);
  const edit = () => setEditing(true);
  if (!editing) {
    return (
      <div className="md-field" onDoubleClick={edit}>
        {value.trim() ? <Markdown text={value} /> : <div className="empty-md" {...pressable(edit)}>{placeholder}</div>}
        {value.trim() && (
          <button className="ghost small edit-btn" onClick={edit} title="Edit (or double-click)">
            <I.Pencil size={13} /> Edit
          </button>
        )}
      </div>
    );
  }
  const save = () => {
    onSave(draft);
    setEditing(false);
  };
  const cancel = () => {
    setDraft(value);
    setEditing(false);
  };
  return (
    <div className="col">
      {value !== base && (
        <ConflictNotice
          onKeep={() => setBase(value)}
          onDiscard={() => {
            setDraft(value);
            setBase(value);
          }}
        />
      )}
      <textarea
        className="code"
        autoFocus
        rows={Math.max(minRows, draft.split("\n").length + 1)}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save();
          if (e.key === "Escape") cancel();
        }}
      />
      <div className="row">
        <button className="primary small" onClick={save}>
          Save
        </button>
        <button className="ghost small" onClick={cancel}>
          Cancel
        </button>
        <span className="faint small">Markdown · ⌘↵ to save · Esc to cancel</span>
      </div>
    </div>
  );
}

/** Text input that commits on blur or Enter. */
export function InlineInput({ value, onCommit, placeholder, className, type = "text" }: { value: string; onCommit: (v: string) => void; placeholder?: string; className?: string; type?: string }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <input
      type={type}
      className={className}
      value={v}
      placeholder={placeholder}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => v !== value && onCommit(v)}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") {
          setV(value);
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

export function Modal({ children, onClose, wide }: { children: ReactNode; onClose: () => void; wide?: boolean }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (el && !el.contains(document.activeElement)) (el.querySelector<HTMLElement>("input, textarea, select, button") ?? el).focus();
  }, []);
  return (
    <div className="modal-wrap" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} className={`modal${wide ? " wide" : ""}`} role="dialog" aria-modal="true" tabIndex={-1}>
        {children}
      </div>
    </div>
  );
}

export function PrioritySelect({ value, onChange, className, emptyLabel }: { value: Priority | ""; onChange: (p: Priority) => void; className?: string; emptyLabel?: string }) {
  return (
    <select className={className} value={value} onChange={(e) => onChange(e.target.value as Priority)}>
      {emptyLabel && <option value="">{emptyLabel}</option>}
      {PRIORITIES.map((p) => (
        <option key={p} value={p}>
          {p === "none" ? "No priority" : p[0].toUpperCase() + p.slice(1)}
        </option>
      ))}
    </select>
  );
}

/** A ticket id that opens the ticket panel (and works as a normal link for middle-click). */
export function TicketRefLink({ id, onOpen }: { id: string; onOpen: (id: string) => void }) {
  return (
    <a
      href={`/t/${id}`}
      className="mono"
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        onOpen(id);
      }}
    >
      {id}
    </a>
  );
}

export function StatusDot({ category, color }: { category?: Category; color?: string }) {
  return <span className={`status-dot ${category ?? ""}`} style={{ color: color ?? "var(--text-3)" }} />;
}

export function PriorityIcon({ priority }: { priority: Priority }) {
  if (priority === "urgent") return <span className="prio urgent" title="Urgent">!</span>;
  const level = { high: 3, medium: 2, low: 1, none: 0 }[priority];
  if (level === 0) return null;
  return (
    <span className="prio" title={priority[0].toUpperCase() + priority.slice(1)}>
      {[1, 2, 3].map((i) => (
        <i key={i} className={i <= level ? "on" : ""} style={{ height: 4 + i * 3 }} />
      ))}
    </span>
  );
}

const AVATAR_COLORS = ["#5b6cf9", "#e5534b", "#3fb67d", "#f2a93b", "#a46ef2", "#2ba3c6", "#d1609e", "#b08d57"];
export function Avatar({ name }: { name: string }) {
  const hash = [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  return (
    <span className="avatar" title={name} style={{ background: AVATAR_COLORS[hash % AVATAR_COLORS.length] }}>
      {name.slice(0, 2).toUpperCase()}
    </span>
  );
}

export function Chip({ label, color, onRemove }: { label: string; color?: string; onRemove?: () => void }) {
  return (
    <span className="chip">
      {color && <span className="swatch" style={{ background: color }} />}
      {label}
      {onRemove && (
        <button onClick={onRemove} aria-label={`Remove ${label}`}>
          <I.X size={11} />
        </button>
      )}
    </span>
  );
}

/** Editable list of strings shown as chips, with suggestions. */
export function TagInput({ values, onChange, suggestions = [], placeholder = "Add…", colorOf }: { values: string[]; onChange: (v: string[]) => void; suggestions?: string[]; placeholder?: string; colorOf?: (v: string) => string | undefined }) {
  const [draft, setDraft] = useState("");
  const id = useRef(`dl-${Math.random().toString(36).slice(2)}`).current;
  const add = (v: string) => {
    const t = v.trim();
    if (t && !values.some((x) => x.toLowerCase() === t.toLowerCase())) onChange([...values, t]);
    setDraft("");
  };
  return (
    <div className="prop-stack" style={{ gap: 6 }}>
      {values.length > 0 && (
        <div className="chips" style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
          {values.map((v) => (
            <Chip key={v} label={v} color={colorOf?.(v)} onRemove={() => onChange(values.filter((x) => x !== v))} />
          ))}
        </div>
      )}
      <input
        list={id}
        value={draft}
        placeholder={placeholder}
        onChange={(e) => {
          const v = e.target.value;
          // Picking from the datalist fires a change with the full value.
          if (suggestions.includes(v)) add(v);
          else setDraft(v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            add(draft);
          }
          if (e.key === "Backspace" && !draft && values.length) onChange(values.slice(0, -1));
        }}
        onBlur={() => draft && add(draft)}
      />
      <datalist id={id}>
        {suggestions.filter((s) => !values.includes(s)).map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
    </div>
  );
}

export function timeAgo(iso?: string): string {
  if (!iso) return "";
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d ago`;
  return new Date(iso).toLocaleDateString();
}

export function fmtDate(d?: string) {
  if (!d) return "";
  const date = new Date(d.length === 10 ? `${d}T00:00:00` : d);
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: date.getFullYear() === new Date().getFullYear() ? undefined : "numeric" });
}

export function isOverdue(due?: string) {
  return !!due && new Date(`${due}T23:59:59`) < new Date();
}

let toastTimer: number | undefined;
export function toast(message: string) {
  document.querySelector(".toast")?.remove();
  const el = document.createElement("div");
  el.className = "toast";
  el.textContent = message;
  document.body.appendChild(el);
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.remove(), 3200);
}

function readLocal<T>(key: string, initial: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : initial;
  } catch {
    return initial;
  }
}

/** useState persisted to localStorage; re-reads when `key` changes (e.g. switching project). */
export function useLocalState<T>(key: string, initial: T): [T, (v: T) => void] {
  const [state, setState] = useState(() => ({ key, v: readLocal(key, initial) }));
  let v = state.v;
  if (state.key !== key) {
    v = readLocal(key, initial);
    setState({ key, v });
  }
  const set = (next: T) => {
    setState({ key, v: next });
    try {
      localStorage.setItem(key, JSON.stringify(next));
    } catch {
      /* storage unavailable: keep in memory only */
    }
  };
  return [v, set];
}
