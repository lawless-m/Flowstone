# IMPLEMENTATION.md — How to Build Flowstone

## Project Setup

Standard Rust binary project:

```
flowstone/
├── Cargo.toml
├── src/
│   ├── main.rs          — Entry point, CLI argument handling
│   ├── scanner.rs       — Find Markdown files in a directory
│   ├── parser.rs        — Extract [[wiki-links]] from Markdown content
│   ├── database.rs      — CozoDB setup, schema creation, data loading
│   └── repl.rs          — Interactive Datalog REPL
└── README.md
```

Keep it flat. No nested modules, no lib.rs split, no over-engineering. Five files.

## Build Order

### Step 1: Scanner

Walk a directory recursively, collect all `.md` files. Return a list of file paths.

- Use `walkdir` crate or `std::fs::read_dir` recursively
- Skip hidden files and directories (starting with `.`)
- Skip any `.git` directory
- File identity is its path relative to the root directory, without the `.md` extension
  - e.g., `/path/to/notes/projects/flowstone.md` → `"projects/flowstone"`

### Step 2: Parser

Read each Markdown file, extract all `[[wiki-links]]`.

- Regex: `\[\[([^\]]+)\]\]`
- For each match, capture the inner text as the link target
- Normalise: trim whitespace, lowercase for matching purposes (but preserve original case for display)
- Return a list of `(source_note, target_note)` pairs
- A note that links to itself is valid (self-link) — store it, don't filter it
- Dangling links (target doesn't exist as a file) are valid — store them, flag them in the schema

### Step 3: Database

Open CozoDB, create the schema, load data.

#### Opening the database

```rust
// Use redb if available, otherwise SQLite
let db = DbInstance::new("redb", db_path, Default::default())
    .or_else(|_| DbInstance::new("sqlite", db_path, Default::default()))
    .expect("Failed to open database");
```

The database file defaults to `.flowstone.db` in the notes directory. Configurable via CLI flag.

#### Schema creation

Run CozoScript to create the stored relations. See SCHEMA.md for the full schema.

#### Data loading

1. Drop and recreate relations on each run (full rebuild, no incremental)
2. Insert all notes
3. Insert all links
4. This should be fast — even thousands of notes is trivial for CozoDB

### Step 4: REPL

A simple loop: read a line, send it to CozoDB as a query, print the results, repeat.

- Use `rustyline` or `linefeed` for readline support (history, line editing)
- Print results as a simple ASCII table
- Handle errors gracefully — show the error, don't crash
- Special commands:
  - `:quit` or `:q` — exit
  - `:help` — show a few example queries (see QUERIES.md)
  - `:reload` — rescan files and rebuild database
  - `:stats` — show note count, link count, dangling link count

### Step 5: Main

Wire it all together:

1. Parse CLI arguments (notes directory path, optional db path)
2. Scan for Markdown files
3. Parse links from all files
4. Open database, create schema, load data
5. Print summary (note count, link count)
6. Enter REPL

## Error Handling

- File read errors: warn and skip the file, don't abort
- Parse errors: shouldn't happen with a simple regex, but warn and skip if they do
- Database errors: these are fatal, crash with a clear message
- Query errors in REPL: show the error, continue the REPL

## Performance Expectations

For a typical knowledge base (hundreds to low thousands of notes):

- Scanning files: < 1 second
- Parsing links: < 1 second
- Loading into CozoDB: < 1 second
- Queries: effectively instant

If someone has tens of thousands of notes we might need to think about incremental sync, but that's a future problem.
