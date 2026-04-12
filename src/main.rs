mod database;
mod parser;
mod repl;
mod scanner;

use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: flowstone <notes-directory> [--db <path>]");
        process::exit(1);
    }

    let notes_dir = PathBuf::from(&args[1]);
    if !notes_dir.is_dir() {
        eprintln!("Error: '{}' is not a directory", notes_dir.display());
        process::exit(1);
    }

    let db_path = if let Some(pos) = args.iter().position(|a| a == "--db") {
        args.get(pos + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                eprintln!("Error: --db requires a path");
                process::exit(1);
            })
    } else {
        notes_dir.join(".flowstone.db")
    };

    // Scan
    let notes = scanner::scan(&notes_dir);
    println!("Loading {} notes...", notes.len());

    // Parse links
    let mut all_links = Vec::new();
    for note in &notes {
        all_links.extend(parser::parse_links(&note.path, &note.abs_path));
    }
    println!("Extracted {} links", all_links.len());

    // Database
    let db = database::open(&db_path);
    database::create_schema(&db);
    database::load_notes(&db, &notes);
    database::load_links(&db, &all_links);
    println!("Database ready.\n");

    // REPL
    repl::run(&db, &notes_dir);
}
