use std::path::Path;

use cozo::{new_cozo_redb, Db, RedbStorage};

pub type FlowstoneDb = Db<RedbStorage>;

pub use flowstone_core::{dangling_count, link_count, note_count};

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
