# BorgUI handoff

Last updated: 2026-07-03.

## Current state

The v0.3–v0.4 feature baseline ends at `5451a6c`. PRs
[#71](https://github.com/bearyjd/borg-ui/pull/71) through
[#80](https://github.com/bearyjd/borg-ui/pull/80) were implemented sequentially,
passed CI, and were squash-merged. There is no feature branch or PR in flight.

The repository still reports application version `0.2.0`. No v0.3/v0.4 tag or
release was created. Treat versioning, installed upgrade validation, and release
publication as the next phase rather than assuming these merges are released.

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

The Windows harness exists but no VM was running at handoff time. `make status`
showed no active container. Installer/updater baselines were also not supplied.
Therefore the following release-level work is still required:

1. Build signed-updater-capable v0.3/v0.4 installers through Release workflow dry
   runs.
2. Validate a production-shaped v0.2 → v0.3 → v0.4 profile and SQLite upgrade.
3. Run Windows smoke coverage for:
   - installer and installed-app updater
   - scheduler, wake, battery/Wi-Fi, removable destination, and USB behavior
   - VSS manual/scheduled backup and restore drills
   - local plus SSH secondary destinations
   - OneDrive Files On-Demand placeholders
   - reporting delivery fixtures
   - recovery on a clean Windows VM
   - all five templates through backup/restore
4. Fix any smoke findings through normal reviewed PRs.
5. Bump application/package versions, create release notes, tag, and publish only
   after the installed upgrade gate is green.

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
