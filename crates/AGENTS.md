<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# crates

## Purpose
Rust library crates that contain all backend logic. Separated into a portable core and a Windows-specific platform layer so the core can be reused on other platforms in the future.

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `borg-core/` | Portable library: borg CLI wrapper, config, SSH, progress parsing (see `borg-core/AGENTS.md`) |
| `borg-platform-win/` | Windows-specific: VSS snapshots, Task Scheduler integration (see `borg-platform-win/AGENTS.md`) |

## For AI Agents

### Working In This Directory
- Each crate has its own `Cargo.toml` — they share workspace dependencies from the root `Cargo.toml`.
- `borg-core` is platform-agnostic. Never add Windows-specific code there.
- `borg-platform-win` depends on `borg-core` for error types and owns Windows-only integrations such as VSS, Task Scheduler, and autostart.

### Dependency Direction
```
borg-platform-win → borg-core
app-tauri/src-tauri → borg-core + borg-platform-win
```

<!-- MANUAL: -->
