# BorgUI

Native Windows GUI for BorgBackup. Back up to a borg server over SSH — or straight
to a local folder, external/USB drive, or network share — without WSL.

## Features

- **Backup destinations**: a borg server over SSH, **or** a local folder / USB
  drive / network share (no server required).
- **Consistent backups of open files (Windows VSS)**: files that are locked or in
  use at backup time (Outlook data, databases, open documents, browser profiles)
  are captured from a **Volume Shadow Copy** — a point-in-time snapshot of the
  drive — so they back up correctly instead of being skipped. Requires
  administrator rights and a single source volume; when VSS can't run, BorgUI
  automatically falls back to a live-file backup, where a locked file is skipped
  with a *warning* rather than failing the whole backup.
- **Reliable backups**: backups can be **cancelled** mid-run, and stalled SSH
  calls **time out** instead of freezing the app. Scheduled backups retry
  transient transport failures up to three total attempts and report missed
  runs/Task Scheduler diagnostics on the dashboard and in Settings.
- **Restore**: browse an archive's contents and restore everything or a selected
  subset. Large archive listings stream in batches, render with virtual
  scrolling, and cancel promptly when the browser closes or changes archive.
- **Encryption** with the passphrase stored in the OS keychain, **scheduled
  backups** via Windows Task Scheduler, **retention/pruning**, multiple
  **profiles**, custom **archive naming**, history, and desktop notifications.
- **Repository integrity checks**: manual metadata-only or full-data `borg check`
  runs, plus an opt-in monthly metadata check for the active profile. BorgUI
  records check history but never exposes Borg's `--repair` mode.
- **Recovery-key export** for encrypted repositories: exports Borg key material
  into a portable, age/scrypt-encrypted JSON file protected by a separate user
  passphrase. Recovery material is excluded from logs, diagnostics, history, and
  configuration exports.
- **Consent-based updates**: BorgUI checks signed GitHub Releases updater
  metadata at startup and from Settings, shows release details, and installs
  only after user confirmation.
- **Self-evident UI**: every settings section has inline help and concrete
  examples aimed at non-technical users, including guided password-free SSH
  public-key onboarding with `authorized_keys` instructions.

## Download & install

Grab the latest installer from the [**Releases**](https://github.com/bearyjd/borg-ui/releases) page:

- **`BorgUI_<version>_x64-setup.exe`** — NSIS installer (recommended for most users).
- **`BorgUI_<version>_x64_en-US.msi`** — MSI installer (for enterprise / Group Policy deployment).

Both bundle BorgBackup (`borg.exe`, BSD-licensed) — no separate borg install is needed.

> **Signing status:** unsigned builds are still supported and clearly labelled
> in the release artifacts. The release workflow is ready for Azure Trusted
> Signing through GitHub OIDC, but production Authenticode signing remains
> disabled until the repository variables/secrets are configured. Unsigned
> installers may trigger SmartScreen; click **More info → Run anyway** if you
> trust the release source.

## Architecture

- **borg-core** — Portable Rust library: config, borg CLI wrapper, SSH, progress parsing
- **borg-platform-win** — Windows-specific: VSS snapshots, Task Scheduler
- **app-tauri** — Tauri 2 desktop app with Svelte 5 frontend

## Development

### Prerequisites

- [Rust](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (20+)
- [pnpm](https://pnpm.io/)
- [borg.exe](https://github.com/marcpope/borg-windows/releases) — download `borg-windows.zip`
  (1.4.4+win6) and extract the **whole** dist (`borg.exe` **and** its sibling
  `_internal/` folder) into `app-tauri/src-tauri/binaries/borg/`. borg is a
  PyInstaller onedir bundle; `borg.exe` will not start without `_internal/` beside it.
  The release workflow stages this automatically; it's only needed for local builds.

### Setup

```bash
cd app-tauri
pnpm install
pnpm tauri dev
```

### Project Structure

```
borg-ui/
├── crates/
│   ├── borg-core/          # Shared portable library
│   └── borg-platform-win/  # Windows platform code
├── app-tauri/
│   ├── src/                # Svelte 5 frontend
│   └── src-tauri/          # Tauri Rust backend
├── docs/                   # Operational docs
└── Cargo.toml              # Workspace root
```

## Testing

```bash
cargo test --workspace          # unit + integration tests
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cd app-tauri && pnpm check && pnpm build
```

### End-to-end backup → restore tests

`crates/borg-core/tests/e2e_backup_restore.rs` drives a **real borg binary**
through init → create → list → browse → extract → byte-for-byte verify, covering
encrypted repos, selective restore, special-character filenames, and the
locked/unreadable-file warning path. These are the trust-critical paths — the
parts that must work for a backup to be restorable.

They are **skipped** unless a borg binary is provided, so CI without borg stays
green. To run them, point `BORG_TEST_BIN` at a borg executable:

```bash
BORG_TEST_BIN=/path/to/borg cargo test -p borg-core --test e2e_backup_restore -- --nocapture
```

A local on-disk repository is used, so no SSH server is needed — the same code
path the app uses for "Local folder / USB drive" repos.

## Release operations

- Cut a release with `git tag vX.Y.Z && git push origin vX.Y.Z`. The Release
  workflow builds MSI and NSIS installers, signs updater artifacts with the
  Tauri updater key, writes `latest.json`, uploads build artifacts, and creates
  a draft GitHub Release for manual review/publish.
- Manual dry run: Actions → Release → Run workflow. It builds installers and
  uploads artifacts without publishing a release.
- Updater signing and Windows Authenticode signing are separate. Store only the
  Tauri updater private key in `TAURI_SIGNING_PRIVATE_KEY`; the public updater
  key is committed in `tauri.conf.json`. Authenticode uses Azure Trusted Signing
  config documented in [docs/windows-signing.md](docs/windows-signing.md).
- Recovery files, passphrases, SSH private keys, source listings, and updater
  private keys must never be added to diagnostics, config exports, history, or
  logs.

## Recovery and support procedures

Encrypted repository recovery:

1. In Settings → Encrypted recovery key, export a recovery key after the
   repository is configured.
2. Choose a recovery passphrase that is different from the Borg repository
   passphrase.
3. Store the recovery file and recovery passphrase separately.
4. To recover, configure the repository, import the recovery file, enter the
   recovery passphrase, and then set the repository passphrase in Windows
   Credential Manager if needed.

Support bundles:

- Export support bundles from Settings → Diagnostics when troubleshooting.
- Bundles and configuration exports intentionally exclude saved passphrases, SSH
  private keys, encrypted recovery payloads, source file listings, and updater
  private keys.
- Do not attach recovery-key files or updater private keys to issues or logs.

Scheduled backup diagnostics:

- Scheduled backups use at most three total attempts: initial run, then retries
  after 30 seconds and 120 seconds for classified transient transport failures.
- Authentication, configuration, hook, repository-not-found, and integrity
  failures are not retried.
- Every attempt is recorded separately from the user-facing backup history; the
  dashboard warns when a configured schedule appears missed.

## Windows smoke commands

From a host that can run the KVM Windows harness:

```bash
cd tests/smoke-windows
make validate-installer
make validate-vss
make validate-vss-manual
make validate-archive-smoke
make validate-gui-flows
```

The smoke harness documentation lives in
[tests/smoke-windows/README.md](tests/smoke-windows/README.md).

## License

MIT
