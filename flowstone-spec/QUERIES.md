# QUERIES.md — Useful Datalog Queries

These should be shown when the user types `:help` in the REPL. They demonstrate what Flowstone can do and serve as a learning tool for Datalog.

## Basic Queries

### List all notes
```
?[path, title] := *notes{path, title}
```

### Find a specific note
```
?[path, title] := *notes{path, title}, path == "projects/flowstone"
```

### All links from a specific note
```
?[target] := *links["projects/flowstone", target]
```

### All backlinks to a specific note (what links TO this note)
```
?[source] := *links[source, "projects/flowstone"]
```

## Graph Queries

### Notes reachable from a starting point (transitive closure)
```
reachable[note] := *links["starting note", note]
reachable[note] := reachable[mid], *links[mid, note]
?[found] := reachable[found]
```

### Shortest path between two notes
```
?[path] <~ ShortestPathBFS(*links[], "note a", "note b")
```

### Mutual links (A links to B AND B links to A)
```
?[a, b] := *links[a, b], *links[b, a], a < b
```

### Notes that link to each other through a common note (two hops)
```
?[a, b, via] := *links[a, via], *links[b, via], a != b, a < b
```

## Discovery Queries

### Orphan notes (no incoming or outgoing links)
```
has_links[n] := *links[n, _]
has_links[n] := *links[_, n]
?[path, title] := *notes{path, title}, not has_links[path]
```

### Most linked-to notes (hubs)
```
?[target, count(source)] := *links[source, target]
:order -count(source)
:limit 20
```

### Most prolific linkers (notes with the most outgoing links)
```
?[source, count(target)] := *links[source, target]
:order -count(target)
:limit 20
```

### Dangling links (links to notes that don't exist yet)
```
?[target, count(source)] := *links[source, target], not *notes{path: target}
:order -count(source)
```

### Notes that only link out, never linked to
```
links_out[n] := *links[n, _]
linked_to[n] := *links[_, n]
?[path] := links_out[path], not linked_to[path]
```

### Notes that are only linked to, never link out
```
links_out[n] := *links[n, _]
linked_to[n] := *links[_, n]
?[path] := linked_to[path], not links_out[path]
```

## Advanced Graph Algorithms

### PageRank — find your most "important" notes
```
?[note, rank] <~ PageRank(*links[])
:order -rank
:limit 20
```

### Community detection — find clusters of related notes
```
?[note, community] <~ CommunityDetectionLouvain(*links[])
:order community
```

### Connected components — find isolated subgraphs
```
?[note, component] <~ ConnectedComponents(*links[])
:order component
```

## Notes

- These queries assume the schema defined in SCHEMA.md
- The graph algorithm syntax (`<~`) is CozoDB-specific Datalog extension
- Results are returned as tables — the REPL should format them readably
- All queries should complete instantly for any reasonable knowledge base size
- These examples are also the basis for testing — if these all work, Flowstone works
