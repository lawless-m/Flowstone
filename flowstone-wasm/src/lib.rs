//! Flowstone in the browser: hand this crate a zip of Markdown notes and
//! you get back an in-memory CozoDB with the same schema the native
//! `flowstone` binary builds, queryable via Datalog. No server required.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use wasm_bindgen::prelude::*;

use cozo::{new_cozo_mem, DataValue, Db, MemStorage, ScriptMutability};
use flowstone_core::{build, dangling_count, link_count, note_count, Note};

// When the `fts` feature is on we build a tantivy full-text index directly —
// bypassing cozo's ::fts create because tantivy's IndexWriter always spawns
// background threads via std::thread::spawn which panics in browser wasm.
// SingleSegmentIndexWriter is thread-free and sufficient for our write-once
// use case (index is built once at zip load time and only read afterwards).
#[cfg(feature = "fts")]
use tantivy::{
    Index, IndexReader, IndexSettings, ReloadPolicy, SingleSegmentIndexWriter, TantivyDocument,
    collector::TopDocs,
    query::QueryParser,
    schema::{Field, OwnedValue, Schema, STORED, STRING, TEXT},
    store::Compressor,
};

#[cfg(feature = "fts")]
struct FtsHandle {
    index: Index,
    reader: IndexReader,
    text_fields: Vec<Field>,
    key_field: Field,
}

#[wasm_bindgen]
pub struct Flowstone {
    db: Db<MemStorage>,
    notes: usize,
    links: usize,
    #[cfg(feature = "fts")]
    fts: Option<FtsHandle>,
}

#[wasm_bindgen]
impl Flowstone {
    /// Build a new Flowstone database from a raw zip of Markdown notes.
    ///
    /// Mirrors the native scanner's path/title construction so wiki-link
    /// targets resolve the same way they do on the server: the path is
    /// the archive-relative filename with the top-level directory and
    /// `.md` extension stripped, with `\` normalised to `/`.
    pub fn from_zip(zip_bytes: &[u8]) -> Result<Flowstone, JsError> {
        set_panic_hook();

        let db = new_cozo_mem()
            .map_err(|e| JsError::new(&format!("failed to open cozo: {e}")))?;

        let notes = notes_from_zip(zip_bytes)
            .map_err(|e| JsError::new(&format!("failed to read zip: {e}")))?;

        let stats = build(&db, &notes);

        #[cfg(feature = "fts")]
        let fts = Some(
            build_fts_index(&notes)
                .map_err(|e| JsError::new(&format!("failed to build FTS index: {e}")))?,
        );

        Ok(Flowstone {
            db,
            notes: stats.notes,
            links: stats.links,
            #[cfg(feature = "fts")]
            fts,
        })
    }

    pub fn note_count(&self) -> usize {
        self.notes
    }

    pub fn link_count(&self) -> usize {
        self.links
    }

    pub fn dangling_count(&self) -> usize {
        dangling_count(&self.db)
    }

    pub fn live_note_count(&self) -> usize {
        note_count(&self.db)
    }

    pub fn live_link_count(&self) -> usize {
        link_count(&self.db)
    }

    /// Run a Datalog script. `params` is a JSON object string (empty for
    /// no params). Returns a JSON string with an `ok` field, shaped
    /// identically to `cozo-lib-wasm` so the same client code can talk
    /// to either.
    /// Full-text search via tantivy (only available in the `fts` build).
    /// Returns JSON `{"hits":[{"path":"...","title":"...","score":1.0},…]}`.
    /// Falls back to an empty hit list when called on a non-FTS build.
    #[cfg(feature = "fts")]
    pub fn fts_search(&self, query: &str, k: usize) -> String {
        let Some(ref fts) = self.fts else {
            return r#"{"hits":[]}"#.to_string();
        };
        let searcher = fts.reader.searcher();
        let parser = QueryParser::for_index(&fts.index, fts.text_fields.clone());
        let parsed = match parser.parse_query(query) {
            Ok(q) => q,
            Err(e) => return error_json(&format!("bad query: {e}")),
        };
        let top = match searcher.search(&parsed, &TopDocs::with_limit(k)) {
            Ok(t) => t,
            Err(e) => return error_json(&format!("search error: {e}")),
        };
        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = match searcher.doc(addr) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let path = doc.get_first(fts.key_field)
                .and_then(|v| if let OwnedValue::Str(s) = v { Some(s.as_str()) } else { None })
                .unwrap_or("")
                .to_string();
            let title = doc.get_first(fts.text_fields[0])
                .and_then(|v| if let OwnedValue::Str(s) = v { Some(s.as_str()) } else { None })
                .unwrap_or(&path)
                .to_string();
            hits.push(serde_json::json!({"path": path, "title": title, "score": score}));
        }
        serde_json::json!({"hits": hits}).to_string()
    }

    /// Insert a new empty note into the in-memory database.
    /// Returns `{"ok":true}` on success or `{"ok":false,"message":"..."}`.
    pub fn create_note(&mut self, path: &str) -> String {
        let path = path.trim();
        if path.is_empty() || path.contains("..") {
            return error_json("invalid path");
        }
        let title = title_from_path(path);
        let body = format!("# {}\n\n", title);
        let note = Note {
            path: path.to_string(),
            title,
            body,
            size: 0,
            modified: 0.0,
        };
        flowstone_core::load_notes(&self.db, &[note]);
        self.notes = note_count(&self.db);
        r#"{"ok":true}"#.to_string()
    }

    pub fn run(&self, script: &str, params: &str, immutable: bool) -> String {
        let params_map = match parse_params(params) {
            Ok(p) => p,
            Err(e) => return error_json(&e),
        };
        let mutability = if immutable {
            ScriptMutability::Immutable
        } else {
            ScriptMutability::Mutable
        };
        match self.db.run_script(script, params_map, mutability) {
            Ok(rows) => {
                let mut j = rows.into_json();
                if let Some(obj) = j.as_object_mut() {
                    obj.insert("ok".to_string(), serde_json::Value::Bool(true));
                }
                j.to_string()
            }
            Err(e) => error_json(&format!("{e:?}")),
        }
    }
}

