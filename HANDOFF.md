# Handoff

Snapshot for whoever picks this up next (human or agent). Last updated 2026-06-01.

## Where things stand

`master` (squash commit of #22) has archive browsing + a production-readiness pass. The cross-platform backup engine is well tested; the Windows GUI layer is not yet validated on real hardware.

- **Verified (Linux CI + local):** `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (191 tests), `svelte-check` (0/0), production frontend build.
- **Verified end-to-end against a real borg binary:** init → create → list → browse → extract → byte-for-byte verify, including encrypted repos, selective restore, special-character filenames, and the locked-file warning path (`crates/borg-core/tests/e2e_backup_restore.rs`).
- **NOT verified:** the actual Tauri app on Windows — the app window, VSS (intentionally disabled), Task Scheduler registration, the OS keychain, and the cancel flow through Tauri. Tested borg version was 1.2.8; the app ships 1.4.x.

## Architecture

- `crates/borg-core` — portable Rust: config/validation, the borg CLI wrapper (`borg.rs`), SSH, progress parsing. Repo location is SSH **or** a local path (empty host + user = local).
- `crates/borg-platform-win` — Windows VSS + Task Scheduler.
- `app-tauri/src-tauri` — Tauri commands (`commands.rs`), keychain, profiles, history, archive naming, tray.
- `app-tauri/src` — Svelte 5 frontend (routes + `lib/components`, `lib/stores`).

## Build & run

```bash
cd app-tauri && pnpm install && pnpm tauri dev   # needs borg.exe in src-tauri/binaries/ on Windows
```

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app-tauri && npm run check && npm run build
```

Real-borg end-to-end tests are skipped unless `BORG_TEST_BIN` is set:

```bash
BORG_TEST_BIN=/path/to/borg cargo test -p borg-core --test e2e_backup_restore -- --nocapture
```

CI (`.github/workflows/ci.yml`) does **not** set `BORG_TEST_BIN`, so the e2e suite does not gate CI — run it locally against the bundled 1.4.x `borg.exe` before trusting a release.

## Do this before first production use

One live round-trip on the target Windows machine: configure a repo, run a backup, restore to a temp folder, and diff against the source. That exercises the GUI/Task-Scheduler/keychain paths a Linux box can't. Also smoke the Settings page: switch profiles and confirm the Schedule/Retention/archive-template fields repopulate (those sections were just componentized).

## Open items

- **#23** — stream + virtualize archive contents for very large archives (100k+ entries). Perf only.
- **VSS** — disabled because shadow-copy paths (`\\?\GLOBALROOT\...`) are unrestorable on Windows. Plan: `.claude/PRPs/plans/fix-vss-paths-in-archive.plan.md`. Live-file backup is the current safe posture (locked files warn, not fail).
- TODO.md Phase 3 leftovers: archive diff, pre/post commands, autostart, repo compaction.

## Gotcha worth knowing

`borg extract --progress --log-json` emits only `progress_percent` events (never `archive_progress`/`nfiles`), unlike `create`. Don't derive restore success or file counts from `archive_progress` — trust the process exit code.
