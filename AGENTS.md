<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# BorgUI

## Purpose
Native Windows GUI for BorgBackup. Lets users back up to a remote borg server over SSH or to a local folder/USB/network share without requiring WSL. Built as a Tauri 2 desktop app with a Svelte 5 frontend and a Rust backend organized as a Cargo workspace.

## Key Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Workspace root — defines members: `borg-core`, `borg-platform-win`, `app-tauri/src-tauri` |
| `Cargo.lock` | Locked dependency versions for the workspace |
| `README.md` | Project overview, architecture, setup instructions |
| `app-icon.png` | Application icon source |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `crates/` | Rust library crates (see `crates/AGENTS.md`) |
| `app-tauri/` | Tauri 2 desktop application — frontend + backend (see `app-tauri/AGENTS.md`) |

## For AI Agents

### Working In This Directory
- This is a Cargo workspace. Run `cargo build`, `cargo test`, `cargo clippy` from the root.
- Frontend dev server: `cd app-tauri && pnpm tauri dev`
- The workspace uses Rust edition 2024 — use current Rust idioms.
- For local Windows builds, stage the whole Borg-for-Windows onedir bundle at `app-tauri/src-tauri/binaries/borg/` (`borg.exe` plus `_internal/`). The release workflow does this automatically.

### Architecture Overview
```
Frontend (Svelte 5 / SvelteKit)
        ↕ Tauri IPC (invoke)
Tauri Commands (app-tauri/src-tauri/src/commands.rs)
        ↕ Rust function calls
borg-core          borg-platform-win
(portable)         (Windows-specific)
        ↕                  ↕
   borg CLI           schtasks / VSS / registry
```

### Testing Requirements
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `pnpm check` and `pnpm build` in `app-tauri/`
- `git diff --check`

### Shipped Scope
1. SSH and local/USB/network-share repositories
2. VSS-backed manual and scheduled backups with cancellation, retries, history, and diagnostics
3. Restore, selective restore, archive browse/diff/delete/compact, and cancellable large archive listing
4. Profiles, retention, scheduling, integrity checks, encrypted recovery-key export/import, updater, and release pipeline

## Dependencies

### External
- Tauri 2.x — desktop app framework
- Svelte 5.x / SvelteKit 2.x — frontend
- serde / serde_json — serialization
- tokio — async runtime
- thiserror — typed errors
- tracing — structured logging

<!-- MANUAL: -->
