use cozo::DbInstance;
use std::path::Path;

use crate::parser::Link;
use crate::scanner::Note;

pub fn open(db_path: &Path) -> DbInstance {
    DbInstance::new("redb", db_path, "")
        .expect("Failed to open database")
}

pub fn create_schema(db: &DbInstance) {
    // Drop existing relations (ignore errors if they don't exist)
    let _ = db.run_default("::remove notes");
    let _ = db.run_default("::remove links");
    let _ = db.run_default("::remove note_lookup");

    db.run_default(
        ":create notes { path: String => title: String, size: Int, modified: Float }",
    )
    .expect("Failed to create notes relation");

    db.run_default(
        ":create links { source: String, target: String }",
    )
    .expect("Failed to create links relation");

    db.run_default(
        ":create note_lookup { normalised: String => path: String }",
    )
    .expect("Failed to create note_lookup relation");
}

pub fn load_notes(db: &DbInstance, notes: &[Note]) {
    for note in notes {
        let meta = std::fs::metadata(&note.abs_path).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let path_escaped = note.path.replace('\'', "''");
        let title_escaped = note.title.replace('\'', "''");
        let normalised = note.path.to_lowercase();
        let norm_escaped = normalised.replace('\'', "''");

        let script = format!(
            "?[path, title, size, modified] <- [['{}', '{}', {}, {}]] :put notes {{path => title, size, modified}}",
            path_escaped, title_escaped, size, modified
        );
        if let Err(e) = db.run_default(&script) {
            eprintln!("Warning: failed to insert note '{}': {}", note.path, e);
        }

        let lookup_script = format!(
            "?[normalised, path] <- [['{}', '{}']] :put note_lookup {{normalised => path}}",
            norm_escaped, path_escaped
        );
        if let Err(e) = db.run_default(&lookup_script) {
            eprintln!("Warning: failed to insert lookup for '{}': {}", note.path, e);
        }
    }
}

pub fn load_links(db: &DbInstance, links: &[Link]) {
    for link in links {
        let source_escaped = link.source.replace('\'', "''");
        let target_escaped = link.target.replace('\'', "''");

        let script = format!(
            "?[source, target] <- [['{}', '{}']] :put links {{source, target}}",
            source_escaped, target_escaped
        );
        if let Err(e) = db.run_default(&script) {
            eprintln!("Warning: failed to insert link '{}' -> '{}': {}", link.source, link.target, e);
        }
    }
}

pub fn note_count(db: &DbInstance) -> usize {
    db.run_default("?[count(path)] := *notes{path}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| match v {
            v => v.get_int().map(|i| i as usize),
        })
        .unwrap_or(0)
}

pub fn link_count(db: &DbInstance) -> usize {
    db.run_default("?[count(source)] := *links[source, _]")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| match v {
            v => v.get_int().map(|i| i as usize),
        })
        .unwrap_or(0)
}

pub fn dangling_count(db: &DbInstance) -> usize {
    db.run_default("?[count(target)] := *links[_, target], not *notes{path: target}")
        .ok()
        .and_then(|r| r.rows.first().cloned())
        .and_then(|row| row.first().cloned())
        .and_then(|v| match v {
            v => v.get_int().map(|i| i as usize),
        })
        .unwrap_or(0)
}
