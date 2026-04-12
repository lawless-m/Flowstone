# DEPENDENCIES.md — Crates and Build Setup

## Cargo.toml Dependencies

### Required

- **cozo** — The database engine. Use with `storage-sqlite` feature initially, switch to `storage-redb` when available.
  ```toml
  cozo = { version = "0.7", features = ["compact", "storage-sqlite"] }
  ```

- **walkdir** — Recursive directory traversal. Small, well-maintained, no unnecessary features.
  ```toml
  walkdir = "2"
  ```

- **regex** — For parsing `[[wiki-links]]`. The standard Rust regex crate.
  ```toml
  regex = "1"
  ```

- **rustyline** — Readline support for the REPL (history, line editing, Ctrl-C handling).
  ```toml
  rustyline = "14"
  ```

### That's It

No web framework, no async runtime, no serialisation library, no CLI argument parser beyond what `std::env::args` provides. If we end up needing `clap` for argument parsing later, add it then. For now, the prototype takes one argument: the path to the notes directory.

## Build

```bash
cargo build --release
```

No special build steps, no C dependencies (assuming SQLite backend initially — CozoDB bundles SQLite). When switching to redb, it's still pure Rust with no external dependencies.

## Target Platforms

- Primary: Debian Linux (home machine)
- Secondary: Windows 11 (work machine)

Both should work out of the box with `cargo build`. No platform-specific code.

## Binary Size

Not a concern for the prototype. If it becomes one later, CozoDB's feature flags allow stripping unused backends and algorithms.

## Minimum Rust Version

Whatever CozoDB requires. Don't set our own MSRV — follow upstream.
