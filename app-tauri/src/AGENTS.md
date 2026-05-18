<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# src (Frontend)

## Purpose
Svelte 5 / SvelteKit frontend for BorgUI. Provides the user interface for configuring repositories, running backups, and browsing archives. Communicates with the Rust backend via Tauri's `invoke` IPC.

## Key Files

| File | Description |
|------|-------------|
| `app.html` | Root HTML template for SvelteKit |
| `app.css` | Global styles — CSS custom properties for design tokens |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `lib/` | Shared library code — stores and components (see `lib/AGENTS.md`) |
| `routes/` | SvelteKit file-based routes — pages and layouts (see `routes/AGENTS.md`) |

## For AI Agents

### Working In This Directory
- Uses Svelte 5 runes (`$state`, `$props`, `$derived`) — not legacy reactive `$:` syntax.
- Tauri IPC via `import { invoke } from '@tauri-apps/api/core'`.
- CSS uses custom properties defined in `app.css` — use tokens like `var(--color-accent)`, `var(--space-4)`.
- SvelteKit with static adapter — no SSR, no server routes.

### Testing Requirements
- `pnpm check` for type checking
- Visual testing via `pnpm tauri dev`

<!-- MANUAL: -->
