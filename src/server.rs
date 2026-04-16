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
    routing::{get, post},
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

#[derive(Serialize)]
struct TagGraphNode {
    id: String,
    count: usize,
    resolved: bool,
}

#[derive(Serialize)]
struct TagGraphLink {
    source: String,
    target: String,
    weight: usize,
}

#[derive(Serialize)]
struct TagGraphResponse {
    nodes: Vec<TagGraphNode>,
    links: Vec<TagGraphLink>,
}

#[derive(serde::Deserialize)]
struct MissingTagsParams {
    note: Option<String>,
}

#[derive(Serialize)]
struct MissingTagHit {
    note_path: String,
    missing_tag: String,
    snippet: String,
}

#[derive(Serialize)]
struct MissingTagsResponse {
    hits: Vec<MissingTagHit>,
}

#[derive(serde::Deserialize)]
struct NoteParams {
    path: String,
}

#[derive(Serialize)]
struct NoteResponse {
    ok: bool,
    path: String,
    title: String,
    body: String,
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
        .route("/api/tag-graph", get(tag_graph_json))
        .route("/api/missing-tags", get(missing_tags_json))
        .route("/api/note", get(note_json).post(create_note_json))
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

async fn tag_graph_json(State(state): State<AppState>) -> Response {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || build_tag_graph(db.as_ref()))
        .await
        .unwrap_or_else(|_| TagGraphResponse { nodes: Vec::new(), links: Vec::new() });
    Json(result).into_response()
}

fn build_tag_graph(db: &FlowstoneDb) -> TagGraphResponse {
    let note_paths: HashSet<String> = match db.run_default("?[path] := *notes{path}") {
        Ok(r) => r.rows.into_iter()
            .filter_map(|row| row.into_iter().next().and_then(|v| v.get_str().map(String::from)))
            .collect(),
        Err(_) => HashSet::new(),
    };

    let nodes: Vec<TagGraphNode> = match db.run_default(
        "?[tag, count(note)] := *links[note, tag] :order -count(note)"
    ) {
        Ok(r) => r.rows.into_iter().filter_map(|row| {
            let id    = row.first().and_then(|v| v.get_str()).unwrap_or("").to_string();
            let count = row.get(1).and_then(|v| v.get_int()).unwrap_or(0) as usize;
            if id.is_empty() || count < 2 { return None; }
            let resolved = note_paths.contains(&id);
            Some(TagGraphNode { id, count, resolved })
        }).collect(),
        Err(e) => { eprintln!("[tag-graph] node query failed: {}", e); Vec::new() }
    };

    let node_set: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    let links: Vec<TagGraphLink> = match db.run_default(
        "?[tag1, tag2, count(note)] := *links[note, tag1], *links[note, tag2], tag1 < tag2 :order -count(note)"
    ) {
        Ok(r) => r.rows.into_iter().filter_map(|row| {
            let source = row.first().and_then(|v| v.get_str()).unwrap_or("").to_string();
            let target = row.get(1).and_then(|v| v.get_str()).unwrap_or("").to_string();
            let weight = row.get(2).and_then(|v| v.get_int()).unwrap_or(0) as usize;
            if weight < 2 || !node_set.contains(source.as_str()) || !node_set.contains(target.as_str()) {
                return None;
            }
            Some(TagGraphLink { source, target, weight })
        }).collect(),
        Err(e) => { eprintln!("[tag-graph] edge query failed: {}", e); Vec::new() }
    };

    TagGraphResponse { nodes, links }
}

async fn missing_tags_json(
    State(state): State<AppState>,
    Query(params): Query<MissingTagsParams>,
) -> Response {
    let db = state.db.clone();
    let filter = params.note;
    let result = tokio::task::spawn_blocking(move || build_missing_tags(db.as_ref(), filter))
        .await
        .unwrap_or_else(|_| MissingTagsResponse { hits: Vec::new() });
    Json(result).into_response()
}

