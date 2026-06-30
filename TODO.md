# BorgUI Roadmap Status

Last updated: 2026-06-29.

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

## Current release posture

- `v0.1.0` has been published with MSI and NSIS installers.
- Later post-v0.1 roadmap work is merged on `master`; cut the next tag to ship it.
- Installers remain usable unsigned. Authenticode signing is prepared but intentionally disabled until Azure Trusted Signing repository configuration exists.
- Updater signing is separate from Authenticode signing; keep the updater private key only in GitHub Actions secrets.

## Explicit follow-up issue candidates

Track new work as GitHub issues instead of reopening the completed roadmap:

- Enable production Authenticode signing after Azure Trusted Signing account/profile variables and OIDC role assignment are configured.
- Run and record the installed-app updater smoke test against a published post-v0.1 release.
- Add metered-network controls only if user feedback shows scheduled backups need them.
- Consider upstreaming or tracking Borg-for-Windows drive-letter repo parsing so BorgUI can eventually stop using the UNC workaround for local drive paths.
- Add provider-specific SSH examples only if support requests justify them; the current onboarding is intentionally provider-neutral.
- Consider FUSE-like mount support only if a credible Windows-native approach emerges; current restore/browse flows are the supported path.

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
