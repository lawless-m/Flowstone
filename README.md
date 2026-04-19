# Flowstone

A small Rust tool that turns a folder of Markdown notes into a queryable
knowledge graph. Point it at your notes, and it parses every `[[wiki-link]]`,
loads the lot into [CozoDB](https://github.com/lawless-m/cozo-redb) (our
fork, carrying the redb storage backend), and hands you either a Datalog
REPL or a little web server with a live graph view.

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
- Offers three ways to poke at it:
  - a **REPL** for raw Datalog queries
  - a **web server** with a browser UI, graph visualisation, tag
    sidebar, full-text search (tantivy, via Cozo's FTS), live
    file-watching so edits show up without restarting, and a detail
    panel that renders each note's Markdown body with clickable
    wiki-links for in-app navigation.
  - an **in-browser wasm build** (`flowstone-wasm/`) that runs the
    whole pipeline client-side — point it at a zip of Markdown and it
    builds the graph in the browser with no server. Live at
    <https://steponnopets.net/flowstone/>.

The browser UI has three fixed view tabs — the force-directed **Net**,
a word-frequency **W-Cloud**, and a **Tags** co-occurrence graph — plus
one dynamically-added tab per YAML nodes-document in the corpus, each
rendering its own typed directed graph (useful for infra diagrams,
data-flow sketches, and the like — see
[YAML directed-graph](#yaml-directed-graph) below). The wasm build can
also round-trip edits back to GitHub via the Contents API, for corpora
served out of a repo.

The Markdown files are always the source of truth. Flowstone itself
never writes to disk — the GitHub round-trip is the user's own commit,
on their own credentials.

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

## YAML directed-graph

The browser grows one tab per YAML **nodes-document** it finds in the
corpus. Each tab renders that document's nodes and edges as its own
typed directed graph; nothing crosses between tabs, so a corpus can
carry any number of unrelated diagrams without them colliding.

Two kinds of YAML file are recognised, told apart by their contents
(any `.yaml` / `.yml` file will do — the filename is used only as an
id):

- A **schema** has `edges:` or `nodes:` at the top level. It declares
  the vocabulary: which edge kinds exist, which node kinds exist, and
  optional render hints (colour per edge, shape per node).
- A **nodes-document** has `schema:` at the top level naming the
  schema it obeys (by filename stem). Every other top-level key is a
  node id whose body carries optional `kind:`, attribute keys, and
  edge keys (anything matching an edge name in the schema) pointing at
  other node ids in the same document.

Example schema (`infra.yaml`):

```yaml
edges:
  hosts:  { from: machine, to: service, directed: true }
  reads:  { from: task, to: endpoint, colour: "#4a9eff", directed: true }
  writes: { from: task, to: table,    colour: "#e94560", directed: true }
nodes:
  machine:  { shape: box }
  service:  { shape: hexagon }
  table:    { shape: cylinder }
  endpoint: { shape: ellipse }
  task:     { shape: diamond }
```

Example nodes-document (`infrastack.yaml`) — one file describing the
whole relationship, each top-level key is a node:

```yaml
schema: infra

x3customerpull:
  kind: task
  cadence: hourly
  owner: ops
  reads:
    - x3-customer-api
  writes:
    - x3rocs-customer

rivsprod02:
  kind: machine
  fqdn: rivsprod02.local
  hosts:
    - postgresql

postgresql:
  kind: service

x3rocs-customer:
  kind: table
  ddl_ref: schemas/x3rocs/customer.sql
```

Edge targets that aren't declared as nodes in the same document still
appear on the graph as dashed placeholder nodes — often the quickest
way to notice that a design sketch has a hole in it. A node can also
have a sibling Markdown file of the same stem (e.g. `x3customerpull.md`)
— the detail pane will load it automatically, so the DDL or design doc
travels with the node.

Each nodes-document is scoped to itself: edges never cross document
boundaries, so the same node id can mean different things in different
diagrams without confusion.

## Repo layout

```
flowstone-core/ Library crate: parser, schema, bulk loaders, counts,
                YAML schema/node parsers and cozo population
src/
  main.rs       CLI entry point — dispatches to REPL or server
  scanner.rs    Walks the notes directory
  pipeline.rs   Drives flowstone-core to (re)load the database
  database.rs   Cozo open/init, thin wrappers over flowstone-core
  repl.rs       Datalog REPL (rustyline)
  server.rs     Axum web server + JSON API
  watcher.rs    notify-based filesystem watcher
static/         Browser UI: index.html, graph.js, yaml-graph.js, style.css
flowstone-wasm/ In-browser build — crate, JS shim, GitHub save helper,
                build.sh (standard + FTS bundles), deploy.sh
flowstone-spec/ Design notes and schema reference
prompts/        Prompts for optional LLM-assisted tagging
```

The workspace is a Cargo workspace: `flowstone-core` is a library that
any other tool can depend on to ingest a notes directory into a Cozo
database, and the `flowstone` binary is the CLI and web server built
on top of it. `flowstone-wasm` is a third crate in the workspace that
reuses the same core to run the pipeline in a browser.

## Status

Early, useful, and honest about it. The prototype scope in
`flowstone-spec/PROJECT.md` describes what was originally in and out of
scope; some of the "out of scope" items (file watching, tags, a web UI)
have since earned their keep and been added. The rest are still on the
"maybe, once we've used it more" list.

## Licence

See [`LICENSE`](LICENSE).
