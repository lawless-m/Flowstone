use cozo::DbInstance;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::Path;

use crate::database;
use crate::parser;
use crate::scanner;

pub fn run(db: &DbInstance, notes_dir: &Path) {
    let history_path = history_path();
    let mut rl = DefaultEditor::new().expect("Failed to create editor");
    let _ = rl.load_history(&history_path);

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            "flowstone> "
        } else {
            "...        "
        };
        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                // Blank line: submit the buffered program, if any.
                if trimmed.is_empty() {
                    submit(db, &mut buffer, &mut rl);
                    continue;
                }

                // :quit from any state: submit buffered program, then exit.
                if trimmed == ":quit" || trimmed == ":q" {
                    submit(db, &mut buffer, &mut rl);
                    break;
                }

                // Other built-ins are only valid when the buffer is empty —
                // otherwise they get treated as lines of the query-in-progress.
                if buffer.is_empty() {
                    match trimmed {
                        ":help" => {
                            print_help();
                            continue;
                        }
                        ":stats" => {
                            print_stats(db);
                            continue;
                        }
                        ":reload" => {
                            reload(db, notes_dir);
                            continue;
                        }
                        _ => {}
                    }
                }

                // Accumulate this line into the buffered program.
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: discard any partial buffer and re-prompt.
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => {
                submit(db, &mut buffer, &mut rl);
                break;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
}

fn submit(db: &DbInstance, buffer: &mut String, rl: &mut DefaultEditor) {
    let trimmed = buffer.trim();
    if !trimmed.is_empty() {
        let program = trimmed.to_string();
        let _ = rl.add_history_entry(program.as_str());
        run_query(db, &program);
    }
    buffer.clear();
}

fn history_path() -> String {
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let dir = std::path::PathBuf::from(home).join(".flowstone");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("history.txt").to_string_lossy().into_owned()
    } else {
        ".flowstone_history".into()
    }
}

fn run_query(db: &DbInstance, query: &str) {
    match db.run_default(query) {
        Ok(result) => print_table(&result.headers, &result.rows),
        Err(e) => eprintln!("Error: {}", e),
    }
}

fn print_table(headers: &[String], rows: &[Vec<cozo::DataValue>]) {
    if headers.is_empty() {
        println!("OK");
        return;
    }

    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

    let formatted: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, val)| {
                    let s = format!("{}", val);
                    if i < cols {
                        widths[i] = widths[i].max(s.len());
                    }
                    s
                })
                .collect()
        })
        .collect();

    // Header
    print_row(headers, &widths);
    // Separator
    let sep: Vec<String> = widths.iter().map(|&w| "-".repeat(w)).collect();
    let sep_refs: Vec<&str> = sep.iter().map(|s| s.as_str()).collect();
    print_row(&sep_refs, &widths);
    // Rows
    for row in &formatted {
        let refs: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
        print_row(&refs, &widths);
    }

    println!("{} row(s)", rows.len());
}

fn print_row(cells: &[impl AsRef<str>], widths: &[usize]) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            print!(" | ");
        }
        let w = widths.get(i).copied().unwrap_or(0);
        print!("{:<width$}", cell.as_ref(), width = w);
    }
    println!();
}

fn print_stats(db: &DbInstance) {
    let notes = database::note_count(db);
    let links = database::link_count(db);
    let dangling = database::dangling_count(db);
    println!("Notes:    {}", notes);
    println!("Links:    {}", links);
    println!("Dangling: {}", dangling);
}

fn reload(db: &DbInstance, notes_dir: &Path) {
    println!("Reloading...");
    let notes = scanner::scan(notes_dir);
    let mut all_links = Vec::new();
    for note in &notes {
        all_links.extend(parser::parse_links(&note.path, &note.abs_path));
    }

    database::create_schema(db);
    database::load_notes(db, &notes);
    database::load_links(db, &all_links);

    println!(
        "Loaded {} notes, {} links.",
        database::note_count(db),
        database::link_count(db),
    );
}

fn print_help() {
    println!(
        "\
Commands:
  :quit, :q   Exit (submits any pending multi-line program first)
  :reload     Rescan files and rebuild database
  :stats      Show note/link/dangling counts
  :help       This message

Multi-line queries: type the program across several lines, then press
Enter on a blank line to submit it. This is required for programs with
:order and :limit directives.

Example queries:

  List all notes:
    ?[path, title] := *notes{{path, title}}

  Links from a note:
    ?[target] := *links[\"my note\", target]

  Backlinks to a note:
    ?[source] := *links[source, \"my note\"]

  Orphan notes:
    has_links[n] := *links[n, _]
    has_links[n] := *links[_, n]
    ?[path, title] := *notes{{path, title}}, not has_links[path]

  Most linked-to notes:
    ?[target, count(source)] := *links[source, target]
    :order -count(source)
    :limit 20

  Dangling links:
    ?[target, count(source)] := *links[source, target], not *notes{{path: target}}
    :order -count(source)

  Reachable from a note:
    reachable[note] := *links[\"starting note\", note]
    reachable[note] := reachable[mid], *links[mid, note]
    ?[found] := reachable[found]

  PageRank:
    ?[note, rank] <~ PageRank(*links[])
    :order -rank
    :limit 20"
    );
}
