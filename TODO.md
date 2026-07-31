# BorgUI Roadmap Status

Last updated: 2026-07-31.

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
- Post-v0.2 follow-up PRs #61–#63 and #69 are merged on `master`.
- The installed-app updater smoke passed against the updater-capable 0.1.0
  baseline and published 0.2.0 target (3 passed, 0 failed, 0 skipped).
- Borg-for-Windows 1.4.4+win7 fixes native drive-letter repositories; BorgUI now
  passes those paths directly, including for standard users.
- Installers remain usable unsigned. Authenticode signing is prepared but intentionally disabled until Azure Trusted Signing repository configuration exists.
- Updater signing is separate from Authenticode signing; keep the updater private key only in GitHub Actions secrets.

Shipped since 2026-07-02 (see `HANDOFF.md` "Update (2026-07-29)"): Vorta-style
connection UX with paste-to-fill and repository summary, context-scoped
plain-language error hints across Settings/Backup/Archives/dashboard, settings
page decomposition, first-run setup wizard at `/setup`, frontend CI switched to
pnpm with a vitest gate.

## Tracked follow-up issues

- [#64](https://github.com/bearyjd/borg-ui/issues/64) — enable production Authenticode signing after Azure/OIDC configuration (blocked on cert provisioning). The repo side is ready: `release.yml` carries the `enable_signing` input, Azure OIDC login and the signing action, and `scripts/validate-signing-config.ps1` exists. What is missing is external — an Azure Artifact Signing account, a certificate profile, a federated identity, and the repo secrets/variables.

**No other open issues.** #100–#104, all raised by the security and adversarial
review passes on #99, are closed:

| Issue | Shipped in |
| --- | --- |
| #100 rotation left a stale recovery-key export counting as "ready" | [#106](https://github.com/bearyjd/borg-ui/pull/106), repaired in [#110](https://github.com/bearyjd/borg-ui/pull/110) |
| #102 a stored passphrase was never verified against the repository | [#107](https://github.com/bearyjd/borg-ui/pull/107) |
| #104 `authenticated` repos were created with an empty passphrase | [#107](https://github.com/bearyjd/borg-ui/pull/107), frontend half in [#110](https://github.com/bearyjd/borg-ui/pull/110) |
| #101 the support bundle could carry account names in paths | [#108](https://github.com/bearyjd/borg-ui/pull/108), extended to `configuration.json` in [#110](https://github.com/bearyjd/borg-ui/pull/110) |
| #103 the webview had no Content-Security-Policy | [#109](https://github.com/bearyjd/borg-ui/pull/109) |

Also closed earlier: the passphrase-change desync
([#99](https://github.com/bearyjd/borg-ui/pull/99)), the app-wide keyboard
`:focus-visible` styles (#97), and borg-core log events being dropped below
ERROR ([#94](https://github.com/bearyjd/borg-ui/pull/94)).

Two of those needed a second pass, which is worth remembering when reviewing
this class of change: #106 shipped as a **silent no-op** because a SQLite CHECK
constraint rejected the new event kind and the failure was swallowed by a
`warn!`, and #107 made both `authenticated` modes impossible to create because
the passphrase rule was duplicated in the frontend and only the backend was
updated. Unit tests over pure functions saw neither — the tests that catch this
class have to cross the boundary (database, or Rust↔TypeScript parity).

Provider-specific SSH examples and Windows archive mounting were evaluated in
[#67](https://github.com/bearyjd/borg-ui/issues/67). There is no recorded user
demand beyond the gate issue itself. Borg-for-Windows does not provide
`borg mount`; WinFsp is a maintained filesystem framework, not a Borg archive
adapter. Do not add provider-specific text or a filesystem-driver dependency
without a new issue containing concrete demand and a maintained, tested design.
Browse/selective restore remains the supported archive access path.

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
