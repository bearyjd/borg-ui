<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# app-tauri

## Purpose
Tauri 2 desktop application combining a Svelte 5 frontend with a Rust backend. This is the user-facing application that ties together borg-core and borg-platform-win into a GUI.

## Key Files

| File | Description |
|------|-------------|
| `package.json` | Node dependencies and scripts (`dev`, `build`, `check`, `tauri`) |
| `pnpm-lock.yaml` | Locked pnpm dependencies |
| `svelte.config.js` | SvelteKit configuration (static adapter for Tauri) |
| `vite.config.ts` | Vite build configuration |
| `tsconfig.json` | TypeScript compiler settings |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `src/` | Svelte 5 frontend source (see `src/AGENTS.md`) |
| `src-tauri/` | Tauri Rust backend — commands, config, icons (see `src-tauri/AGENTS.md`) |

## For AI Agents

### Working In This Directory
- Run `pnpm install` after modifying `package.json`.
- `pnpm tauri dev` starts both the Vite dev server and the Tauri Rust backend.
- `pnpm build` builds only the frontend; `pnpm tauri build` builds the full app.
- Frontend uses Svelte 5 runes (`$state`, `$props`, `$derived`) — not Svelte 4 stores for component state.
- The `repoConfig` store in `src/lib/stores/repo.ts` is the one Svelte store (writable) for shared app-level state.

### Testing Requirements
- `pnpm check` for Svelte and TypeScript type checking
- Rust backend tests via `cargo test -p borg-ui` from workspace root

## Dependencies

### External
- `@tauri-apps/api` ^2 — Tauri frontend API (invoke, events)
- `@tauri-apps/cli` ^2 — Tauri CLI tooling
- `svelte` ^5 — UI framework
- `@sveltejs/kit` ^2 — App framework
- `@sveltejs/adapter-static` ^3 — Static build for Tauri
- `vite` ^6 — Build tool
- `typescript` ^5 — Type checking

<!-- MANUAL: -->
