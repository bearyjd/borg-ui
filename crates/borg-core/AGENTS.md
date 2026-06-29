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
| `src/borg.rs` | `BorgClient` — async wrapper around the borg CLI binary (version, info, create/check/extract/list/diff/prune/compact/key export-import) |
| `src/config.rs` | `RepoConfig`, `BackupProfile`, `AppConfig`, `Compression` — serializable config types |
| `src/error.rs` | `BorgError` enum (thiserror) and `Result<T>` alias |
| `src/ssh.rs` | SSH helpers: reachability checks, connection tests, key generation/validation, public-key extraction, and failure classification |
| `src/archive.rs` | `ArchiveEntry` type and archive JSON-lines parsing helpers |
| `src/progress.rs` | `ProgressEvent` enum — deserializes borg's JSON progress output |

## For AI Agents

### Working In This Directory
- This crate must remain platform-agnostic. Windows-specific logic belongs in `borg-platform-win`.
- `BorgClient` spawns borg as a child process, parses JSON output from stdout/stderr, and must remain non-interactive.
- Long-running or streamed Borg operations should accept `CancelToken` and kill children promptly on cancellation.
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
- Archive contents are streamed in bounded batches; do not reintroduce whole-archive IPC payloads for large listings.

## Dependencies

### Internal
- None (leaf crate)

### External
- `serde` + `serde_json` — config serialization and borg JSON parsing
- `tokio` — async process spawning and I/O
- `thiserror` — typed error definitions
- `tracing` — structured debug logging

<!-- MANUAL: -->
