<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# lib

## Purpose
Shared frontend library code — Svelte stores for app-wide state and reusable components.

## Key Files

| File | Description |
|------|-------------|
| `stores/repo.ts` | `repoConfig` and `isConnected` writable stores — shared repo connection state |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `components/` | Reusable Svelte components (empty in v0.1) |
| `stores/` | Svelte writable stores for shared app state |

## For AI Agents

### Working In This Directory
- Stores use Svelte's `writable` from `svelte/store` (not runes) because they're shared across routes.
- `RepoConfig` interface mirrors the Rust `RepoConfig` struct — keep them in sync.
- New shared components go in `components/`. Import via `$lib/components/ComponentName.svelte`.
- New stores go in `stores/`. Import via `$lib/stores/storeName`.

### Common Patterns
- Store values match Rust types for seamless `invoke` calls (snake_case field names)

## Dependencies

### Internal
- Used by all route pages for repo state

### External
- `svelte/store` — writable store primitives

<!-- MANUAL: -->
