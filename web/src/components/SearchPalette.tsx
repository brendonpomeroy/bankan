import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { api, enc, SearchHit } from "../api";
import * as I from "../icons";

type Item = { key: string; label: string; sub?: string; kind: string; go: () => void };

export default function SearchPalette({ onClose }: { onClose: () => void }) {
  const [q, setQ] = useState("");
  const [debounced, setDebounced] = useState("");
  const [active, setActive] = useState(0);
  const navigate = useNavigate();
  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api.projects() });
  const hits = useQuery({ queryKey: ["search", debounced], queryFn: () => api.search(debounced), enabled: debounced.trim().length > 0 });

  useEffect(() => {
    const t = setTimeout(() => setDebounced(q), 120);
    return () => clearTimeout(t);
  }, [q]);

  const go = (path: string) => {
    onClose();
    navigate(path);
  };
  const hitPath = (h: SearchHit) =>
    h.kind === "ticket" ? `/p/${h.project}/board?ticket=${h.id}` : h.kind === "proposal" ? `/p/${h.project}/proposals/${h.id}` : `/p/${h.project}/docs/${enc(h.id)}`;

  const lower = q.trim().toLowerCase();
  const items: Item[] = [
    ...(projects.data ?? [])
      .filter((p) => !lower || p.key.toLowerCase().includes(lower) || p.name.toLowerCase().includes(lower))
      .slice(0, lower ? 5 : 8)
      .map((p) => ({ key: `p:${p.key}`, label: p.name, sub: p.key, kind: "project", go: () => go(`/p/${p.key}/board`) })),
    ...(lower ? (hits.data ?? []) : []).map((h) => ({ key: `${h.kind}:${h.project}:${h.id}`, label: h.title, sub: `${h.kind === "doc" ? h.project + " · " + h.id : h.id}${h.status ? " · " + h.status : ""}${h.snippet ? " — " + h.snippet : ""}`, kind: h.kind, go: () => go(hitPath(h)) })),
  ];

  // Items change with both the raw query (projects) and the debounced one (hits): reset on either, and clamp.
  useEffect(() => setActive(0), [q, debounced]);
  const sel = Math.max(0, Math.min(active, items.length - 1));

  return (
    <div className="modal-wrap" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Search">
        <div className="palette-input">
        <I.Search size={17} />
        <input
          autoFocus
          placeholder="Search tickets, docs, proposals, projects…"
          aria-label="Search"
          role="combobox"
          aria-expanded
          aria-controls="palette-list"
          aria-activedescendant={items[sel] ? `palette-${sel}` : undefined}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive(Math.min(sel + 1, items.length - 1));
            }
            if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive(Math.max(sel - 1, 0));
            }
            if (e.key === "Enter") items[sel]?.go();
          }}
        />
        <span className="kbd">esc</span>
        </div>
        <div className="palette-list" id="palette-list" role="listbox">
          {items.map((it, i) => (
            <div key={it.key} id={`palette-${i}`} role="option" aria-selected={i === sel} className={`palette-item${i === sel ? " active" : ""}`} onMouseEnter={() => setActive(i)} onClick={it.go}>
              {it.kind === "project" ? <I.Board /> : it.kind === "doc" ? <I.Doc /> : it.kind === "proposal" ? <I.Proposal /> : <I.List />}
              <div className="col grow" style={{ gap: 0, minWidth: 0 }}>
                <span>{it.label}</span>
                {it.sub && <span className="snippet">{it.sub}</span>}
              </div>
            </div>
          ))}
          {lower && hits.data?.length === 0 && items.length === 0 && <div className="faint" style={{ padding: 12 }}>No results for “{q}”.</div>}
        </div>
        <div className="palette-foot">
          <span>↑↓ to navigate</span>
          <span>↵ to open</span>
          <span className="spacer" />
          <span>{items.length} results</span>
        </div>
      </div>
    </div>
  );
}