/// Build an in-RAM tantivy FTS index from the loaded notes using
/// `SingleSegmentIndexWriter`, which has no background threads.
/// `Compressor::None` is used to avoid LZ4, which is unavailable in wasm.
#[cfg(feature = "fts")]
fn build_fts_index(notes: &[Note]) -> Result<FtsHandle, String> {
    let mut builder = Schema::builder();
    let key_field   = builder.add_text_field("path",  STRING | STORED);
    let title_field = builder.add_text_field("title", TEXT   | STORED);
    let body_field  = builder.add_text_field("body",  TEXT);
    let schema = builder.build();

    let settings = IndexSettings {
        docstore_compression: Compressor::None,
        docstore_compress_dedicated_thread: false,
        ..Default::default()
    };
    let index = Index::builder()
        .schema(schema)
        .settings(settings)
        .create_in_ram()
        .map_err(|e| format!("create_in_ram: {e}"))?;

    let mut writer = SingleSegmentIndexWriter::new(index, 50 * 1024 * 1024)
        .map_err(|e| format!("new_writer: {e}"))?;

    for note in notes {
        let mut doc = TantivyDocument::default();
        doc.add_text(key_field,   &note.path);
        doc.add_text(title_field, &note.title);
        doc.add_text(body_field,  &note.body);
        writer.add_document(doc).map_err(|e| format!("add_document: {e}"))?;
    }

    let index = writer.finalize().map_err(|e| format!("finalize: {e}"))?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .map_err(|e: tantivy::TantivyError| format!("reader: {e}"))?;

    Ok(FtsHandle {
        index,
        reader,
        text_fields: vec![title_field, body_field],
        key_field,
    })
}

fn title_from_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn parse_params(params: &str) -> Result<BTreeMap<String, DataValue>, String> {
    if params.is_empty() {
        return Ok(BTreeMap::new());
    }
    let parsed: serde_json::Value =
        serde_json::from_str(params).map_err(|e| format!("bad params: {e}"))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "params must be a JSON object".to_string())?;
    Ok(obj
        .iter()
        .map(|(k, v)| (k.clone(), DataValue::from(v.clone())))
        .collect())
}

fn error_json(msg: &str) -> String {
    serde_json::json!({
        "ok": false,
        "message": msg,
    })
    .to_string()
}

fn notes_from_zip(zip_bytes: &[u8]) -> Result<Vec<Note>, String> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;

    let mut notes = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;

        if entry.is_dir() {
            continue;
        }

        let raw_name = entry.name().to_string();

        // GitHub archive zips wrap everything under a top-level
        // `<repo>-<ref>/` directory. Strip that prefix so paths match
        // the native scanner's output. If there is no leading
        // directory the file lives at the zip root, in which case
        // treat its name as-is.
        let rel_name = match raw_name.split_once('/') {
            Some((_, rest)) if !rest.is_empty() => rest.to_string(),
            _ => raw_name.clone(),
        };

        // Mirror the native scanner: `.md` only.
        let lower = rel_name.to_lowercase();
        if !lower.ends_with(".md") {
            continue;
        }

        let path = rel_name[..rel_name.len() - 3].replace('\\', "/");
        let title = title_from_path(&path);

        let mut body = String::new();
        if let Err(e) = entry.read_to_string(&mut body) {
            // Non-UTF8 files: skip rather than fail the whole load.
            web_sys_warn(&format!("skipping {rel_name}: {e}"));
            continue;
        }

        let size = body.len() as u64;

        notes.push(Note {
            path,
            title,
            body,
            size,
            modified: 0.0,
        });
    }

    Ok(notes)
}

// Log a warning to the browser console without taking a hard dep on web_sys.
fn web_sys_warn(msg: &str) {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn warn(s: &str);
    }
    warn(msg);
}

// When the fts feature is on, expose wasm-bindgen-rayon's thread-pool
// initialiser so the JS host can call `await initThreadPool(N)` before
// loading any zip.  The function is a no-op on builds without fts.
#[cfg(feature = "fts")]
pub use wasm_bindgen_rayon::init_thread_pool;

fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
