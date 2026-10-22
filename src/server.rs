//! The daemon: REST API, SSE change feed, embedded web UI and the backup loop.

use crate::backup::{self, BackupStatus};
use crate::config::{self, Config};
use crate::model::Event;
use crate::store::{self, DocInput, MoveInput, NewProject, ProjectPatch, ProposalInput, Store, TicketFilter, TicketInput};
use axum::body::Body;
use axum::extract::{FromRequest, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

#[derive(rust_embed::RustEmbed)]
#[folder = "web/dist"]
struct Assets;

pub struct AppState {
    store: RwLock<Store>,
    events: broadcast::Sender<Event>,
    config: RwLock<Config>,
    home: PathBuf,
    /// Last backup result. A plain mutex, held only to copy the value, so
    /// readers never wait on a running git command.
    backup: std::sync::Mutex<BackupStatus>,
    /// Held for the duration of a backup run so runs don't overlap.
    backup_running: Mutex<()>,
    /// Flips to true on shutdown so long-lived SSE streams end and the server can exit.
    shutdown: tokio::sync::watch::Sender<bool>,
}

type S = State<Arc<AppState>>;
type ApiResult<T = Json<Value>> = Result<T, ApiError>;

pub struct ApiError(store::Error);

impl From<store::Error> for ApiError {
    fn from(e: store::Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match &self.0 {
            store::Error::NotFound(_) => StatusCode::NOT_FOUND,
            store::Error::Invalid(_) => StatusCode::BAD_REQUEST,
            store::Error::Conflict(_) => StatusCode::CONFLICT,
            store::Error::Internal(e) => {
                eprintln!("bankan: internal error: {e:#}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (code, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

/// A JSON request body whose parse errors (bad types, unknown fields) come back
/// as our usual `{"error": …}` 400 instead of axum's plain-text 422, so the UI
/// and the MCP server can show them.
pub struct JsonBody<T>(T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for JsonBody<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(JsonBody(v)),
            Err(e) => Err(ApiError(store::Error::Invalid(e.body_text()))),
        }
    }
}

fn to_json<T: serde::Serialize>(v: T) -> ApiResult {
    Ok(Json(serde_json::to_value(v).map_err(|e| store::Error::Internal(e.into()))?))
}

impl AppState {
    fn emit(&self, kind: &'static str, action: &'static str, project: &str, id: Option<&str>) {
        let _ = self.events.send(Event { kind, action, project: project.to_string(), id: id.map(str::to_string) });
    }
}

fn project_of(id: &str) -> String {
    id.rsplit_once('-').map(|(k, _)| k.to_ascii_uppercase()).unwrap_or_default()
}

// --- security ---------------------------------------------------------------

/// Only answer requests addressed to localhost (blocks DNS rebinding) and,
/// when a browser sends an Origin, only from a localhost page (blocks other
/// websites from calling the API).
async fn local_only(req: Request, next: Next) -> Response {
    let is_local_host = |h: &str| {
        let host = if h.starts_with('[') { h.split(']').next().map(|s| format!("{s}]")).unwrap_or_default() } else { h.split(':').next().unwrap_or("").to_string() };
        matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
    };
    let headers = req.headers();
    let host_ok = headers.get(header::HOST).and_then(|h| h.to_str().ok()).is_none_or(is_local_host);
    let origin_ok = headers
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .is_none_or(|o| o.strip_prefix("http://").is_some_and(is_local_host));
    if !host_ok || !origin_ok {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "BanKan only accepts requests from localhost" }))).into_response();
    }
    next.run(req).await
}

// --- status & config --------------------------------------------------------

async fn status(State(s): S) -> ApiResult {
    let cfg = s.config.read().await.clone();
    let backup = s.backup.lock().unwrap().clone();
    let store = s.store.read().await;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "home": s.home,
        "data_dir": store.root(),
        "port": cfg.port,
        "projects": store.list_projects(true).len(),
        "backup": { "config": cfg.backup, "status": backup },
    })))
}

async fn get_config(State(s): S) -> ApiResult {
    to_json(s.config.read().await.clone())
}

