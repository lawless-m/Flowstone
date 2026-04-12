use std::path::Path;

use cozo::DbInstance;

use crate::database;
use crate::parser;
use crate::scanner;

pub struct LoadStats {
    pub notes: usize,
    pub links: usize,
}

pub fn load(db: &DbInstance, notes_dir: &Path) -> LoadStats {
    let notes = scanner::scan(notes_dir);
    let mut all_links = Vec::new();
    for note in &notes {
        all_links.extend(parser::parse_links(&note.path, &note.abs_path));
    }

    database::create_schema(db);
    database::load_notes(db, &notes);
    database::load_links(db, &all_links);

    LoadStats {
        notes: notes.len(),
        links: all_links.len(),
    }
}
