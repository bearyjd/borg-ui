<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# borg-core

## Purpose
Portable Rust library that wraps the borg CLI. Provides configuration types, SSH utilities, progress event parsing, and archive listing. Platform-agnostic — no Windows or Linux specific code.

## Key Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Crate manifest — depends on serde, tokio, thiserror, tracing |
| `src/lib.rs` | Module declarations — re-exports all submodules |
| `src/borg.rs` | `BorgClient` — async wrapper around the borg CLI binary (version, info, create, list) |
| `src/config.rs` | `RepoConfig`, `BackupProfile`, `AppConfig`, `Compression` — serializable config types |
| `src/error.rs` | `BorgError` enum (thiserror) and `Result<T>` alias |
| `src/ssh.rs` | SSH helpers: `test_connection`, `generate_key`, `read_public_key` |
| `src/archive.rs` | `ArchiveEntry` type and `list_archive_contents` function |
| `src/progress.rs` | `ProgressEvent` enum — deserializes borg's JSON progress output |

## For AI Agents

### Working In This Directory
- This crate must remain platform-agnostic. Windows-specific logic belongs in `borg-platform-win`.
- `BorgClient` spawns borg as a child process and parses JSON output from stdout/stderr.
- All async functions use `tokio::process::Command` — the workspace uses `tokio` with full features.
- Error types use `thiserror` — add new variants to `BorgError` for new failure modes.
- Config types derive `Serialize`/`Deserialize` — they cross the Tauri IPC boundary as JSON.

### Testing Requirements
- Unit tests in `#[cfg(test)]` modules within each source file
- Integration tests require a borg binary — mock the process for unit tests
- `cargo test -p borg-core`

### Common Patterns
- Builder pattern on `BorgClient` (`new` + `with_passcommand`)
- All borg commands return `Result<T>` with `BorgError`
- Progress is streamed via callback: `impl Fn(ProgressEvent) + Send + 'static`

## Dependencies

### Internal
- None (leaf crate)

### External
- `serde` + `serde_json` — config serialization and borg JSON parsing
- `tokio` — async process spawning and I/O
- `thiserror` — typed error definitions
- `tracing` — structured debug logging

<!-- MANUAL: -->