async fn put_config(State(s): S, JsonBody(mut cfg): JsonBody<Config>) -> ApiResult {
    if cfg.backup.interval_minutes == 0 {
        cfg.backup.interval_minutes = 1;
    }
    cfg.backup.remote = cfg.backup.remote.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    if cfg.backup.branch.trim().is_empty() {
        cfg.backup.branch = "main".into();
    }
    cfg.save(&s.home).map_err(store::Error::from)?;
    *s.config.write().await = cfg.clone();
    to_json(cfg)
}

async fn run_backup(s: Arc<AppState>) -> BackupStatus {
    let _running = s.backup_running.lock().await;
    let cfg = s.config.read().await.backup.clone();
    let dir = s.store.read().await.root().to_path_buf();
    let prev = s.backup.lock().unwrap().clone();
    let mut st = prev.clone();
    let st = tokio::task::spawn_blocking(move || {
        backup::run(&dir, &cfg, &mut st);
        st
    })
    .await
    .unwrap_or_else(|e| BackupStatus { last_error: Some(format!("backup task failed: {e}")), ..prev });
    *s.backup.lock().unwrap() = st.clone();
    st
}

async fn backup_now(State(s): S) -> ApiResult {
    to_json(run_backup(s).await)
}

// --- projects ---------------------------------------------------------------

#[derive(Deserialize)]
struct ListProjectsQuery {
    #[serde(default)]
    archived: bool,
}

async fn list_projects(State(s): S, Query(q): Query<ListProjectsQuery>) -> ApiResult {
    to_json(s.store.read().await.list_projects(q.archived))
}

async fn create_project(State(s): S, JsonBody(input): JsonBody<NewProject>) -> ApiResult {
    let p = s.store.write().await.create_project(input)?;
    s.emit("project", "created", &p.key, None);
    to_json(p)
}

async fn get_project(State(s): S, Path(key): Path<String>) -> ApiResult {
    to_json(s.store.read().await.get_project(&key)?)
}

async fn update_project(State(s): S, Path(key): Path<String>, JsonBody(patch): JsonBody<ProjectPatch>) -> ApiResult {
    let p = s.store.write().await.update_project(&key, patch)?;
    s.emit("project", "updated", &p.key, None);
    to_json(p)
}

async fn delete_project(State(s): S, Path(key): Path<String>) -> ApiResult {
    s.store.write().await.delete_project(&key)?;
    s.emit("project", "deleted", &key.to_ascii_uppercase(), None);
    Ok(Json(json!({ "ok": true })))
}

// --- tickets ----------------------------------------------------------------

async fn list_tickets(State(s): S, Path(key): Path<String>, Query(f): Query<TicketFilter>) -> ApiResult {
    to_json(s.store.read().await.list_tickets(&key, &f)?)
}

async fn create_ticket(State(s): S, Path(key): Path<String>, JsonBody(input): JsonBody<TicketInput>) -> ApiResult {
    let t = s.store.write().await.create_ticket(&key, input)?;
    s.emit("ticket", "created", &project_of(&t.id), Some(&t.id));
    to_json(t)
}

async fn get_ticket(State(s): S, Path(id): Path<String>) -> ApiResult {
    let store = s.store.read().await;
    let t = store.get_ticket(&id)?;
    let (blocked_by, children) = store.ticket_backlinks(&t.id);
    let mut v = serde_json::to_value(t).map_err(store::Error::from)?;
    v["blocked_by"] = json!(blocked_by);
    v["children"] = json!(children);
    Ok(Json(v))
}

async fn update_ticket(State(s): S, Path(id): Path<String>, JsonBody(input): JsonBody<TicketInput>) -> ApiResult {
    let t = s.store.write().await.update_ticket(&id, input)?;
    s.emit("ticket", "updated", &project_of(&t.id), Some(&t.id));
    to_json(t)
}

async fn move_ticket(State(s): S, Path(id): Path<String>, JsonBody(input): JsonBody<MoveInput>) -> ApiResult {
    let t = s.store.write().await.move_ticket(&id, input)?;
    s.emit("ticket", "moved", &project_of(&t.id), Some(&t.id));
    to_json(t)
}

