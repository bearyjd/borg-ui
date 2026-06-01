# BorgUI

Native Windows GUI for BorgBackup. Back up to a borg server over SSH — or straight
to a local folder, external/USB drive, or network share — without WSL.

## Features

- **Backup destinations**: a borg server over SSH, **or** a local folder / USB
  drive / network share (no server required).
- **Reliable backups**: a file that's locked or in use at backup time (Outlook
  data, open documents, browser profiles) is skipped with a *warning* — it never
  fails the whole backup. Backups can be **cancelled** mid-run, and stalled SSH
  calls **time out** instead of freezing the app.
- **Restore**: browse an archive's contents and restore everything or a selected
  subset.
- **Encryption** with the passphrase stored in the OS keychain, **scheduled
  backups** via Windows Task Scheduler, **retention/pruning**, multiple
  **profiles**, custom **archive naming**, history, and desktop notifications.
- **Self-evident UI**: every settings section has inline help and concrete
  examples aimed at non-technical users.

## Architecture

- **borg-core** — Portable Rust library: config, borg CLI wrapper, SSH, progress parsing
- **borg-platform-win** — Windows-specific: VSS snapshots, Task Scheduler
- **app-tauri** — Tauri 2 desktop app with Svelte 5 frontend

## Development

### Prerequisites

- [Rust](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (20+)
- [pnpm](https://pnpm.io/)
- [borg.exe](https://github.com/marcpope/borg-windows/releases) (place in `app-tauri/src-tauri/binaries/`)

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
└── Cargo.toml              # Workspace root
```

## Testing

```bash
cargo test --workspace          # unit + integration tests
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
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

## License

MIT
