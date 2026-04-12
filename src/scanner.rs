use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Note {
    pub path: String,
    pub title: String,
    pub abs_path: PathBuf,
}

pub fn scan(root: &Path) -> Vec<Note> {
    let mut notes = Vec::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            // The root entry itself is never dotfile-filtered — otherwise
            // passing `.` as the notes directory would prune the whole tree.
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let file_path = entry.path();
        if file_path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let rel = file_path.strip_prefix(root).unwrap_or(file_path);
        let path = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let title = file_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        notes.push(Note {
            path,
            title,
            abs_path: file_path.to_path_buf(),
        });
    }

    notes
}
