# Flowstone

A small Rust tool that turns a folder of Markdown notes into a queryable
knowledge graph. Point it at your notes, and it parses every `[[wiki-link]]`,
loads the lot into [CozoDB](https://www.cozodb.org/), and hands you either a
Datalog REPL or a little web server with a live graph view.

The name is deliberate: Obsidian is volcanic glass — cooled in a flash.
Flowstone forms slowly in caves, layer by layer. A knowledge base that
accumulates and hardens over time.

## What it does

- Walks a directory and reads every `.md` file it finds.
- Extracts `[[wiki-links]]`, `#tags`, and note content.
- Stores notes, links, and tags as CozoDB relations so you can ask
  graph-shaped questions in Datalog.
- Rebuilds from the Markdown on every run — the database is purely
  derived, so delete it whenever you like.
- Offers two ways to poke at it:
  - a **REPL** for raw Datalog queries
  - a **web server** with a browser UI, graph visualisation, tag
    sidebar, full-text search (tantivy, via Cozo's FTS), and live
    file-watching so edits show up without restarting.

The Markdown files are always the source of truth. Flowstone never writes
back to them.

## Building

Flowstone depends on a local path to Matt's fork of Cozo, so the repo
layout is not quite standalone:

```
parent/
├── Flowstone/      <-- this repo
└── cozordb/cozo/   <-- sibling checkout of the cozo fork
```

It also routes `graph_builder` through a patched fork for rayon 1.11
compatibility — see the comment at the top of `Cargo.toml` for the gory
details. Once both are in place:

```sh
cargo build --release
```

## Usage

```sh
# Start the REPL
flowstone /path/to/notes

# Start the web server (default port 3030)
flowstone serve /path/to/notes --port 3030

# Optional: put the database somewhere other than <notes>/.flowstone.db
flowstone serve /path/to/notes --db /tmp/flowstone.db
```

### REPL example

```
$ flowstone ~/notes
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
```

For the shape of the relations (`notes`, `links`, `tags`, …) and a
handful of useful queries to get you started, see
[`flowstone-spec/SCHEMA.md`](flowstone-spec/SCHEMA.md) and
[`flowstone-spec/QUERIES.md`](flowstone-spec/QUERIES.md).

### Web server

`flowstone serve` starts an Axum server that hosts the static UI in
`static/` and exposes JSON endpoints for notes, tags, search, and the
graph. A filesystem watcher re-runs the pipeline when notes change, and
the browser is nudged over an event stream so the view stays current.

## Repo layout

```
src/
  main.rs       CLI entry point — dispatches to REPL or server
  scanner.rs    Walks the notes directory
  parser.rs     Extracts wiki-links and tags from Markdown
  pipeline.rs   Loads parsed notes into Cozo
  database.rs   Cozo open/init + schema
  repl.rs       Datalog REPL (rustyline)
  server.rs     Axum web server + JSON API
  watcher.rs    notify-based filesystem watcher
static/         Browser UI: index.html, style.css, graph.js
flowstone-spec/ Design notes and schema reference
prompts/        Prompts for optional LLM-assisted tagging
```

## Status

Early, useful, and honest about it. The prototype scope in
`flowstone-spec/PROJECT.md` describes what was originally in and out of
scope; some of the "out of scope" items (file watching, tags, a web UI)
have since earned their keep and been added. The rest are still on the
"maybe, once we've used it more" list.

## Licence

See [`LICENSE`](LICENSE).
