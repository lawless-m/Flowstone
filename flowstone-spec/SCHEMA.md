# SCHEMA.md — CozoDB Relations

## Stored Relations

### notes

Every Markdown file becomes a note.

```
:create notes {
    path: String      => 
    title: String,
    size: Int,
    modified: Float
}
```

- `path` — relative path without `.md` extension, serves as the primary key. e.g., `"projects/flowstone"`
- `title` — the filename without extension. e.g., `"flowstone"`. (Later we might extract a title from the first heading, but for now filename is fine.)
- `size` — file size in bytes
- `modified` — last modified timestamp as Unix epoch float

### links

Every `[[wiki-link]]` becomes a link.

```
:create links {
    source: String,
    target: String
}
```

- `source` — path of the note containing the link
- `target` — the normalised link target text
- The composite key `(source, target)` means duplicate links within the same note are deduplicated automatically

### dangling

A derived/computed view, not stored — but useful to query.

This isn't a stored relation. Instead, it's expressed as a Datalog query:

```
dangling[target] := *links[_, target], not *notes{path: target}
```

"All link targets that don't have a corresponding note."

## Notes on the Schema

### Why minimal

This is the prototype. We store only what we can extract from file metadata and `[[links]]`. No frontmatter, no tags, no headings, no content.

If we find we need more, we add it. Adding relations is easy. Migrating a complex schema is hard.

### Key normalisation

Link targets need normalisation to match note paths:

- `[[Flowstone]]` should match `flowstone.md`
- `[[projects/Flowstone]]` should match `projects/flowstone.md`

Strategy: normalise both note paths and link targets to lowercase for matching. Store the original case for display.

This might need a separate lookup relation:

```
:create note_lookup {
    normalised: String =>
    path: String
}
```

Where `normalised` is the lowercased path. Links are resolved by looking up the normalised target in this relation. This is a detail for implementation — the exact approach depends on how CozoDB handles case-insensitive matching.

### No content storage

We deliberately don't store the full text of notes in CozoDB. The Markdown files are the source of truth. CozoDB holds the graph structure only.

If we later want full-text search, CozoDB has FTS built in and we can add a `content` field. But for the prototype, keep it lean.
