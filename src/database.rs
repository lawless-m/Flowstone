use std::collections::BTreeMap;
use std::path::Path;

use cozo::{DataValue, DbInstance, Num, ScriptMutability};

use crate::parser::Link;
use crate::scanner::Note;

pub fn open(db_path: &Path) -> DbInstance {
    DbInstance::new("redb", db_path, "").expect("Failed to open database")
}

pub fn create_schema(db: &DbInstance) {
    // Drop existing relations (ignore errors if they don't exist yet)
    let _ = db.run_default("::remove notes");
    let _ = db.run_default("::remove links");

    db.run_default(":create notes { path: String => title: String, size: Int, modified: Float }")
        .expect("Failed to create notes relation");

    db.run_default(":create links { source: String, target: String }")
        .expect("Failed to create links relation");
}

pub fn load_notes(db: &DbInstance, notes: &[Note]) {
    if notes.is_empty() {
        return;
    }

    let rows: Vec<DataValue> = notes
        .iter()
        .map(|note| {
            let meta = std::fs::metadata(&note.abs_path).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            DataValue::List(vec![
                DataValue::Str(note.path.as_str().into()),
                DataValue::Str(note.title.as_str().into()),
                DataValue::Num(Num::Int(size as i64)),
                DataValue::Num(Num::Float(modified)),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    if let Err(e) = db.run_script(
        "?[path, title, size, modified] <- $data :put notes {path => title, size, modified}",
        params,
        ScriptMutability::Mutable,
    ) {
        eprintln!("Warning: failed to bulk-insert notes: {}", e);
    }
}

pub fn load_links(db: &DbInstance, links: &[Link]) {
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

pub fn note_count(db: &DbInstance) -> usize {
    db.run_default("?[count(path)] := *notes{path}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn link_count(db: &DbInstance) -> usize {
    db.run_default("?[count(source)] := *links[source, _]")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn dangling_count(db: &DbInstance) -> usize {
    db.run_default("?[count(target)] := *links[_, target], not *notes{path: target}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}
