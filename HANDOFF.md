# Handoff

Snapshot for whoever picks this up next (human or agent). Last updated 2026-06-01.

## Where things stand

`master` has, in order: archive browsing + a production-readiness pass (#22), and borg non-interactive hardening (#24). The cross-platform backup **engine** is well tested; the **Windows GUI layer is still not validated on real hardware**.

- **Verified (Linux CI + local):** `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (191 tests), `svelte-check` (0/0), production frontend build.
- **Verified end-to-end against a real borg binary (Linux):** init → create → list → browse → extract → byte-for-byte verify, including encrypted repos, selective restore, special-character filenames, and the locked-file warning path (`crates/borg-core/tests/e2e_backup_restore.rs`).
- **Verified on real Windows (manual):** the Rust workspace compiles, and manual `borg.exe 1.4.4+win6` operations (init an encrypted repo, version) work with the hardening env vars in place. See the headless-testing section below.
- **NOT verified:** the actual Tauri app on Windows — the app window, VSS (intentionally disabled), Task Scheduler registration, the OS keychain, the cancel flow through Tauri, and a full automated backup→restore on Windows.

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

## borg must be fully non-interactive (#24)

`borg.rs::base_command_with` sets these so borg **never blocks on a prompt the GUI can't answer** (each one is a real hang if omitted):

- `BORG_PASSPHRASE` always set (empty when none) — no passphrase prompt.
- `BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK=yes` — no "unknown unencrypted repo? [y/N]" prompt.
- `BORG_DISPLAY_PASSPHRASE=no` — no "display passphrase for verification? [y/N]" prompt on `init`.
- `BORG_RELOCATED_REPO_ACCESS_IS_OK=yes`, and stdin closed as a backstop.

Don't remove these — they were found the hard way on Windows. Vorta (the macOS/Linux borg GUI) sets the same vars for the same reason.

## Windows headless testing (`tests/smoke-windows/`)

There is a working KVM-backed Windows VM harness (Docker/`dockurr/windows`, tiny11). On a host with `/dev/kvm` it boots Windows, installs Rust + MinGW + `borg.exe`, deploys the source, and runs the test suite. It now also installs `borg.exe 1.4.4-win6` and has a real-borg e2e step. Run it from `tests/smoke-windows/` with `./run.sh all` (or `make smoke`); `KEEP_VM=1` keeps it warm; `./run.sh down` tears it down. The `.bin/docker` shim is gitignored and auto-detects distrobox.

**Known limitation (important):** the automated e2e could **not** be driven to green headless. `borg.exe` is a PyInstaller bundle; when spawned by the Rust test binary (via tokio, several levels deep under an SSH session with no console), it **hangs at spawn** — even with all the non-interactive env vars, stdin closed, and `CREATE_NO_WINDOW`. The *same* `borg.exe` works when launched directly from PowerShell (which has a console). This is a **test-environment artifact** (console-less spawn under SSH), not a product defect — the real Tauri GUI has a window station. Two practical consequences:
- The e2e suite validates the engine on Linux; on Windows, run it under a session that has a real console, or validate via the actual app.
- The `borg-core` unit tests also hang under this harness because the `ssh::` tests spawn real `ssh.exe`/`ssh-keygen.exe` that prompt; skip them (`-- --skip ssh::`) if running unit tests headless.

## Do this before first production use

One live round-trip on the target Windows machine **with the actual app**: configure a repo, run a backup, restore to a temp folder, and diff against the source. That exercises the GUI / Task-Scheduler / keychain paths a Linux box can't, and confirms borg spawns cleanly from the GUI. Also smoke the Settings page: switch profiles and confirm Schedule/Retention/archive-template fields repopulate.

## Open items

- **`CREATE_NO_WINDOW` on spawned processes (Windows)** — DONE in `crates/borg-core/src/proc.rs`: `proc::command()` builds a `std::process::Command`, sets `CREATE_NO_WINDOW` under `#[cfg(windows)]`, then converts to `tokio::process::Command`. All borg (`borg.rs::base_command_with`) and ssh (`ssh.rs`) spawns route through it; no-op on non-Windows. Suppresses the cosmetic console-window flash. **Still needs a visual confirm in the real Windows GUI** — it compiles cross-platform and Linux e2e proves spawning is unaffected, but the window-suppression itself can't be verified headless. (It is unrelated to the headless-e2e spawn hang, which has a different root cause.)
- **#23** — stream + virtualize archive contents for very large archives (100k+ entries). Perf only.
- **VSS** — disabled because shadow-copy paths (`\\?\GLOBALROOT\...`) are unrestorable on Windows. Plan: `.claude/PRPs/plans/fix-vss-paths-in-archive.plan.md`. Live-file backup is the current safe posture (locked files warn, not fail).
- TODO.md Phase 3 leftovers: archive diff, pre/post commands, autostart, repo compaction.

## Gotchas worth knowing

- **borg prompts hang headless.** Anything that makes borg ask a question (unknown/unencrypted repo, missing/wrong passphrase, init verification) blocks forever with no TTY. The env vars above prevent it; keep them.
- `borg extract --progress --log-json` emits only `progress_percent` events (never `archive_progress`/`nfiles`), unlike `create`. Don't derive restore success or file counts from `archive_progress` — trust the process exit code.
- On Windows, `cargo`'s sparse-index network refresh can stall in a constrained VM; build with `CARGO_NET_OFFLINE=true` once deps are cached. A network-stalled `cargo` can become unkillable and hold the package-cache lock — reboot the VM to clear it.