async fn delete_ticket(State(s): S, Path(id): Path<String>) -> ApiResult {
    s.store.write().await.delete_ticket(&id)?;
    s.emit("ticket", "deleted", &project_of(&id), Some(&id.to_ascii_uppercase()));
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentInput {
    #[serde(default)]
    author: String,
    body: String,
}

async fn add_comment(State(s): S, Path(id): Path<String>, JsonBody(c): JsonBody<CommentInput>) -> ApiResult {
    let t = s.store.write().await.add_comment(&id, &c.author, &c.body)?;
    s.emit("ticket", "updated", &project_of(&t.id), Some(&t.id));
    to_json(t)
}

async fn delete_comment(State(s): S, Path((id, cid)): Path<(String, u32)>) -> ApiResult {
    let t = s.store.write().await.delete_comment(&id, cid)?;
    s.emit("ticket", "updated", &project_of(&t.id), Some(&t.id));
    to_json(t)
}

// --- proposals --------------------------------------------------------------

async fn list_proposals(State(s): S, Path(key): Path<String>) -> ApiResult {
    to_json(s.store.read().await.list_proposals(&key)?)
}

async fn create_proposal(State(s): S, Path(key): Path<String>, JsonBody(input): JsonBody<ProposalInput>) -> ApiResult {
    let p = s.store.write().await.create_proposal(&key, input)?;
    s.emit("proposal", "created", &project_of(&p.id), Some(&p.id));
    to_json(p)
}

async fn get_proposal(State(s): S, Path(id): Path<String>) -> ApiResult {
    to_json(s.store.read().await.get_proposal(&id)?)
}

/// By key or alias (`D17`) within a project.
async fn find_proposal(State(s): S, Path((key, id)): Path<(String, String)>) -> ApiResult {
    to_json(s.store.read().await.find_proposal(&key, &id)?)
}

async fn update_proposal(State(s): S, Path(id): Path<String>, JsonBody(input): JsonBody<ProposalInput>) -> ApiResult {
    let p = s.store.write().await.update_proposal(&id, input)?;
    s.emit("proposal", "updated", &project_of(&p.id), Some(&p.id));
    to_json(p)
}

async fn delete_proposal(State(s): S, Path(id): Path<String>) -> ApiResult {
    s.store.write().await.delete_proposal(&id)?;
    s.emit("proposal", "deleted", &project_of(&id), Some(&id.to_ascii_uppercase()));
    Ok(Json(json!({ "ok": true })))
}

// --- docs -------------------------------------------------------------------

async fn list_docs(State(s): S, Path(key): Path<String>) -> ApiResult {
    to_json(s.store.read().await.list_docs(&key)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDoc {
    path: Option<String>,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_doc(State(s): S, Path(key): Path<String>, JsonBody(input): JsonBody<NewDoc>) -> ApiResult {
    let path = input.path.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| store::slugify(&input.title));
    let mut store = s.store.write().await;
    if store.get_doc(&key, &path).is_ok() {
        return Err(store::Error::Conflict(format!("doc '{path}' already exists")).into());
    }
    let d = store.put_doc(&key, &path, DocInput { title: Some(input.title), body: Some(input.body), tags: Some(input.tags), new_path: None, ..Default::default() })?;
    drop(store);
    s.emit("doc", "created", &key.to_ascii_uppercase(), Some(&d.path));
    to_json(d)
}

async fn get_doc(State(s): S, Path((key, path)): Path<(String, String)>) -> ApiResult {
    to_json(s.store.read().await.get_doc(&key, &path)?)
}

async fn put_doc(State(s): S, Path((key, path)): Path<(String, String)>, JsonBody(input): JsonBody<DocInput>) -> ApiResult {
    let d = s.store.write().await.put_doc(&key, &path, input)?;
    s.emit("doc", "updated", &key.to_ascii_uppercase(), Some(&d.path));
    to_json(d)
}

async fn delete_doc(State(s): S, Path((key, path)): Path<(String, String)>) -> ApiResult {
    s.store.write().await.delete_doc(&key, &path)?;
    s.emit("doc", "deleted", &key.to_ascii_uppercase(), Some(&path));
    Ok(Json(json!({ "ok": true })))
}

// --- search & events --------------------------------------------------------

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    project: Option<String>,
    limit: Option<usize>,
}

async fn search(State(s): S, Query(q): Query<SearchQuery>) -> ApiResult {
    to_json(s.store.read().await.search(&q.q, q.project.as_deref(), q.limit.unwrap_or(30))?)
}

async fn events(State(s): S) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let mut shutdown = s.shutdown.subscribe();
    let stream = BroadcastStream::new(s.events.subscribe()).filter_map(|e| {
        // A slow client that fell behind gets a "resync" so it refetches instead of missing changes.
        let e = e.unwrap_or_else(|_| Event { kind: "resync", action: "lagged", project: String::new(), id: None });
        Some(Ok(axum::response::sse::Event::default().json_data(e).ok()?))
    });
    let stream = futures::StreamExt::take_until(stream, async move {
        let _ = shutdown.wait_for(|stop| *stop).await;
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

// --- static UI --------------------------------------------------------------

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such endpoint" }))).into_response();
    }
    let (file, path) = match Assets::get(path) {
        Some(f) if !path.is_empty() => (f, path),
        // Client-side routes all serve the app shell.
        _ => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return (StatusCode::NOT_FOUND, "web UI not built").into_response(),
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap());
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    (headers, Body::from(file.data.into_owned())).into_response()
}

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/config", get(get_config).put(put_config))
        .route("/backup", post(backup_now))
        .route("/search", get(search))
        .route("/events", get(events))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{key}", get(get_project).patch(update_project).delete(delete_project))
        .route("/projects/{key}/tickets", get(list_tickets).post(create_ticket))
        .route("/projects/{key}/proposals", get(list_proposals).post(create_proposal))
        .route("/projects/{key}/proposals/{id}", get(find_proposal))
        .route("/projects/{key}/docs", get(list_docs).post(create_doc))
        .route("/projects/{key}/docs/{*path}", get(get_doc).put(put_doc).delete(delete_doc))
        .route("/tickets/{id}", get(get_ticket).patch(update_ticket).delete(delete_ticket))
        .route("/tickets/{id}/move", post(move_ticket))
        .route("/tickets/{id}/comments", post(add_comment))
        .route("/tickets/{id}/comments/{cid}", axum::routing::delete(delete_comment))
        .route("/proposals/{id}", get(get_proposal).patch(update_proposal).delete(delete_proposal));
    Router::new()
        .nest("/api", api)
        .fallback(static_file)
        .layer(middleware::from_fn(local_only))
        .with_state(state)
}

pub fn build_state(home: PathBuf, cfg: Config) -> anyhow::Result<Arc<AppState>> {
    let store = Store::open(&config::data_dir(&home))?;
    let (events, _) = broadcast::channel(256);
    let (shutdown, _) = tokio::sync::watch::channel(false);
    Ok(Arc::new(AppState { store: RwLock::new(store), events, config: RwLock::new(cfg), home, backup: Default::default(), backup_running: Mutex::new(()), shutdown }))
}

pub async fn serve(home: PathBuf) -> anyhow::Result<()> {
    let cfg = Config::load(&home)?;
    if !home.join("config.toml").exists() {
        cfg.save(&home)?;
    }
    let port = cfg.port;
    let state = build_state(home, cfg)?;

    // Backup loop: re-reads the config every tick, so changes from the UI apply without a restart.
    let bg = state.clone();
    tokio::spawn(async move {
        loop {
            let (enabled, minutes) = {
                let c = bg.config.read().await;
                (c.backup.enabled, c.backup.interval_minutes)
            };
            tokio::time::sleep(Duration::from_secs(minutes.max(1) * 60)).await;
            if enabled && bg.config.read().await.backup.enabled {
                run_backup(bg.clone()).await;
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1:{port}: {e}. Is BanKan already running? (`bankan status`)"))?;
    println!("BanKan {} listening on http://127.0.0.1:{port}", env!("CARGO_PKG_VERSION"));
    println!("data: {}", state.store.read().await.root().display());

    let stop = state.clone();
    axum::serve(listener, router(state.clone()))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = stop.shutdown.send(true);
        })
        .await?;

    if state.config.read().await.backup.enabled {
        println!("bankan: final backup before exit");
        run_backup(state).await;
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}
