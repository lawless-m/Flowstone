use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::get,
    Json, Router,
};
use futures::Stream;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::database::FlowstoneDb;
use crate::watcher;

const INDEX_HTML: &str = include_str!("../static/index.html");
const GRAPH_JS: &str = include_str!("../static/graph.js");
const STYLE_CSS: &str = include_str!("../static/style.css");

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<FlowstoneDb>,
    pub notes_dir: PathBuf,
    pub reload_tx: broadcast::Sender<()>,
}

#[derive(Serialize)]
struct GraphNode {
    id: String,
    title: String,
    in_degree: usize,
    out_degree: usize,
    is_hub: bool,
}

#[derive(Serialize)]
struct GraphLink {
    source: String,
    target: String,
}

#[derive(Serialize)]
struct GraphResponse {
    nodes: Vec<GraphNode>,
    links: Vec<GraphLink>,
}

#[derive(serde::Deserialize)]
struct SearchParams {
    q: String,
}

#[derive(Serialize)]
struct SearchHit {
    path: String,
    title: String,
    score: f64,
}

#[derive(Serialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Serialize)]
struct TagInfo {
    target: String,
    count: usize,
    resolved: bool,
}

#[derive(Serialize)]
struct TagsResponse {
    tags: Vec<TagInfo>,
}

pub async fn run(
    db: Arc<FlowstoneDb>,
    notes_dir: PathBuf,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reload_tx, _) = broadcast::channel::<()>(16);

    let state = AppState {
        db,
        notes_dir,
        reload_tx,
    };

    // Spawn the file watcher — runs for the lifetime of the server.
    let watcher_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = watcher::watch(watcher_state).await {
            eprintln!("[watcher] stopped: {}", e);
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/static/graph.js", get(graph_js))
        .route("/static/style.css", get(style_css))
        .route("/api/graph", get(graph_json))
        .route("/api/search", get(search_json))
        .route("/api/tags", get(tags_json))
        .route("/api/events", get(events))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Serving on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn graph_js() -> Response {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        GRAPH_JS,
    )
        .into_response()
}

async fn style_css() -> Response {
    ([("content-type", "text/css; charset=utf-8")], STYLE_CSS).into_response()
}

async fn graph_json(State(state): State<AppState>) -> Response {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || build_graph(db.as_ref()))
        .await
        .unwrap_or_else(|_| GraphResponse {
            nodes: Vec::new(),
            links: Vec::new(),
        });
    Json(result).into_response()
}

fn build_graph(db: &FlowstoneDb) -> GraphResponse {
    let mut nodes_map: HashMap<String, GraphNode> = HashMap::new();
    let mut links: Vec<GraphLink> = Vec::new();

    match db.run_default("?[path, title] := *notes{path, title}") {
        Ok(r) => {
            for row in r.rows {
                let path = row
                    .first()
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                let title = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                if path.is_empty() {
                    continue;
                }
                nodes_map.insert(
                    path.clone(),
                    GraphNode {
                        id: path,
                        title,
                        in_degree: 0,
                        out_degree: 0,
                        is_hub: false,
                    },
                );
            }
        }
        Err(e) => eprintln!("[graph] notes query failed: {}", e),
    }

    match db.run_default("?[source, target] := *links[source, target], *notes{path: target}") {
        Ok(r) => {
            for row in r.rows {
                let source = row
                    .first()
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                let target = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                if source.is_empty() || target.is_empty() {
                    continue;
                }
                if nodes_map.contains_key(&source) && nodes_map.contains_key(&target) {
                    links.push(GraphLink { source, target });
                }
            }
        }
        Err(e) => eprintln!("[graph] links query failed: {}", e),
    }

    for link in &links {
        if let Some(n) = nodes_map.get_mut(&link.source) {
            n.out_degree += 1;
        }
        if let Some(n) = nodes_map.get_mut(&link.target) {
            n.in_degree += 1;
        }
    }

    // Simple heuristic: anything with 4+ incoming links is a "hub" for the UI.
    for n in nodes_map.values_mut() {
        n.is_hub = n.in_degree >= 4;
    }

    let mut nodes: Vec<GraphNode> = nodes_map.into_values().collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    GraphResponse { nodes, links }
}

async fn tags_json(State(state): State<AppState>) -> Response {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || build_tags(db.as_ref()))
        .await
        .unwrap_or_else(|_| TagsResponse { tags: Vec::new() });
    Json(result).into_response()
}

fn build_tags(db: &FlowstoneDb) -> TagsResponse {
    let note_paths: HashSet<String> = match db.run_default("?[path] := *notes{path}") {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| {
                row.into_iter()
                    .next()
                    .and_then(|v| v.get_str().map(String::from))
            })
            .collect(),
        Err(e) => {
            eprintln!("[tags] notes query failed: {}", e);
            HashSet::new()
        }
    };

    let mut tags = Vec::new();
    match db.run_default("?[target, count(source)] := *links[source, target] :order -count(source)")
    {
        Ok(r) => {
            for row in r.rows {
                let target = row
                    .first()
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                let count = row.get(1).and_then(|v| v.get_int()).unwrap_or(0) as usize;
                if target.is_empty() {
                    continue;
                }
                let resolved = note_paths.contains(&target);
                tags.push(TagInfo {
                    target,
                    count,
                    resolved,
                });
            }
        }
        Err(e) => eprintln!("[tags] links query failed: {}", e),
    }

    TagsResponse { tags }
}

async fn search_json(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Response {
    let q = params.q.trim().to_string();
    if q.is_empty() {
        return Json(SearchResponse { hits: Vec::new() }).into_response();
    }
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || run_search(db.as_ref(), &q))
        .await
        .unwrap_or_else(|_| SearchResponse { hits: Vec::new() });
    Json(result).into_response()
}

fn run_search(db: &FlowstoneDb, q: &str) -> SearchResponse {
    // cozo's FTS parser requires `query` and `k` to be literals (see
    // SEARCH.md), so we can't pass `q` as a parameter — we inline it into
    // the script text. To avoid escaping, we use a cozoscript *raw* string
    // delimited by a long underscore run (`______"..."______`) so that any
    // user-supplied quotes, backslashes, etc. pass through verbatim into
    // tantivy's own query parser, preserving Lucene-style syntax (phrases,
    // +required, field:value, etc.). The raw-string delimiter would only
    // collide if the user's query contained the literal sequence `"______`,
    // which isn't a meaningful search term.
    let script = format!(
        "?[path, title, score] := ~notes:ft{{path, title | query: ______\"{}\"______, k: 50, bind_score: score}}",
        q
    );
    match db.run_default(&script) {
        Ok(r) => {
            let mut hits = Vec::with_capacity(r.rows.len());
            for row in r.rows {
                let path = row
                    .first()
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                let title = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_string();
                let score = row.get(2).and_then(|v| v.get_float()).unwrap_or(0.0);
                if path.is_empty() {
                    continue;
                }
                hits.push(SearchHit { path, title, score });
            }
            SearchResponse { hits }
        }
        Err(e) => {
            eprintln!("[search] query failed: {}", e);
            SearchResponse { hits: Vec::new() }
        }
    }
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.reload_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result
            .ok()
            .map(|_| Ok(Event::default().event("update-available").data("")))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
