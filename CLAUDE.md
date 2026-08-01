# CLAUDE.md — Agent Operating Notes

This file is procedural: how to work safely and autonomously in this repo. For
structure (what each directory/crate is for, dependency direction, key files),
read `AGENTS.md` and the nested `AGENTS.md` in `crates/`, `crates/borg-core/`,
`crates/borg-platform-win/`, `app-tauri/`, `app-tauri/src-tauri/`, `app-tauri/src/`
first — this file does not repeat that.

## Verified commands

All commands below are confirmed against `Cargo.toml`, `README.md`,
`.github/workflows/ci.yml`, and `app-tauri/package.json` — do not invent new ones.

```bash
# Rust — from repo root
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # matches CI's Windows job; Linux CI job omits --all-targets
cargo test --workspace
cargo test -p borg-core                                  # single crate
cargo test -p borg-ui                                     # Tauri backend

# Frontend — from app-tauri/
pnpm install
pnpm check          # Svelte + TS type check
pnpm test            # vitest suite — CI runs this and will fail without it
pnpm build           # frontend only (CI does NOT run this; the release build does)
pnpm tauri dev        # full app, dev server + Rust backend
pnpm tauri build --no-bundle   # release binary WITHOUT dev-server fallback — see "dev-mode" pitfall below

# Real-borg e2e (skipped without this — see Verification gaps below)
scripts/fetch-borg-linux.sh   # one-time: fetches a pinned, checksum-verified borg into .cache/borg-test-bin/
BORG_TEST_BIN=$(pwd)/.cache/borg-test-bin/borg cargo test -p borg-core --test e2e_backup_restore -- --nocapture

# Windows smoke (needs a KVM-capable host; not available in most agent sandboxes)
make -C tests/smoke-windows validate-all
```

`cargo build` / `cargo test --workspace` can be slow (full Tauri + rusqlite
bundled build) — prefer `cargo check` or a scoped `cargo test -p <crate>` while
iterating, matching the allowlist already in `.claude/settings.local.json`.

## Before trusting a doc claim, verify it against git log

`HANDOFF.md` is a living status file, not a changelog — it can and does lag
behind `master`. Its "Audit (2026-07-04)" section has since been re-verified
(2026-07-07) and now correctly marks the SSH argument-injection RCE and the
cross-profile prune data-loss bug as fixed — but the lesson stands: this file
has gone stale before. **Before acting on any "known issue" or "still open"
claim in `HANDOFF.md` or `TODO.md`, run `git log --oneline -- <the cited file>`
for the referenced path/line range.** If a fix postdates the doc, treat the doc
as stale, not the code.

## The option-injection gate — apply to every new untrusted argv field

`borg` and `ssh` are spawned via direct argv (`tokio::process::Command`), so
shell metacharacter injection is not the risk — a value beginning with `-`
being parsed as a flag is (e.g. `ssh_user = -oProxyCommand=...`). All argv-bound
untrusted strings must pass `reject_option_like` (`crates/borg-core/src/config.rs:25-36`)
before ever reaching a `Command`. `RepoConfig::validate()` already gates
`repo_path`, `ssh_host`, `ssh_user`; `test_ssh_connection` in
`app-tauri/src-tauri/src/commands.rs:142-157` gates the same fields
independently since it can be called before a full `validate()`. If you add any
new user-controlled field that becomes an argv token (a borg binary path
override, an archive name pattern, a new SSH option), route it through this
same gate and add a `..._rejects_option_like_...` test alongside the existing
ones (see `crates/borg-core/src/config.rs` tests and
`app-tauri/src-tauri/src/commands.rs:2332-2342`).

## Security/privacy invariants (never regress these)

Keep out of logs, diagnostics exports, history/config exports, and report
payloads — this list is enforced by convention/review, not by a lint, so check
it manually on any change touching diagnostics, exports, or logging:

- repository and SMTP passphrases
- webhook secrets
- SSH private keys and recovery payloads
- source listings and archive filenames
- temporary restore paths

**One deliberate exception, decided in #101:** a borg *warning* that names the
single file it failed on (`C:\Users\alice\Documents\tax.pdf: Permission denied`)
does reach `borgui.log` and therefore the support bundle, because that is the
diagnostic content the log exists for. The line above still holds for
*listings* — the file-by-file `archive_progress` stream is never logged. What is
scrubbed on that path is the account name (`redaction.rs` rewrites
`C:\Users\<name>` and `/home/<name>`), and the Diagnostics section says plainly
that a bundle can still name individual files. Do not "fix" this by scrubbing
whole paths without replacing the diagnostic value some other way.

Credential Manager (via the `keyring` crate) is the sole authority for
passphrases/secrets — never persist them to `profiles.rs` config or SQLite
(`history.rs`).

## What CI actually proves (don't over-trust green)

- The Linux `rust` CI job only compiles `cfg(not(windows))` code — VSS,
  scheduler, autostart, cloud placeholders, and other `crates/borg-platform-win`
  internals are invisible to it.
- The `rust-windows` CI job compiles, clippies (`--all-targets`, so it includes
  `cfg(windows)` tests), and unit-tests the Windows code — but it stages a
  **placeholder empty `borg.exe`** (see the "Create placeholder borg resource"
  step in `.github/workflows/ci.yml`) and never runs a real backup, VSS
  snapshot, or scheduled task. It proves compilation and unit-test correctness,
  not runtime behavior.
- Real runtime verification of Windows-only behavior (VSS, Task Scheduler,
  installer launch, GUI flows) only happens via `tests/smoke-windows/` against
  a KVM-backed Windows VM — see that directory's `README.md`. This is not
  reachable from most agent sandboxes; treat `cfg(windows)` runtime behavior as
  unverified unless you have that harness available.
- The e2e backup→restore test (`crates/borg-core/tests/e2e_backup_restore.rs`)
  silently **skips** (not fails) unless `BORG_TEST_BIN` is set. A green
  `cargo test --workspace` run without that variable set has not exercised the
  real backup/restore path at all.

## Style

- Rust edition 2024, `thiserror` for typed errors (extend `BorgError`, don't
  introduce new error types per module).
- `borg-core` must stay platform-agnostic — no `cfg(windows)` or Windows API
  calls there; Windows-only logic belongs in `borg-platform-win`.
- Tauri commands (`app-tauri/src-tauri/src/commands.rs`) return `Result<T, String>`
  (Tauri IPC requires string errors) — map `BorgError` with `.map_err(|e| e.to_string())`.
- Frontend: Svelte 5 runes (`$state`, `$props`, `$derived`), not legacy `$:`
  reactive statements or Svelte-4-style stores for component-local state.
- No hardcoded CSS colors — every value is a token in `app-tauri/src/app.css`;
  see `DESIGN.md` for the full token/component system before touching UI.
