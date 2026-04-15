use std::collections::BTreeMap;

use cozo::{DataValue, Db, Num, ScriptMutability, Storage};

use crate::model::{Link, LoadStats, Note};
use crate::parser;

pub fn create_schema<S>(db: &Db<S>)
where
    S: for<'s> Storage<'s> + 'static,
{
    // Drop the FTS index before removing the base relation — cozo's
    // destroy_relation refuses to drop a relation that still has indices
    // attached. All drops are best-effort: on first run none exist yet.
    #[cfg(feature = "fts")]
    let _ = db.run_default("::fts drop notes:ft");
    let _ = db.run_default("::remove notes");
    let _ = db.run_default("::remove links");

    db.run_default(
        ":create notes { path: String => title: String, body: String, size: Int, modified: Float }",
    )
    .expect("Failed to create notes relation");

    db.run_default(":create links { source: String, target: String }")
        .expect("Failed to create links relation");

    // FTS index creation: skip on wasm32 because tantivy's IndexWriter always
    // spawns background threads (segment updater + merge pool) via
    // std::thread::spawn, which panics in browser wasm regardless of whether
    // wasm-bindgen-rayon's thread pool is set up.  The flowstone-wasm crate
    // builds its own tantivy index via SingleSegmentIndexWriter instead.
    #[cfg(all(feature = "fts", not(target_arch = "wasm32")))]
    db.run_default("::fts create notes:ft { fields: [title, body] }")
        .expect("Failed to create notes FTS index");
}

pub fn load_notes<S>(db: &Db<S>, notes: &[Note])
where
    S: for<'s> Storage<'s> + 'static,
{
    if notes.is_empty() {
        return;
    }

    let rows: Vec<DataValue> = notes
        .iter()
        .map(|note| {
            DataValue::List(vec![
                DataValue::Str(note.path.as_str().into()),
                DataValue::Str(note.title.as_str().into()),
                DataValue::Str(note.body.as_str().into()),
                DataValue::Num(Num::Int(note.size as i64)),
                DataValue::Num(Num::Float(note.modified)),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    if let Err(e) = db.run_script(
        "?[path, title, body, size, modified] <- $data :put notes {path => title, body, size, modified}",
        params,
        ScriptMutability::Mutable,
    ) {
        eprintln!("Warning: failed to bulk-insert notes: {}", e);
    }
}

pub fn load_links<S>(db: &Db<S>, links: &[Link])
where
    S: for<'s> Storage<'s> + 'static,
{
    if links.is_empty() {
        return;
    }

    let rows: Vec<DataValue> = links
        .iter()
        .map(|link| {
            DataValue::List(vec![
                DataValue::Str(link.source.as_str().into()),
                DataValue::Str(link.target.as_str().into()),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    if let Err(e) = db.run_script(
        "?[source, target] <- $data :put links {source, target}",
        params,
        ScriptMutability::Mutable,
    ) {
        eprintln!("Warning: failed to bulk-insert links: {}", e);
    }
}

pub fn note_count<S>(db: &Db<S>) -> usize
where
    S: for<'s> Storage<'s> + 'static,
{
    db.run_default("?[count(path)] := *notes{path}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn link_count<S>(db: &Db<S>) -> usize
where
    S: for<'s> Storage<'s> + 'static,
{
    db.run_default("?[count(source)] := *links[source, _]")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn dangling_count<S>(db: &Db<S>) -> usize
where
    S: for<'s> Storage<'s> + 'static,
{
    db.run_default("?[count(target)] := *links[_, target], not *notes{path: target}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

/// Parse links from every note's body, (re)create the schema, and load
/// notes and links into the database. The one-shot pipeline entry point
/// that both the native scanner path and the wasm zip path call.
pub fn build<S>(db: &Db<S>, notes: &[Note]) -> LoadStats
where
    S: for<'s> Storage<'s> + 'static,
{
    let mut all_links = Vec::new();
    for note in notes {
        all_links.extend(parser::parse_links(&note.path, &note.body));
    }

    create_schema(db);
    load_notes(db, notes);
    load_links(db, &all_links);

    LoadStats {
        notes: notes.len(),
        links: all_links.len(),
    }
}
