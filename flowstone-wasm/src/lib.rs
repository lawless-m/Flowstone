//! Flowstone in the browser: hand this crate a zip of Markdown notes and
//! you get back an in-memory CozoDB with the same schema the native
//! `flowstone` binary builds, queryable via Datalog. No server required.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use wasm_bindgen::prelude::*;

use cozo::{new_cozo_mem, DataValue, Db, MemStorage, ScriptMutability};
use flowstone_core::{build, dangling_count, link_count, note_count, Note};

#[wasm_bindgen]
pub struct Flowstone {
    db: Db<MemStorage>,
    notes: usize,
    links: usize,
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
        Ok(Flowstone {
            db,
            notes: stats.notes,
            links: stats.links,
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
        let title = path.rsplit('/').next().unwrap_or(&path).to_string();

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

fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
