# PR Review: #44 — feat: add Windows release workflow that bundles borg and publishes installers

**Reviewed**: 2026-06-23
**Author**: bearyjd
**Branch**: feat/windows-release-installers → master
**Reviewer**: independent `code-reviewer` agent (separate lane; author did not self-approve)
**Decision**: REQUEST-CHANGES → **addressed** in commit `7174b92`

## Summary

Tag-triggered GitHub Actions pipeline that builds Windows installers (MSI + NSIS)
with borg bundled and publishes them to GitHub Releases. The borg-staging and the
Tauri `resources` design are correct. The independent review found one CRITICAL
defect (stale pnpm lockfile would fail the workflow's `--frozen-lockfile` step) plus
several MEDIUM/LOW items. The CRITICAL and the `workflow_dispatch` footgun are now
fixed; full lockfile unification is left as an optional follow-up.

## Findings

### CRITICAL
- **Stale `app-tauri/pnpm-lock.yaml` would fail `pnpm install --frozen-lockfile`**
  (`release.yml`). The committed pnpm lockfile predated `@tauri-apps/plugin-notification`
  (absent entirely) and pinned `@sveltejs/vite-plugin-svelte` at `^4` vs `package.json`'s
  `^5`. The PR's Linux CI uses `npm ci` against the in-sync `package-lock.json`, so it
  would have stayed green while the release pipeline broke. **Verified locally** then
  **FIXED** (`7174b92`): added `"packageManager": "pnpm@10.33.0"` to `package.json`,
  regenerated `pnpm-lock.yaml`, and confirmed `pnpm install --frozen-lockfile` exits 0.

### HIGH
- None.

### MEDIUM
- **Two divergent lockfiles** (`package-lock.json` kept current by CI vs `pnpm-lock.yaml`
  used by release). Root cause of the CRITICAL; will recur. **Partially addressed**:
  `packageManager` now declares pnpm as canonical and the lockfile is synced.
  **Remaining (optional follow-up):** delete `package-lock.json` and convert the
  `ci.yml` frontend job to pnpm so every PR validates the same lockfile. Left out of
  this PR to avoid rewriting working CI without explicit ask.
- **`workflow_dispatch` from a branch could create a junk release** named after the
  branch (`tagName: github.ref_name`). **FIXED** (`7174b92`): the publish step is now
  guarded with `if: startsWith(github.ref, 'refs/tags/v')`, so a branch dispatch runs
  the full build/staging as a dry-run but skips publishing.
- **`pnpm/action-setup` explicit `version` vs `packageManager` drift.** Mitigated:
  workflow pinned to `version: 10`, matching the `packageManager` field and the
  regenerated lockfile. (The action reads `packageManager` from the repo-root
  `package.json`, which doesn't exist here, so there is no conflict with the explicit
  version.)

### LOW
- Actions pinned by major tag (`@v0`, `@v5`) not full SHA — acceptable, matches `ci.yml`
  convention. Optional hardening: SHA-pin the write-scoped publish actions
  (`tauri-action`, `pnpm/action-setup`). Not blocking.
- `Invoke-WebRequest` for the borg download has no retry — a transient blip fails the
  release. Optional resilience improvement.

### Open question (low confidence, verify on first real run)
- `tauri-action` package-manager auto-detection with two lockfiles present. The
  synced pnpm lockfile + the canonical `packageManager` field make this safe; deleting
  `package-lock.json` (the MEDIUM follow-up) would remove the ambiguity entirely.

## Positive observations
- borg staging is exemplary: pinned SHA-256, `ErrorActionPreference=Stop`, and
  post-extract assertions that `borg.exe` + `_internal/` exist before the slow bundle.
- `resources: { "binaries/borg/": "" }` uses the correct trailing-slash/`""` form that
  preserves the nested `_internal/` (globs would have flattened it); on Windows
  `$RESOURCE` == exe dir, so borg lands beside `borg-ui.exe` with no runtime code change.
- `releaseDraft: true` and the unsigned-installer note (releaseBody + README) are honest
  and safe defaults. `permissions: contents: write` is correctly scoped.

## Validation Results

| Check | Result |
|---|---|
| YAML/JSON parse (workflow + config) | Pass |
| `pnpm install --frozen-lockfile` (the workflow gate) | Pass (exit 0) |
| Windows `tauri build` (installers + borg layout) | Deferred — runner-only; first tag/dispatch run |
| Lint / tests (Rust, frontend) | Via PR CI (Linux) |

## Files Reviewed
- `.github/workflows/release.yml` — Added (then fixed)
- `app-tauri/src-tauri/tauri.conf.json` — Modified
- `README.md` — Modified
- `app-tauri/package.json` — Modified (fix commit)
- `app-tauri/pnpm-lock.yaml` — Regenerated (fix commit)
