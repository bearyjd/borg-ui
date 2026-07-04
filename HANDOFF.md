# BorgUI handoff

Last updated: 2026-07-03.

## Current state

The v0.3 feature baseline (PRs [#71](https://github.com/bearyjd/borg-ui/pull/71)
through [#80](https://github.com/bearyjd/borg-ui/pull/80)) is consolidated as
**0.3.0**. `master` is at `3b9add5`. The installed 0.3.0 app now launches and
renders its real UI on a clean Windows machine — but only after **two
release-blocker fixes** found this pass (#85, #86 below), each of which also
shipped in the published **v0.2.0**. Five PRs landed after the feature baseline:

- **#82** (`bc805c1`) — bumped the version 0.2.0 → **0.3.0**, fixed a Windows-only
  compile break in `removable.rs`, and added v0.2→0.3 migration regression tests.
- **#83** (`d00dbfc`) — added a `windows-latest` CI job so `cfg(windows)` code is
  compiled/tested on every PR, plus the `network.rs`/`ssh.rs` fixes it surfaced.
- **#84** (`611ef32`) — handoff docs.
- **#85** (`2b826c2`) — 🔴 **crt-static**: the installed `borg-ui.exe` failed to
  launch on a clean Windows with `0xC0000135` (missing VC++ runtime; no CRT
  bundled). Fixed with `.cargo/config.toml` `+crt-static`. Verified on a
  no-redist VM.
- **#86** (`3b9add5`) — 🔴 **dev-mode**: post-#56 installers shipped a
  `cargo build --release` binary that loads the Vite dev server, so the app
  showed "localhost refused to connect" instead of its UI. Fixed by building the
  release exe with `pnpm tauri build --no-bundle`. Verified on a clean VM
  (real Dashboard renders).

The repository now reports application version **0.3.0**, but this is
**0.3.0-dev**: no `v0.3.0` tag or GitHub release exists (latest tag is `v0.2.0`).
There is no feature branch or PR in flight. Release publication and the
end-to-end updater test are the remaining phase.

**Why the version bump exposed work:** the 0.3.0 Release dry-run was the first
Windows build since #71–#80 merged. Linux CI only compiles the
`cfg(not(windows))` stubs, so a compile break in `removable.rs` (from #73) had
passed every PR and only surfaced at the release gate. The new #83 Windows CI job
closes that structural gap — expect it to be stricter than the Release
`cargo build` (`-D warnings` + `--all-targets`).

## Delivered roadmap

| PR | Merge | Feature |
|---|---|---|
| #71 | `27680a1` | Backup coverage wizard and canonical backup selection |
| #72 | `d279f9a` | Restore search/version preview and sample-restore drills |
| #73 | `b3c2a27` | Resource-aware scheduling, snooze, wake, Wi-Fi, battery, and removable triggers |
| #74 | `d844207` | Append-only repository hardening workflow |
| #75 | `1aca493` | Unified protection health and opt-in reporting |
| #76 | `a740947` | Primary/secondary destinations under one logical backup |
| #77 | `754a757` | Windows cloud-placeholder detection and hydration policy |
| #78 | `e40470c` | Aggregate storage/performance metrics and forecasting |
| #79 | `118aae6` | Recovery-readiness workflow |
| #80 | `5451a6c` | Versioned built-in profile templates |
| #82 | `bc805c1` | 0.3.0 version bump + `removable.rs` Windows compile fix + v0.2→0.3 migration tests |
| #83 | `d00dbfc` | `windows-latest` CI job (+ `network.rs` / `ssh.rs` fixes it surfaced) |
| #84 | `611ef32` | handoff docs |
| #85 | `2b826c2` | 🔴 crt-static — app wouldn't launch on clean Windows (`0xC0000135`) |
| #86 | `3b9add5` | 🔴 tauri-build — app showed dev-server error page instead of its UI |

## Schema endpoints

- Profile schema: **v11**, in `app-tauri/src-tauri/src/profiles.rs`.
- SQLite schema: **v7**, in `app-tauri/src-tauri/src/history.rs`.
- Older profile data migrates to the current schema.
- Future profile and SQLite versions are rejected without overwriting them.
- `BackupSelection` already carries `template_id` and `template_version`.

When changing either schema, preserve atomic migration behavior and future-version
rejection. Do not reuse old version numbers.

## Architecture map

- `crates/borg-core`: portable Borg CLI wrapper, cancellation, validation,
  progress parsing, archive operations, and SSH helpers.
- `crates/borg-platform-win`: VSS, Task Scheduler, autostart, power/WLAN APIs,
  cloud-file detection/hydration, and other Windows integrations.
- `app-tauri/src-tauri`: Tauri IPC, profiles, SQLite history, scheduling,
  reporting, recovery, health, forecasting, templates, and backup orchestration.
- `app-tauri/src`: Svelte 5 UI and stores.
- `tests/smoke-windows`: KVM/Windows installer, updater, GUI, VSS, scheduler, and
  archive smoke harness.

## Security and privacy invariants

Keep these outside logs, diagnostics, history/config exports, and report payloads:

- repository and SMTP passphrases
- webhook secrets
- SSH private keys and recovery payloads
- source listings and archive filenames
- temporary restore paths

Repository metrics are aggregate-only. Restore-search results are streamed and
not persisted. Readiness events store typed outcomes and timestamps, not recovery
file locations or passphrases. Credential Manager remains the authority for
repository passphrases, secondary credentials, webhook URLs, and SMTP passwords.

## Verification completed

Every feature PR passed:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app-tauri && pnpm check && pnpm build
git diff --check
```

The last merged feature gate included 145 `borg-core` tests, 58
`borg-platform-win` tests, and 85 app-backend tests. GitHub Frontend and Rust CI
were green for PR #80. Relevant real-Borg tests passed where supported by the
local test fixture.

## Remaining release gate

Progress this pass (0.3.0), on the KVM Windows VM:

- **Done — version bump:** application/package versions are at 0.3.0 (#82).
- **Done — dry-run installers:** 0.3.0 unsigned MSI+NSIS built via a Release
  `workflow_dispatch` dry run (run `28681637938`; no GitHub release created).
- **Done — installed-installer validation:** `make validate-installer` on the
  0.3.0 build passed **10/10** (bundled-borg layout + engine round-trip, MSI+NSIS).
- **Done — v0.2 → v0.3 upgrade:** migration regression tests cover profile schema
  2 → 11 and SQLite schema 3 → 7 on-disk, data-preserving and idempotent (#82).
- **Done — runtime smoke:** `make validate` (borg backup/restore, reg autostart,
  schtasks) **5/5**.
- **🔴 Found + fixed — two release blockers in the installed app itself** (#85, #86
  above), each of which also shipped in **v0.2.0**. They were invisible until now
  because `validate-installer` only runs the self-contained `borg.exe` and never
  launches `borg-ui.exe`. **FOLLOW-UP: add a `borg-ui.exe` launch smoke-check to
  `validate-installer.ps1`** so this class is caught automatically.
- **Broader GUI matrix (0.3.0, both blockers fixed):** the app launches and renders
  its real Dashboard UI on a clean VM; `validate-archive-smoke` **4/5** (100k
  stream/virtualize/select-all/scroll pass). The `validate-vss` /
  `validate-gui-flows` "no history / no archive" failures are **stale-harness false
  failures**: the scripts poll `%APPDATA%\com.borgui.app\history.json`, but 0.3.0
  records history in **SQLite (`borgui.sqlite3`)**. Proven by direct repro
  (`--scheduled-backup` created a real archive). The backup engine works.
  **FOLLOW-UP: update the smoke scripts to read `borgui.sqlite3`, not
  `history.json`.** New #71–#80 features (wake/battery/Wi-Fi/removable/USB,
  SSH/secondary destinations, OneDrive placeholders, reporting, recovery, 5
  templates) still have no smoke automation.
- **To stage the production exe for GUI smoke** without a 15–30 min VM build:
  install the NSIS, then copy `%LOCALAPPDATA%\BorgUI` to
  `C:\borgui-test\target\release\` (the path the GUI scripts hardcode); drive via
  `/IT` session-1 `schtasks`.

Still required before publishing:

1. **Publish + updater test.** The updater test needs a real published
   `releases/latest` (the app polls a hard-coded
   `github.com/bearyjd/borg-ui/releases/latest/download/latest.json`;
   `validate-updater.ps1` does not mock it). Sequence: tag `v0.3.0` → CI builds a
   `--draft` release → publish → `make validate-updater
   BASELINE_INSTALLER=<0.2.0 -setup.exe> EXPECTED_UPDATE_VERSION=0.3.0`. An
   updater-capable 0.2.0 baseline is available from CI run `28559522011`
   (`borgui-windows-installers-unsigned`).
2. **Broader Windows smoke matrix for 0.3.0** (not re-run this pass): scheduler,
   wake, battery/Wi-Fi, removable destination, USB; VSS manual/scheduled
   backup+restore; local plus SSH secondary destinations; OneDrive Files
   On-Demand placeholders; reporting delivery fixtures; recovery on a clean VM;
   all five templates through backup/restore. Needs a production `tauri build`
   exe for the GUI-driven checks.
3. **Fix any smoke findings** through normal reviewed PRs.
4. **Create release notes, tag `v0.3.0`, and publish** once the above is green.

## Audit (2026-07-04)

A read-only features/UI/security audit of 0.3.0-dev ran this pass. The codebase is
well-built (complete v0.3 feature set, parameterized SQL, secrets in Credential
Manager, correct `unsafe` FFI, signed updater, minimal capabilities). Two findings
are **candidate release blockers** on par with #85/#86:

1. **🔴 SSH argument-injection → RCE.** `SSH_FORBIDDEN` blocks metacharacters but
   NOT a leading `-`, so `ssh_user = -oProxyCommand=calc.exe` reaches `ssh` as an
   executed option. It passes `validate()` and is reachable by **importing a
   malicious profile**; `test_ssh_connection` validates nothing.
   `crates/borg-core/src/config.rs:15-17,61-70`, `ssh.rs:19-28`, `commands.rs:133-144`.
   Fix: reject a leading `-` in host/user; validate in the test path.
2. **🔴 Cross-profile prune data loss.** Auto-prune after every backup passes no
   `--glob-archives` (`crates/borg-core/src/borg.rs:455-496`); two profiles/machines
   sharing a repo prune each other's archives. Fix: scope prune to the archive glob.

Next tier (HIGH): no `--` end-of-options before borg positional paths
(`borg.rs:411-414,446-448`); no passphrase rotation + `set_repo_passphrase` can
desync Credential Manager from the repo (`commands.rs:1757`); generated SSH key not
ACL-restricted on Windows (`ssh.rs:256-266`); no missed-backup catch-up
(`<StartWhenAvailable>` missing, `scheduler.rs:97-108`); undefined `--color-error`
CSS token → a failed integrity check renders gray not red
(`IntegritySection.svelte:135`); no keyboard focus styles anywhere (WCAG).

Full Top-10 + detail in the `borgui-audit-2026-07` memory. **All audit items are
findings only — nothing implemented.**

Useful entry points:

```bash
make -C tests/smoke-windows vm
make -C tests/smoke-windows ssh
make -C tests/smoke-windows validate
make -C tests/smoke-windows validate-vss
make -C tests/smoke-windows validate-vss-manual
make -C tests/smoke-windows validate-gui-flows
make -C tests/smoke-windows validate-installer
make -C tests/smoke-windows validate-updater
```

Read `tests/smoke-windows/README.md` before provisioning. Some targets require a
production Tauri executable, installer directory, or `BASELINE_INSTALLER`.

## Operational gotchas

- Keep Borg prompts disabled in GUI and headless paths.
- Keep the raw Windows drive-letter repository behavior; do not restore the old
  administrative-share rewrite.
- Manual and scheduled backups must use the same `BackupSelection`.
- Placeholder scanning/materialization must finish before VSS creation.
- A multi-destination run must reuse one VSS snapshot and archive name.
- Cancellation stops the active destination and skips remaining destinations.
- Append-only client access must not run compact; prune/delete remain logical.
- Manual backups ignore battery/Wi-Fi skip policy but still honor bandwidth and
  sleep prevention.
- Templates resolve known folders when listed/applied and never silently update
  an existing explicit selection.
- Keep Windows PowerShell smoke scripts ASCII-only for PowerShell 5.1.

## Independent follow-up

Production Authenticode activation remains tracked in issue #64. It must not
weaken or block unsigned development dry runs.
