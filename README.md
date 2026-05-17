# BorgUI

Native Windows GUI for BorgBackup. Back up to your borg server without WSL.

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

## v0.1 Scope

1. Connect to borg repo over SSH
2. Back up a folder
3. List archives

## License

MIT
