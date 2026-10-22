// Types mirror src/model.rs; endpoints mirror src/server.rs.

export type Category = "backlog" | "unstarted" | "started" | "completed" | "canceled";
export type Priority = "none" | "urgent" | "high" | "medium" | "low";
export type ProposalStatus = "draft" | "review" | "accepted" | "rejected" | "withdrawn" | "superseded" | "implemented";
export type FieldKind = "text" | "number" | "select" | "date" | "url";

export interface Status { name: string; category: Category; color?: string }
export interface Named { name: string; color?: string }
export interface Milestone { name: string; description?: string; due?: string; done: boolean }
export interface FieldDef { name: string; kind: FieldKind; options?: string[] }

export interface Project {
  key: string;
  name: string;
  description: string;
  color?: string;
  archived: boolean;
  statuses: Status[];
  types: Named[];
  labels: Named[];
  milestones: Milestone[];
  fields: FieldDef[];
  next_ticket: number;
  next_proposal: number;
  created: string;
  updated: string;
}

export interface Counts { backlog: number; unstarted: number; started: number; completed: number; canceled: number; docs: number; proposals: number }
export type ProjectSummary = Project & { counts: Counts };

export interface Link { url: string; title?: string }
export interface Comment { id: number; author: string; at: string; body: string }

export interface Ticket {
  id: string;
  title: string;
  status: string;
  type: string;
  priority: Priority;
  labels?: string[];
  assignee?: string;
  estimate?: number;
  due?: string;
  milestone?: string;
  parent?: string;
  blocks?: string[];
  related?: string[];
  duplicate_of?: string;
  proposal?: string;
  branch?: string;
  links?: Link[];
  fields?: Record<string, unknown>;
  order: number;
  created: string;
  updated: string;
  closed?: string;
  comments?: Comment[];
  description: string;
  // Only on GET /tickets/:id
  blocked_by?: string[];
  children?: string[];
}

export interface Proposal {
  id: string;
  title: string;
  status: ProposalStatus;
  authors?: string[];
  labels?: string[];
  tickets?: string[];
  supersedes?: string;
  superseded_by?: string;
  decided?: string;
  created: string;
  updated: string;
  body: string;
}

export interface Doc { path: string; title: string; tags?: string[]; created: string; updated: string; body: string }

export interface SearchHit { kind: "ticket" | "doc" | "proposal"; project: string; id: string; title: string; snippet: string; status?: string }

export interface BackupConfig { enabled: boolean; interval_minutes: number; remote?: string | null; branch: string }
export interface Config { port: number; backup: BackupConfig }
export interface BackupStatus { last_run?: string; last_commit?: string; last_push?: string; last_error?: string }
export interface ServerStatus { version: string; home: string; data_dir: string; port: number; projects: number; backup: { config: BackupConfig; status: BackupStatus } }

export class ApiError extends Error {
  constructor(message: string, public status: number) {
    super(message);
  }
}

export interface ProjectPatch {
  name?: string;
  description?: string;
  color?: string;
  archived?: boolean;
  statuses?: Status[];
  types?: Named[];
  labels?: Named[];
  milestones?: Milestone[];
  fields?: FieldDef[];
  renames?: Record<string, Record<string, string>>;
  expected_updated?: string;
}
export type NewTicket = Pick<Ticket, "title"> & Partial<Pick<Ticket, "description" | "status" | "type" | "priority" | "labels" | "proposal">>;

async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(data.error ?? `HTTP ${res.status}`, res.status);
  return data as T;
}

export const enc = (s: string) => s.split("/").map(encodeURIComponent).join("/");

export const api = {
  status: () => req<ServerStatus>("GET", "/status"),
  config: () => req<Config>("GET", "/config"),
  saveConfig: ({ port, backup: b }: Config) =>
    req<Config>("PUT", "/config", { port, backup: { enabled: b.enabled, interval_minutes: b.interval_minutes, remote: b.remote ?? null, branch: b.branch } }),
  backup: () => req<BackupStatus>("POST", "/backup", {}),

  projects: (archived = false) => req<ProjectSummary[]>("GET", `/projects?archived=${archived}`),
  project: (key: string) => req<ProjectSummary>("GET", `/projects/${key}`),
  createProject: (p: { key: string; name: string; description?: string }) => req<Project>("POST", "/projects", p),
  updateProject: (key: string, patch: ProjectPatch) => req<Project>("PATCH", `/projects/${key}`, patch),
  deleteProject: (key: string) => req<unknown>("DELETE", `/projects/${key}`),

  tickets: (key: string) => req<Ticket[]>("GET", `/projects/${key}/tickets`),
  ticket: (id: string) => req<Ticket>("GET", `/tickets/${id}`),
  createTicket: (key: string, t: NewTicket) => req<Ticket>("POST", `/projects/${key}/tickets`, t),
  updateTicket: (id: string, t: Record<string, unknown>) => req<Ticket>("PATCH", `/tickets/${id}`, t),
  moveTicket: (id: string, status: string, index: number) => req<Ticket>("POST", `/tickets/${id}/move`, { status, index }),
  deleteTicket: (id: string) => req<unknown>("DELETE", `/tickets/${id}`),
  comment: (id: string, body: string) => req<Ticket>("POST", `/tickets/${id}/comments`, { author: "me", body }),
  deleteComment: (id: string, cid: number) => req<Ticket>("DELETE", `/tickets/${id}/comments/${cid}`),

  proposals: (key: string) => req<Proposal[]>("GET", `/projects/${key}/proposals`),
  proposal: (id: string) => req<Proposal>("GET", `/proposals/${id}`),
  createProposal: (key: string, p: { title: string; authors?: string[] }) => req<Proposal>("POST", `/projects/${key}/proposals`, p),
  updateProposal: (id: string, p: Record<string, unknown>) => req<Proposal>("PATCH", `/proposals/${id}`, p),
  deleteProposal: (id: string) => req<unknown>("DELETE", `/proposals/${id}`),

  docs: (key: string) => req<Doc[]>("GET", `/projects/${key}/docs`),
  doc: (key: string, path: string) => req<Doc>("GET", `/projects/${key}/docs/${enc(path)}`),
  createDoc: (key: string, d: { title: string; path?: string; body?: string; tags?: string[] }) => req<Doc>("POST", `/projects/${key}/docs`, d),
  saveDoc: (key: string, path: string, d: { title?: string; body?: string; tags?: string[]; new_path?: string }) => req<Doc>("PUT", `/projects/${key}/docs/${enc(path)}`, d),
  deleteDoc: (key: string, path: string) => req<unknown>("DELETE", `/projects/${key}/docs/${enc(path)}`),

  search: (q: string, project?: string) => req<SearchHit[]>("GET", `/search?q=${encodeURIComponent(q)}${project ? `&project=${project}` : ""}`),
};

export const PRIORITIES: Priority[] = ["urgent", "high", "medium", "low", "none"];
export const CATEGORIES: Category[] = ["backlog", "unstarted", "started", "completed", "canceled"];
export const PROPOSAL_STATUSES: ProposalStatus[] = ["draft", "review", "accepted", "rejected", "withdrawn", "superseded", "implemented"];

export const isClosed = (c?: Category) => c === "completed" || c === "canceled";

/** Only http(s) and mailto URLs may become links; anything else (javascript:, data:, relative…) returns undefined. */
export function safeUrl(url: unknown): string | undefined {
  if (typeof url !== "string") return undefined;
  try {
    return ["http:", "https:", "mailto:"].includes(new URL(url.trim()).protocol) ? url.trim() : undefined;
  } catch {
    return undefined;
  }
}
