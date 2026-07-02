# Handoff

Last updated: 2026-06-29.

## Current state

`master` is at `2cb5963` (`feat: cancel archive listing streams`, PR #59) before
this documentation PR. The v0.1 release exists, and the post-v0.1 roadmap items
through archive-list cancellation are merged:

- repository integrity checks and opt-in monthly metadata check
- encrypted recovery-key export/import
- consent-based Tauri updater with signed updater artifacts and `latest.json`
- Windows release pipeline prepared for Azure Trusted Signing, disabled by default
- scheduled-backup retry/missed-run reporting
- guided SSH public-key onboarding
- archive listing cancellation

No implementation PR is in flight. Remaining roadmap work is documentation only.

## Architecture map

- `crates/borg-core`: portable Borg CLI wrapper, config validation, SSH helpers,
  archive/diff parsing, cancellation, updater-independent core logic.
- `crates/borg-platform-win`: Windows VSS, Task Scheduler, autostart, and
  Windows-specific command wrappers.
- `app-tauri/src-tauri`: Tauri IPC commands, profiles, SQLite history,
  diagnostics import/export, recovery-key handling, keychain, scheduled runners,
  updater plumbing, tray/window lifecycle.
- `app-tauri/src`: Svelte 5 UI, stores, Settings sections, archive browser,
  backup/restore flows, update/recovery/integrity/schedule UI.

## Data and secret handling

- History is SQLite-backed. User-facing backup/restore events, integrity events,
  and scheduled-attempt diagnostics are separate record types.
- Profile schema rejects future versions unless explicitly overwritten.
- Diagnostics/config exports intentionally exclude passphrases, SSH private key
  paths/material, recovery payloads, source file listings, and updater private
  keys.
- Recovery-key exports are portable JSON files containing age/scrypt-encrypted
  Borg key material. Store the file and recovery passphrase separately.
- Updater signing and Windows Authenticode signing are separate trust systems.
  Keep only the updater private key in `TAURI_SIGNING_PRIVATE_KEY`; the public
  updater key is committed in Tauri config.

## Release operations

1. Confirm local quality gate:

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   cd app-tauri && pnpm check && pnpm build
   git diff --check
   ```

2. Cut a tag:

   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. The Release workflow stages the pinned Borg-for-Windows bundle, builds MSI and
   NSIS installers, signs Tauri updater artifacts, writes `latest.json`, uploads
   artifacts, and creates a draft GitHub Release.
4. Review the draft release assets and notes, then publish manually.
5. For a dry run, dispatch the Release workflow manually; it uploads artifacts but
   does not publish a release.

Authenticode signing remains disabled until Azure Trusted Signing is configured.
See `docs/windows-signing.md`. If signing is explicitly enabled without required
secrets/variables or if signing/verification fails, the release workflow fails.

## Windows smoke commands

Run from a host with the KVM Windows harness when the change affects the named
surface:

```bash
cd tests/smoke-windows
make validate-installer
make validate-vss
make validate-vss-manual
make validate-archive-smoke
make validate-gui-flows
make validate-autostart-login
```

Use Release workflow dry runs for installer/updater/signing changes. The smoke
harness details are in `tests/smoke-windows/README.md`.

## Operational gotchas

- Borg prompts must remain disabled in GUI/headless paths. Keep
  `BORG_PASSPHRASE`, `BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK`,
  `BORG_DISPLAY_PASSPHRASE=no`, `BORG_RELOCATED_REPO_ACCESS_IS_OK=yes`, and
  closed stdin behavior intact.
- Borg-for-Windows 1.4.4+win7 accepts raw drive-letter repository paths
  (`C:\repo`) directly. Do not restore the former `\\localhost\C$` rewrite:
  administrative shares prevent local repositories from working as standard users.
- VSS stores archive paths through a drive-letter junction so VSS backups remain
  restorable and match live backup path layout.
- `borg extract --progress --log-json` reports restore progress as
  `progress_percent`, not `archive_progress`; do not infer restore success from
  file-count events.
- Archive listing streams are cancellable by request id. Browser close, Escape,
  archive replacement, component teardown, and closed IPC channels should remain
  neutral UI states rather than failures.
- Windows PowerShell smoke scripts should stay ASCII-only for PS 5.1.

## Follow-up issue candidates

The post-v0.2 follow-ups are tracked explicitly:

- `#64` production Authenticode activation after Azure/OIDC configuration.
- `#65` installed-app updater smoke execution using
  `tests/smoke-windows/validate-updater.ps1`.
- `#66` Borg-for-Windows drive-letter path parsing/upstream tracking.
- `#67` demand-gated provider-specific SSH examples and Windows mount research.

Metered-network controls shipped in PR `#61`; the updater smoke harness shipped
in PR `#62`; Azure signing configuration validation and its Windows Release dry
run shipped in PR `#63`.
