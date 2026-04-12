use std::collections::BTreeMap;
use std::path::Path;

use cozo::{new_cozo_redb, DataValue, Db, Num, RedbStorage, ScriptMutability};

use crate::parser::Link;
use crate::scanner::Note;

pub type FlowstoneDb = Db<RedbStorage>;

pub fn open(db_path: &Path) -> FlowstoneDb {
    clean_orphan_sidecar(db_path);
    new_cozo_redb(db_path).expect("Failed to open database")
}

/// If the redb file is gone but the tantivy sidecar directory is still on
/// disk, delete the sidecar. cozo derives the sidecar path as `<db>.ft/` at
/// `::fts create` time, so a stale sidecar left over from a previous run
/// would shadow the new one.
fn clean_orphan_sidecar(db_path: &Path) {
    let sidecar = sidecar_path(db_path);
    if !db_path.exists() && sidecar.is_dir() {
        match std::fs::remove_dir_all(&sidecar) {
            Ok(()) => eprintln!(
                "[flowstone] removed orphan FTS sidecar at {}",
                sidecar.display()
            ),
            Err(e) => eprintln!(
                "[flowstone] failed to remove orphan FTS sidecar at {}: {}",
                sidecar.display(),
                e
            ),
        }
    }
}

fn sidecar_path(db_path: &Path) -> std::path::PathBuf {
    let mut s = db_path.as_os_str().to_os_string();
    s.push(".ft");
    std::path::PathBuf::from(s)
}

pub fn create_schema(db: &FlowstoneDb) {
    // Drop the FTS index before removing the base relation — cozo's
    // destroy_relation refuses to drop a relation that still has indices
    // attached. All three drops are best-effort: on first run none exist yet.
    let _ = db.run_default("::fts drop notes:ft");
    let _ = db.run_default("::remove notes");
    let _ = db.run_default("::remove links");

    db.run_default(
        ":create notes { path: String => title: String, body: String, size: Int, modified: Float }",
    )
    .expect("Failed to create notes relation");

    db.run_default(":create links { source: String, target: String }")
        .expect("Failed to create links relation");

    db.run_default("::fts create notes:ft { fields: [title, body] }")
        .expect("Failed to create notes FTS index");
}

pub fn load_notes(db: &FlowstoneDb, notes: &[Note]) {
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
                DataValue::Str(note.body.as_str().into()),
                DataValue::Num(Num::Int(size as i64)),
                DataValue::Num(Num::Float(modified)),
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

pub fn load_links(db: &FlowstoneDb, links: &[Link]) {
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

pub fn note_count(db: &FlowstoneDb) -> usize {
    db.run_default("?[count(path)] := *notes{path}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn link_count(db: &FlowstoneDb) -> usize {
    db.run_default("?[count(source)] := *links[source, _]")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}

pub fn dangling_count(db: &FlowstoneDb) -> usize {
    db.run_default("?[count(target)] := *links[_, target], not *notes{path: target}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.get_int().map(|i| i as usize))
        .unwrap_or(0)
}
