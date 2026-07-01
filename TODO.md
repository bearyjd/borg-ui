# BorgUI Roadmap Status

Last updated: 2026-06-30.

The original Vorta-parity roadmap is complete for the Windows-focused v0.1 line:

- backup destinations: SSH, local folder, USB drive, and network share
- VSS snapshots for consistent Windows backups of open files, with live-file fallback
- restore, selective restore, archive browsing, archive diff, and archive-list cancellation
- repository initialization, encryption/passphrase storage, retention/prune, compact, and delete
- multiple profiles, profile import/export, custom archive naming, pre/post hooks
- backup history, SQLite diagnostics/history storage, desktop notifications, tray, and autostart
- scheduled backups through Windows Task Scheduler, including transient retry and missed-run reporting
- repository integrity checks, manual full-data verification, and opt-in monthly metadata checks
- encrypted portable recovery-key export/import
- consent-based signed updater flow
- Windows release workflow, unsigned artifacts by default, and signing-ready Azure Trusted Signing path
- guided SSH public-key onboarding without password collection
- opt-in metered-network skipping for scheduled backups
- installed-app updater smoke harness
- tested Azure signing configuration preflight

## Current release posture

- `v0.2.0` is published with MSI, NSIS, updater signatures, and `latest.json`.
- Post-v0.2 follow-up PRs #61–#63 are merged on `master`.
- Installers remain usable unsigned. Authenticode signing is prepared but intentionally disabled until Azure Trusted Signing repository configuration exists.
- Updater signing is separate from Authenticode signing; keep the updater private key only in GitHub Actions secrets.

## Tracked follow-up issues

- [#64](https://github.com/bearyjd/borg-ui/issues/64) — enable production Authenticode signing after Azure/OIDC configuration.
- [#65](https://github.com/bearyjd/borg-ui/issues/65) — run and record the installed-app updater smoke with an updater-capable baseline.
- [#66](https://github.com/bearyjd/borg-ui/issues/66) — track/upstream Borg-for-Windows drive-letter repository parsing.
- [#67](https://github.com/bearyjd/borg-ui/issues/67) — gate provider-specific SSH examples and Windows mount research on evidence.

## Quality gate for future PRs

Run the relevant focused tests plus:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cd app-tauri && pnpm check && pnpm build
git diff --check
```

For release-affecting changes, also run the applicable Windows smoke command from
`tests/smoke-windows/README.md` and a Release workflow dry run.
