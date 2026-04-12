use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::get,
    Json, Router,
};
use cozo::DbInstance;
use futures::Stream;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::watcher;

const INDEX_HTML: &str = include_str!("../static/index.html");
const GRAPH_JS: &str = include_str!("../static/graph.js");
const STYLE_CSS: &str = include_str!("../static/style.css");

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<DbInstance>,
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

pub async fn run(
    db: Arc<DbInstance>,
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

fn build_graph(db: &DbInstance) -> GraphResponse {
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
