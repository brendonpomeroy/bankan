// Pure helpers behind the Markdown renderer: GitHub-style heading slugs, repo-style relative link
// resolution to BanKan docs, and auto-linking of ticket/proposal keys. No React here so they can be
// checked with plain node.

/** A minimal slice of the hast tree that rehype plugins see. */
type HNode = { type: string; tagName?: string; value?: string; properties?: Record<string, unknown>; children?: HNode[] };

/** GitHub's heading slug: lowercase, drop punctuation except `-`, `_` and spaces, spaces → `-`. */
export function githubSlug(text: string) {
  return text.toLowerCase().replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "").replace(/ /g, "-");
}

const textOf = (n: HNode): string => (n.type === "text" ? n.value ?? "" : (n.children ?? []).map(textOf).join(""));
const HEADING = /^h[1-6]$/;

/** Rehype plugin: gives every heading a GitHub-style `id`, numbering repeats (`x`, `x-1`, …) like GitHub. */
export function rehypeHeadingIds() {
  return (tree: HNode) => {
    const seen = new Map<string, number>();
    const walk = (n: HNode) => {
      if (n.type === "element" && HEADING.test(n.tagName ?? "")) {
        // Same numbering as github-slugger.
        const base = githubSlug(textOf(n).trim());
        let id = base;
        while (seen.has(id)) {
          seen.set(base, seen.get(base)! + 1);
          id = `${base}-${seen.get(base)}`;
        }
        seen.set(id, 0);
        n.properties = { ...n.properties, id };
        return;
      }
      n.children?.forEach(walk);
    };
    walk(tree);
  };
}

/** App route for a ticket (`HH-12`) or proposal (`HH-P17`) key. */
export const keyHref = (key: string, id: string) => (/-P\d+$/.test(id) ? `/p/${key}/proposals/${id}` : `/t/${id}`);

/** Rehype plugin: turns `KEY-12` / `KEY-P3` in text into links, for the given project keys; skips code and existing links. */
export function rehypeKeyLinks(keys: string[]) {
  const re = keys.length ? new RegExp(`\\b(${keys.map((k) => k.replace(/[^A-Za-z0-9]/g, "\\$&")).join("|")})-P?\\d+\\b`, "g") : null;
  return () => (tree: HNode) => {
    if (!re) return;
    const walk = (n: HNode) => {
      if (!n.children || (n.type === "element" && ["a", "code", "pre"].includes(n.tagName ?? ""))) return;
      n.children = n.children.flatMap((c): HNode[] => {
        if (c.type !== "text") return walk(c), [c];
        const out: HNode[] = [];
        const s = c.value ?? "";
        let last = 0;
        for (const m of s.matchAll(re)) {
          if (m.index! > last) out.push({ type: "text", value: s.slice(last, m.index) });
          out.push({ type: "element", tagName: "a", properties: { href: keyHref(m[1], m[0]), className: ["key-link"] }, children: [{ type: "text", value: m[0] }] });
          last = m.index! + m[0].length;
        }
        if (!out.length) return [c];
        if (last < s.length) out.push({ type: "text", value: s.slice(last) });
        return out;
      });
    };
    walk(tree);
  };
}

export type LinkTarget =
  | { kind: "external" } // has a scheme or is protocol-relative: open in a new tab
  | { kind: "anchor" } // `#x` within the page: leave to the browser
  | { kind: "app"; to: string } // in-app route
  | { kind: "missing"; path: string }; // a relative link that matches no doc

const decode = (s: string) => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

/**
 * Classifies a Markdown link. Relative links (no scheme, not `#…`) are doc paths: resolved against the
 * folder of `docPath` (or the docs root), `.md` stripped, `#anchor` kept. `docs` maps lowercase doc
 * path → stored path; when it's unknown (still loading) the link is trusted. Without a project key,
 * relative links are left alone (treated as external).
 */
export function resolveLink(href: string, opts: { projectKey?: string; docPath?: string; docs?: Map<string, string> }): LinkTarget {
  if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) return { kind: "external" };
  if (href.startsWith("#") || href === "") return { kind: "anchor" };
  if (/^\/[tp]\//.test(href)) return { kind: "app", to: href };
  const { projectKey: key, docPath = "", docs } = opts;
  if (!key) return { kind: "external" };
  const [, rawPath, hash = ""] = href.match(/^([^?#]*)(?:\?[^#]*)?(#.*)?$/) ?? ["", href];
  const path = decode(rawPath);
  if (!path) return { kind: "anchor" };
  const parts = path.startsWith("/") ? [] : docPath.split("/").slice(0, -1);
  // `..` past the root clamps there: imports flatten repo siblings (`../reports/x.md`) into the docs tree.
  for (const seg of path.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg !== "" && seg !== ".") parts.push(seg);
  }
  const target = parts.join("/").replace(/\.md$/i, "").toLowerCase();
  // A folder link (`api/`) lands on that folder's readme or index.
  const candidates = [target, `${target}/readme`, `${target}/index`].map((c) => c.replace(/^\//, "")).filter(Boolean);
  // Docs imported from a repo's docs/ folder may link back in through it (`../docs/07-x.md`).
  if (target.startsWith("docs/")) candidates.push(target.slice(5));
  // Imported file names with spaces are stored slugified (`Market positioning.md` → `market-positioning`).
  if (/\s/.test(target)) candidates.push(target.replace(/\s+/g, "-"));
  const found = docs ? candidates.map((c) => docs.get(c)).find(Boolean) : target;
  if (!found) return { kind: "missing", path };
  return { kind: "app", to: `/p/${key}/docs/${found.split("/").map(encodeURIComponent).join("/")}${hash}` };
}

/** Cheap check for links that may need the doc list to resolve. */
export const hasRelativeLink = (text: string) => /\]\((?![a-z][a-z0-9+.-]*:|#|\/\/)[^)\s]|^\s*\[[^\]]+\]:\s*(?![a-z][a-z0-9+.-]*:|#)/im.test(text);
