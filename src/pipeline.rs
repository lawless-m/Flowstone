use std::path::Path;

use flowstone_core::LoadStats;

use crate::database::FlowstoneDb;
use crate::scanner;

pub fn load(db: &FlowstoneDb, notes_dir: &Path) -> LoadStats {
    let notes = scanner::scan(notes_dir);
    flowstone_core::build(db, &notes)
}