fn build_missing_tags(db: &FlowstoneDb, note_filter: Option<String>) -> MissingTagsResponse {
    // Fetch every known tag target from the links relation. These are the
    // vocabulary of things we'll look for in note bodies.
    let tag_targets: Vec<String> = match db.run_default("?[target] := *links[_, target]") {
        Ok(r) => r
            .rows
            .into_iter()
            .filter_map(|row| {
                row.into_iter()
                    .next()
                    .and_then(|v| v.get_str().map(String::from))
            })
            .filter(|s| s.len() >= 3) // skip two-letter targets — too noisy
            .collect(),
        Err(e) => {
            eprintln!("[missing-tags] links query failed: {}", e);
            return MissingTagsResponse { hits: Vec::new() };
        }
    };
    if tag_targets.is_empty() {
        return MissingTagsResponse { hits: Vec::new() };
    }

    // Fetch note bodies, optionally filtered to a single path.
    let notes_script = if note_filter.is_some() {
        "?[path, body] := *notes{path, body}, path = $path"
    } else {
        "?[path, body] := *notes{path, body}"
    };
    let mut params: std::collections::BTreeMap<String, cozo::DataValue> =
        std::collections::BTreeMap::new();
    if let Some(p) = &note_filter {
        params.insert("path".to_string(), cozo::DataValue::Str(p.as_str().into()));
    }
    let notes_result = match db.run_script(notes_script, params, cozo::ScriptMutability::Immutable)
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[missing-tags] notes query failed: {}", e);
            return MissingTagsResponse { hits: Vec::new() };
        }
    };

    let wiki_link_re = match regex::Regex::new(r"\[\[[^\]]*\]\]") {
        Ok(r) => r,
        Err(_) => return MissingTagsResponse { hits: Vec::new() },
    };

    let mut hits = Vec::new();
    for row in notes_result.rows {
        let path = row
            .first()
            .and_then(|v| v.get_str())
            .unwrap_or("")
            .to_string();
        let body = row
            .get(1)
            .and_then(|v| v.get_str())
            .unwrap_or("")
            .to_string();
        if path.is_empty() || body.is_empty() {
            continue;
        }

        // Strip existing wiki-links from the body so already-tagged text
        // doesn't get re-flagged. Replace with spaces of equal length so
        // match offsets are preserved for snippet extraction.
        let stripped = wiki_link_re.replace_all(&body, |caps: &regex::Captures<'_>| {
            " ".repeat(caps[0].len())
        });

        // Build a single alternation regex over all tags for this note,
        // escaping each and excluding the note's own path (so the note
        // doesn't flag its own title mentions). Word boundaries ensure we
        // don't match substrings like "rust" inside "trust".
        let alternation: Vec<String> = tag_targets
            .iter()
            .filter(|t| t.as_str() != path.as_str())
            .map(|t| regex::escape(t))
            .collect();
        if alternation.is_empty() {
            continue;
        }
        let pattern = format!(r"(?i)\b({})\b", alternation.join("|"));
        let tag_re = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[missing-tags] regex build failed: {}", e);
                continue;
            }
        };

        // Record at most one hit per (note, tag) pair — the first occurrence
        // is enough to show the user the context.
        let mut seen: HashSet<String> = HashSet::new();
        for m in tag_re.find_iter(&stripped) {
            let matched = m.as_str().to_lowercase();
            if !seen.insert(matched.clone()) {
                continue;
            }
            let snippet = snippet_around(&stripped, m.start(), m.end());
            hits.push(MissingTagHit {
                note_path: path.clone(),
                missing_tag: matched,
                snippet,
            });
        }
    }

    MissingTagsResponse { hits }
}

fn snippet_around(text: &str, start: usize, end: usize) -> String {
    const RADIUS: usize = 40;
    let pre_start = text[..start]
        .char_indices()
        .rev()
        .take(RADIUS)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let post_end = text[end..]
        .char_indices()
        .take(RADIUS)
        .last()
        .map(|(i, c)| end + i + c.len_utf8())
        .unwrap_or(text.len());
    let prefix = if pre_start > 0 { "…" } else { "" };
    let suffix = if post_end < text.len() { "…" } else { "" };
    let snippet: String = text[pre_start..post_end]
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}{}{}", prefix, snippet, suffix)
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

async fn note_json(State(state): State<AppState>, Query(params): Query<NoteParams>) -> Response {
    let path = params.path.trim().to_string();
    if path.is_empty() {
        return Json(NoteResponse {
            ok: false,
            path,
            title: String::new(),
            body: String::new(),
        })
        .into_response();
    }
    let db = state.db.clone();
    let requested = path.clone();
    let fetched = tokio::task::spawn_blocking(move || fetch_note(db.as_ref(), path))
        .await
        .ok()
        .flatten();
    match fetched {
        Some(r) => Json(r).into_response(),
        None => Json(NoteResponse {
            ok: false,
            path: requested,
            title: String::new(),
            body: String::new(),
        })
        .into_response(),
    }
}

fn fetch_note(db: &FlowstoneDb, path: String) -> Option<NoteResponse> {
    use std::collections::BTreeMap;

    use cozo::{DataValue, ScriptMutability};

    let mut params: BTreeMap<String, DataValue> = BTreeMap::new();
    params.insert("p".to_string(), DataValue::Str(path.as_str().into()));

    let script = "?[title, body] := *notes{path, title, body}, path = $p";
    match db.run_script(script, params, ScriptMutability::Immutable) {
        Ok(r) => {
            let row = r.rows.first()?;
            let title = row
                .first()
                .and_then(|v| v.get_str())
                .unwrap_or("")
                .to_string();
            let body = row
                .get(1)
                .and_then(|v| v.get_str())
                .unwrap_or("")
                .to_string();
            Some(NoteResponse {
                ok: true,
                path,
                title,
                body,
            })
        }
        Err(e) => {
            eprintln!("[note] query failed: {}", e);
            None
        }
    }
}

#[derive(serde::Deserialize)]
struct CreateNoteBody {
    path: String,
}

async fn create_note_json(
    State(state): State<AppState>,
    Json(body): Json<CreateNoteBody>,
) -> Response {
    let path = body.path.trim().to_string();
    if path.is_empty() || path.contains("..") {
        return Json(serde_json::json!({"ok": false, "message": "invalid path"})).into_response();
    }
    let file_path = state.notes_dir.join(&path).with_extension("md");
    let title = std::path::Path::new(&path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let content = format!("# {}\n\n", title);
    if let Some(parent) = file_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Json(serde_json::json!({"ok": false, "message": e.to_string()})).into_response();
        }
    }
    match std::fs::write(&file_path, &content) {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => Json(serde_json::json!({"ok": false, "message": e.to_string()})).into_response(),
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
