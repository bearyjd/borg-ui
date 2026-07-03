# BorgUI handoff

Last updated: 2026-07-03.

## Current state

The v0.3 feature baseline (PRs [#71](https://github.com/bearyjd/borg-ui/pull/71)
through [#80](https://github.com/bearyjd/borg-ui/pull/80)) is consolidated as
**0.3.0** and validated on Windows. `master` is at `d00dbfc`. Two PRs landed
after the feature baseline:

- **#82** (`bc805c1`) — bumped the application/package version 0.2.0 → **0.3.0**,
  fixed a Windows-only compile break in `removable.rs` (see below), and added
  v0.2→0.3 migration regression tests.
- **#83** (`d00dbfc`) — added a `windows-latest` CI job so `cfg(windows)` code is
  compiled and tested on every PR, plus the two further Windows-only fixes it
  surfaced (`network.rs` dead code, a non-portable `ssh.rs` test).

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
