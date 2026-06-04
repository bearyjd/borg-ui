# PR Review: #33 — test(smoke-windows): Windows GUI-validation harness (keychain + scheduled-firing + Tier-C signals)

**Reviewed**: 2026-06-04
**Author**: bearyjd
**Branch**: feat/windows-gui-validation-harness → master
**Decision**: APPROVE (with comments) — informational; formal approval left to a human/separate-agent pass (author self-review)

## Summary
A Windows GUI-validation harness plus a small product fix (`BorgError::detail()` so failed scheduled backups record borg's stderr). All five real-desktop items (keychain, scheduled-firing, window/UI/tray, `--minimized`, console flash) plus close-to-tray were validated on a real KVM Windows VM during development. Code is well-tested and well-commented; this independent pass found one MEDIUM (a data-loss edge in the backup/restore added during the earlier devil's-advocate fixes) which has been fixed in `2ec9d55`. No CRITICAL/HIGH issues; validation green.

## Findings

### CRITICAL
None.

### HIGH
None.

### MEDIUM
- **`tests/smoke-windows/validate-gui.ps1` — profiles.json backup/restore crash-reentrancy data loss (FIXED in `2ec9d55`).** The Tier B backup did `Copy-Item $profilesPath $profilesBak -Force` unconditionally. If a prior run crashed after staging but before the `finally` restore, `profiles.json` already holds the smoke profile and `.smoke-bak` holds the real config; a re-run would overwrite `.smoke-bak` (real) with the staged smoke profile and lose the user's real config permanently — defeating the safety the backup was added for. Fixed with a guard: only back up when no `.smoke-bak` already exists.

### LOW
- **`validate-gui.ps1` `Find-BorgUi` recursive fallback is unordered.** `Get-ChildItem C:\borgui-test -Recurse -Filter borg-ui.exe | Select-Object -First 1` could pick a debug build over a release one if both exist. Mitigated by the ordered candidate list checked first (release path preferred); the recursive sweep is only a last resort. Optional: sort by `LastWriteTime -Descending`.
- **`validate-gui.ps1` `Ensure-BorgBeside` pollutes the build output dir.** It copies the whole borg distribution (`borg.exe` + `_internal\`) into the workspace `target/release` and never removes it. Cosmetic on a throwaway VM; harmless but leaves build artifacts mixed with vendored borg files.
- **`gui-results.json` written with `-Encoding UTF8`** (BOM under PS 5.1) while the rest of the harness is ASCII-strict. Consistent with `validate.ps1`/`validate-edge.ps1` (not a regression), but a BOM can trip strict JSON parsers. Optional: `-Encoding ascii`.

## Notes on prior review coverage
This PR already went through two earlier review passes this session (an `oh-my-claudecode:code-reviewer` agent pass and a 5-round devil's-advocate review), whose 8 action items were all applied in `09cd3ad` (toast first-line cap, `stderr_tail` window + boundary tests, non-panicking `cmdkey_list`, SKIP-contract HRESULT fallback + doc, `$event`→`$successEvt`, `-match`→`.Contains`, and the now-superseded backup/restore). This pass adds the reentrancy fix on top.

## Validation Results

| Check | Result |
|---|---|
| Type check | N/A (Rust) |
| Lint (`cargo clippy --workspace --all-targets -- -D warnings`) | Pass |
| Tests (`cargo test --workspace`) | Pass (223 tests; `error::` 9/9, `borg-ui` lib 28/28) |
| Build (`cargo build --workspace`) | Pass |
| Harness static (ASCII-only, brace balance, `bash -n run.sh`) | Pass |
| On-VM (`make validate-gui`) | Pass — keychain + scheduled-firing 2/2; Tier C (window/UI/tray, `--minimized`, console-flash, close-to-tray) confirmed via session-1 screenshots |

## Files Reviewed
- `.gitignore` — Modified (ignore smoke run logs + stray lock)
- `HANDOFF.md` — Modified (status + findings)
- `app-tauri/src-tauri/src/keychain.rs` — Modified (env-gated Credential Manager test + non-panicking helper)
- `app-tauri/src-tauri/src/lib.rs` — Modified (notification first-line + cap)
- `app-tauri/src-tauri/src/scheduled.rs` — Modified (record `e.detail()` on failures)
- `crates/borg-core/src/error.rs` — Modified (`BorgError::detail()` + `stderr_tail` + 5 tests)
- `tests/smoke-windows/Makefile` — Modified (`validate-gui` / `gui-all` targets)
- `tests/smoke-windows/README.md` — Modified (Tier C procedure, build-route + bundling gotchas)
- `tests/smoke-windows/run.sh` — Modified (`validate-gui` / `gui-all` subcommands)
- `tests/smoke-windows/validate-gui.ps1` — Added (the harness)
