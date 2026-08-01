# BorgUI Roadmap Status

Last updated: 2026-08-01.

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

- [#64](https://github.com/bearyjd/borg-ui/issues/64) — production Authenticode signing. Repo side complete; blocked on Azure provisioning. Runbook + exact missing secrets/variables are in a comment on the issue.
- [#114](https://github.com/bearyjd/borg-ui/issues/114) — archive browser intermittently under-counts (~1 run in 6). [#116](https://github.com/bearyjd/borg-ui/pull/116) fixes the race it is attributed to and makes an incomplete listing visible; deliberately left open because the fix is reasoned, not demonstrated.
- [#119](https://github.com/bearyjd/borg-ui/issues/119) — `keychain_credential_manager` has never run, so **Credential Manager is unverified on Windows**. One blocker left: the wrong-architecture `WebView2Loader.dll` loads (`0xC0000139`).

**Release state: `v0.3.1` is tagged and built but the GitHub release is a
DRAFT.** `releases/latest` still resolves to v0.3.0, so nobody receives 0.3.1
until it is published. Publishing is also the prerequisite for testing the
updater path at all — the endpoint is hardcoded to `releases/latest`.

Closed in the 0.3.1 cycle: #100, #101, #102, #103, #104 (all raised by the
security and adversarial review passes on #99), plus the passphrase-change
desync, app-wide `:focus-visible`, and borg-core log events being dropped
below ERROR.

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
