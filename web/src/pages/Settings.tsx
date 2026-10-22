import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, Config } from "../api";
import { timeAgo, toast } from "../ui";

export default function Settings() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["status"], queryFn: api.status, refetchInterval: 15_000 });
  const config = useQuery({ queryKey: ["config"], queryFn: api.config });
  const archived = useQuery({ queryKey: ["projects", "archived"], queryFn: () => api.projects(true) });
  const [cfg, setCfg] = useState<Config | null>(null);
  useEffect(() => {
    if (config.data && !cfg) setCfg(config.data);
  }, [config.data, cfg]);

  const save = useMutation({
    mutationFn: (c: Config) => api.saveConfig(c),
    onSuccess: (c) => {
      setCfg(c);
      qc.invalidateQueries({ queryKey: ["status"] });
      toast(c.port !== status.data?.port ? "Saved. Restart BanKan for the new port to apply." : "Saved");
    },
  });
  const backup = useMutation({
    mutationFn: api.backup,
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: ["status"] });
      toast(s.last_error ? `Backup failed: ${s.last_error}` : "Backup done");
    },
  });

  const s = status.data;
  const b = s?.backup.status;

  return (
    <div className="page narrow col" style={{ gap: 16 }}>
      <h1>Settings</h1>

      <section className="section">
        <h2>Backup</h2>
        <p className="muted" style={{ margin: 0 }}>
          Everything lives in <code>{s?.data_dir ?? "~/.bankan/data"}</code> as Markdown files. BanKan can commit that folder to git on a timer and push it to a
          remote (for example a private GitHub repo), using your normal git credentials.
        </p>
        {cfg && (
          <div className="col">
            <label className="row" style={{ gap: 8 }}>
              <input type="checkbox" checked={cfg.backup.enabled} onChange={(e) => setCfg({ ...cfg, backup: { ...cfg.backup, enabled: e.target.checked } })} />
              Commit changes automatically
            </label>
            <div className="row" style={{ alignItems: "flex-end", flexWrap: "wrap" }}>
              <label className="field" style={{ width: 150 }}>
                Every (minutes)
                <input type="number" min={1} value={cfg.backup.interval_minutes} onChange={(e) => setCfg({ ...cfg, backup: { ...cfg.backup, interval_minutes: Number(e.target.value) || 1 } })} />
              </label>
              <label className="field grow">
                Push to remote (optional)
                <input
                  className="mono"
                  placeholder="git@github.com:you/bankan-data.git"
                  value={cfg.backup.remote ?? ""}
                  onChange={(e) => setCfg({ ...cfg, backup: { ...cfg.backup, remote: e.target.value || null } })}
                />
              </label>
              <label className="field" style={{ width: 120 }}>
                Branch
                <input className="mono" value={cfg.backup.branch} onChange={(e) => setCfg({ ...cfg, backup: { ...cfg.backup, branch: e.target.value } })} />
              </label>
            </div>
            <div className="row">
              <button className="primary" onClick={() => save.mutate(cfg)}>
                Save
              </button>
              <button onClick={() => backup.mutate()} disabled={backup.isPending}>
                {backup.isPending ? "Backing up…" : "Back up now"}
              </button>
              <span className="spacer" />
              {b && (
                <span className="small faint">
                  Last run {b.last_run ? timeAgo(b.last_run) : "never"}
                  {b.last_push && ` · pushed ${timeAgo(b.last_push)}`}
                </span>
              )}
            </div>
            {b?.last_error && <div className="error">Last backup failed: {b.last_error}</div>}
          </div>
        )}
      </section>

      <section className="section">
        <h2>Connect AI agents</h2>
        <p className="muted" style={{ margin: 0 }}>
          Register BanKan's MCP server once, for every repo. In a repo linked with <code>bankan link KEY</code>, agents use that project automatically.
        </p>
        <pre className="md" style={{ margin: 0 }}>
          <code>claude mcp add bankan --scope user -- bankan mcp</code>
        </pre>
        <p className="muted small" style={{ margin: 0 }}>
          Other MCP clients: run <code>bankan mcp</code> as a stdio server with the repo as its working directory. Slash commands in Claude Code:{" "}
          <code>/bankan:next</code>, <code>/bankan:wrap</code>, <code>/bankan:plan</code>.
        </p>
      </section>

      {archived.data && archived.data.some((p) => p.archived) && (
        <section className="section">
          <h2>Archived projects</h2>
          {archived.data
            .filter((p) => p.archived)
            .map((p) => (
              <Link key={p.key} to={`/p/${p.key}/settings`} className="row">
                <span className="project-key">{p.key}</span> {p.name}
              </Link>
            ))}
        </section>
      )}

      <section className="section">
        <h2>About</h2>
        {s && (
          <div className="col small muted" style={{ gap: 4 }}>
            <span>BanKan {s.version}, listening on port {s.port}</span>
            <span>
              Home: <code>{s.home}</code>
            </span>
            {cfg && (
              <label className="row" style={{ gap: 8 }}>
                Port
                <input type="number" style={{ width: 90 }} value={cfg.port} onChange={(e) => setCfg({ ...cfg, port: Number(e.target.value) })} />
                <span className="faint">(takes effect after a restart)</span>
              </label>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
