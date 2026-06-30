<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# borg-platform-win

## Purpose
Windows-specific platform integrations: Volume Shadow Copy (VSS) snapshots for consistent backups of locked files, and Windows Task Scheduler integration for automated backup scheduling.

## Key Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Crate manifest — depends on `borg-core` (for error types) and `tokio` |
| `src/lib.rs` | Module declarations for Windows integrations |
| `src/vss.rs` | VSS snapshot planning, junction mounting, release, and fallback behavior |
| `src/scheduler.rs` | Windows Task Scheduler via `schtasks.exe` CLI — backup and integrity-check tasks |
| `src/autostart.rs` | Per-user Windows `Run` key autostart integration |

## For AI Agents

### Working In This Directory
- VSS is implemented with shadow-copy junctions and live-file fallback. Preserve stored-path parity with live backups.
- The scheduler module uses `schtasks.exe` CLI — not the COM API. It supports `Hourly`, `Daily`, and separate monthly metadata integrity-check tasks.
- This crate depends on `borg-core::error` for the shared `BorgError` and `Result` types.
- All functions are async (tokio) to match the rest of the workspace.
- Code here only runs on Windows. Guard with `#[cfg(target_os = "windows")]` if compiling cross-platform.

### Testing Requirements
- `cargo test -p borg-platform-win`
- Scheduler tests need mocking — `schtasks.exe` requires Windows and may need elevated privileges
- VSS path-planning tests run cross-platform; live VSS behavior is covered by Windows smoke scripts.

### Common Patterns
- Async process spawning via `tokio::process::Command`
- Errors mapped to `BorgError::ProcessFailed` with stderr capture

## Dependencies

### Internal
- `borg-core` — error types (`BorgError`, `Result`)

### External
- `tokio` — async process execution
- `tracing` — logging

<!-- MANUAL: -->
