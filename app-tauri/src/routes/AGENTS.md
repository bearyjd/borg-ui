<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-05-17 | Updated: 2026-05-17 -->

# routes

## Purpose
SvelteKit file-based routing. Each subdirectory is a page in the app. The layout provides the sidebar navigation shell.

## Key Files

| File | Description |
|------|-------------|
| `+layout.svelte` | App shell — sidebar nav (Dashboard, Backup, Archives, Settings) + content area |
| `+page.svelte` | Dashboard — shows borg version, backup history, repo connection, next scheduled run, latest integrity check, and missed-run warnings |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `backup/` | Backup page — select source folders and trigger `create_backup` command |
| `archives/` | Archives page — list and browse backup archives from the repo |
| `settings/` | Settings page — configure SSH connection and repository path, test connection |
| `setup/` | Setup redirect — immediately navigates to `/settings` |

## For AI Agents

### Working In This Directory
- Each route has a single `+page.svelte`. SvelteKit routing is filesystem-based.
- All pages use Svelte 5 runes (`$state`) for local state.
- Backend calls use `invoke` from `@tauri-apps/api/core`.
- The layout uses Svelte 5 snippets (`{@render children()}`) — not `<slot/>`.
- CSS is scoped per-component via `<style>` blocks. Use design tokens from `app.css`.

### Page Summary

| Route | Tauri Commands Used |
|-------|-------------------|
| `/` (Dashboard) | `get_borg_version`, history/integrity/scheduled status commands |
| `/backup` | `create_backup`, `cancel_backup` |
| `/archives` | `list_archives`, `stream_archive_contents`, `cancel_archive_listing`, restore/diff/delete/compact commands |
| `/settings` | repo, SSH key, profile, schedule, retention, integrity, recovery, diagnostics, update, startup commands |
| `/setup` | None (redirect only) |

### Adding a New Route
1. Create `route-name/+page.svelte` in this directory
2. Add nav entry in `+layout.svelte` `navItems` array
3. Use `invoke` to call any needed Tauri commands

### Common Patterns
- Page header: `<header class="page-header"><h1>Title</h1><p class="subtitle">...</p></header>`
- Error display: conditional `{#if error}` block with `.error-banner` styling
- Empty state: `.empty-state` div with dashed border and instructional text
- Button classes: `.btn .btn-primary` and `.btn .btn-secondary`

## Dependencies

### Internal
- `$lib/stores/repo` — shared repo configuration state
- `$app/stores` — SvelteKit page store for active route detection
- `$app/navigation` — `goto` for programmatic navigation

### External
- `@tauri-apps/api/core` — `invoke` for Tauri IPC

<!-- MANUAL: -->
