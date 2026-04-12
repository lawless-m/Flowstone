# PROJECT.md — Flowstone

## What It Is

A CLI tool that turns a folder of Markdown files into a queryable knowledge graph, powered by CozoDB and Datalog.

You write notes in any editor you like. Flowstone parses them, extracts `[[wiki-links]]`, loads everything into CozoDB, and gives you a Datalog REPL to explore your knowledge base as a graph.

## What It Is Not

- Not a note-taking app (use your editor)
- Not a Markdown renderer
- Not Obsidian (no GUI, no plugins, no sync)
- Not a database you write to directly (the Markdown files are the source of truth)

## The Name

Obsidian is volcanic glass. Flowstone forms slowly in caves, layer by layer, over time. A knowledge base that accumulates and solidifies.

## Prototype Scope

The smallest useful thing. Nothing more until we've used it and know what's missing.

### In Scope

- Point it at a directory of `.md` files
- Recursively find all Markdown files
- Parse `[[wiki-links]]` from file contents
- Store notes and links as CozoDB relations
- Provide a Datalog REPL for querying
- Rebuild the database from files on each run (database is purely derived, disposable)

### Out of Scope (for now)

- File watching / live sync
- Frontmatter / YAML metadata parsing
- Tags (e.g., #tag)
- Headings as structure
- Section links (`[[note#heading]]`)
- Display text links (`[[note|display text]]`)
- Web UI or TUI
- Vector embeddings / semantic search
- Any kind of write-back to Markdown files
- Daemon mode

### Why This Scope

We don't know what Flowstone needs to be yet. Building the minimum and using it will teach us:

- What queries are actually useful day-to-day
- Whether we need metadata beyond links
- Whether rebuild-on-run is fast enough or we need incremental sync
- Whether a REPL is the right interface or we want something else
- What's missing that we didn't think of

Design decisions made from experience are better than design decisions made from imagination.

## How It Works

```
$ flowstone /path/to/notes
Loading 342 notes...
Extracted 1,247 links
Database ready.

flowstone> ?[target] := *links["my note", target]
┌─────────────────┐
│ target          │
├─────────────────┤
│ CozoDB          │
│ Datalog         │
│ graph databases │
└─────────────────┘

flowstone> 
```

The database file lives alongside the tool (or in a configurable location). It's disposable — delete it and Flowstone rebuilds from the Markdown files next time.

## Technical Stack

- **Language**: Rust
- **Database**: CozoDB (with redb backend, falling back to SQLite if redb isn't available yet)
- **Markdown parsing**: Minimal — we only need to extract `[[links]]`, not render Markdown
- **CLI**: Simple REPL, no framework needed initially

## Future Possibilities (not commitments)

These are things that might make sense after using the prototype:

- Incremental sync (only re-parse changed files)
- Frontmatter parsing for structured metadata
- Tag extraction
- Pre-built queries as named commands (e.g., `backlinks "my note"` instead of raw Datalog)
- Saved queries
- Export graph as DOT/SVG for visualisation
- Vector embeddings for semantic similarity (using the 3090 at work)
- Web UI for graph exploration
- Daemon mode with file watcher
