mod database;
mod parser;
mod pipeline;
mod repl;
mod scanner;
mod server;
mod watcher;

use std::path::PathBuf;
use std::process;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    if args[1] == "serve" {
        run_serve(&args);
    } else {
        run_repl(&args);
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  flowstone <notes-directory> [--db <path>]");
    eprintln!("      Start REPL");
    eprintln!("  flowstone serve <notes-directory> [--port N] [--db <path>]");
    eprintln!("      Start web server (default port 3030)");
}

fn run_repl(args: &[String]) {
    let notes_dir = validate_dir(&args[1]);
    let db_path = find_db_path(args, &notes_dir);

    let db = database::open(&db_path);
    let stats = pipeline::load(&db, &notes_dir);
    println!("Loading {} notes...", stats.notes);
    println!("Extracted {} links", stats.links);
    println!("Database ready.\n");

    repl::run(&db, &notes_dir);
}

fn run_serve(args: &[String]) {
    if args.len() < 3 {
        eprintln!("Usage: flowstone serve <notes-directory> [--port N] [--db <path>]");
        process::exit(1);
    }

    let notes_dir = validate_dir(&args[2]);
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|p| args.get(p + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(3030);

    let db_path = find_db_path(args, &notes_dir);
    let db = database::open(&db_path);
    let stats = pipeline::load(&db, &notes_dir);
    println!("Loading {} notes...", stats.notes);
    println!("Extracted {} links", stats.links);
    println!("Database ready.");

    let db = Arc::new(db);
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    if let Err(e) = rt.block_on(server::run(db, notes_dir, port)) {
        eprintln!("Server error: {}", e);
        process::exit(1);
    }
}

fn validate_dir(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if !p.is_dir() {
        eprintln!("Error: '{}' is not a directory", p.display());
        process::exit(1);
    }
    p
}

fn find_db_path(args: &[String], notes_dir: &PathBuf) -> PathBuf {
    if let Some(pos) = args.iter().position(|a| a == "--db") {
        args.get(pos + 1).map(PathBuf::from).unwrap_or_else(|| {
            eprintln!("Error: --db requires a path");
            process::exit(1);
        })
    } else {
        notes_dir.join(".flowstone.db")
    }
}
