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

**Runtime validation pass (`validate.ps1`, headless-safe).** Because the Rust-spawned borg e2e hangs, `tests/smoke-windows/validate.ps1` (run via `./run.sh validate` or `make validate-all`) drives the real native tools **directly from PowerShell**, which works headless: a full borg.exe backup→restore round-trip (unencrypted + encrypted, byte-verified, with a wrong-passphrase rejection check), the autostart `reg HKCU\…\Run` add/query/delete round-trip, and the `schtasks` create/query/delete round-trip. It needs only a booted VM (no Rust toolchain/source). The hanging `cargo test` e2e step in `smoke-test.ps1` is now a clear SKIP that points here. What still needs eyes on a real desktop (can't be asserted headlessly): the Tauri window/tray actually rendering, `--minimized` landing in the tray, a scheduled task firing the headless run, the console-flash being gone, and the keychain writing to Windows Credential Manager.

## Do this before first production use

One live round-trip on the target Windows machine **with the actual app**: configure a repo, run a backup, restore to a temp folder, and diff against the source. That exercises the GUI / Task-Scheduler / keychain paths a Linux box can't, and confirms borg spawns cleanly from the GUI. Also smoke the Settings page: switch profiles and confirm Schedule/Retention/archive-template fields repopulate.

## Open items

- **`CREATE_NO_WINDOW` on spawned processes (Windows)** — DONE in `crates/borg-core/src/proc.rs`: `proc::command()` builds a `std::process::Command`, sets `CREATE_NO_WINDOW` under `#[cfg(windows)]`, then converts to `tokio::process::Command`. All borg (`borg.rs::base_command_with`) and ssh (`ssh.rs`) spawns route through it; no-op on non-Windows. Suppresses the cosmetic console-window flash. **Still needs a visual confirm in the real Windows GUI** — it compiles cross-platform and Linux e2e proves spawning is unaffected, but the window-suppression itself can't be verified headless. (It is unrelated to the headless-e2e spawn hang, which has a different root cause.)
- **#23** — stream + virtualize archive contents for very large archives (100k+ entries). Perf only.
- **VSS** — disabled because shadow-copy paths (`\\?\GLOBALROOT\...`) are unrestorable on Windows. Plan: `.claude/PRPs/plans/fix-vss-paths-in-archive.plan.md`. Live-file backup is the current safe posture (locked files warn, not fail).
- TODO.md Phase 3 is **complete** — archive diff, repository compaction, pre/post backup commands, and autostart at login are all done. Autostart (`borg-platform-win::autostart`) shells out to `reg` to manage the `HKCU\...\Run` value `"<exe>" --minimized`; the `--minimized` flag (handled in `lib.rs`) starts BorgUI hidden in the tray. **Needs a real-Windows confirm**: that the `reg` add/delete/query round-trip works and that a `--minimized` login launch actually starts in the tray (the validation/command-construction logic is unit-tested on Linux, but `reg` is absent there so the registry I/O itself is unverified).
- **Scheduled backups now run headlessly.** The Task Scheduler entry launches `<exe> --scheduled-backup`; `lib.rs` detects the flag and (instead of showing the GUI) runs `scheduled::run_scheduled_backup` — one backup from the active profile's *schedule* config (its own source paths + excludes), with the profile's pre/post hooks and retention prune, recording the outcome to history and showing a desktop notification, then exiting 0/1. The runner core (`scheduled.rs`) is Tauri-free and tested against real borg via `BORG_TEST_BIN`. **Still needs a real-Windows confirm** that the scheduled task actually launches the headless run end-to-end (window stays hidden, exit code surfaces in Task Scheduler) — the engine is Linux-verified but the GUI/Task-Scheduler wiring is not.

## Gotchas worth knowing

- **borg prompts hang headless.** Anything that makes borg ask a question (unknown/unencrypted repo, missing/wrong passphrase, init verification) blocks forever with no TTY. The env vars above prevent it; keep them.
- `borg extract --progress --log-json` emits only `progress_percent` events (never `archive_progress`/`nfiles`), unlike `create`. Don't derive restore success or file counts from `archive_progress` — trust the process exit code.
- On Windows, `cargo`'s sparse-index network refresh can stall in a constrained VM; build with `CARGO_NET_OFFLINE=true` once deps are cached. A network-stalled `cargo` can become unkillable and hold the package-cache lock — reboot the VM to clear it.
