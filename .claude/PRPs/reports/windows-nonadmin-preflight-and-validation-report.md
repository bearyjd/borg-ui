# Implementation Report: non-admin preflight (plan 2) + non-admin/multi-drive validation (plan 4)

## Summary
Implemented two coupled plans together: (2) a Windows-only `RepoConfig::local_repo_preflight()` that turns the inaccessible-admin-share case into a clear "run as admin / use SSH" error instead of a cryptic borg failure, wired into the borg-running commands; and (4) a smoke-harness edge pass (`validate-edge.ps1` + `provision-edge`/`validate-edge`/`edge-all`) that validates the non-admin behavior and the multi-drive (repo on D:, restore to C:) case. The non-admin half was validated 3/3 on the real Windows VM; the multi-drive half is harness-complete but blocked from running here by a dockur disk-provisioning limitation (details below). A subsequent `/devils-advocate` review produced 7 action items, all applied and re-validated on the live VM (see "Devil's-Advocate Review Fixes").

## Assessment vs Reality

| Metric | Predicted (plans) | Actual |
|---|---|---|
| Complexity | Small (plan 2) + Medium (plan 4) | As predicted |
| Confidence | 8/10 (plan 2), 7/10 (plan 4) | Plan 2 fully met; plan 4 non-admin met, multi-drive blocked by env (the flagged top risk materialized) |
| Files Changed | ~2 (plan 2) + ~5 (plan 4) | 2 + 6 |

## Tasks Completed

| # | Task | Status | Notes |
|---|---|---|---|
| 2.1 | `unc_share_root` / `non_admin_message` / `share_unreachable` + `local_repo_preflight` | ✅ Complete | Pure helpers `#[cfg(any(windows,test))]`; probe `#[cfg(windows)]`; let-chain in the method |
| 2.2 | Wire preflight into commands | ✅ Complete | Initially scoped to 5 first-contact ops; the devil's-advocate review (#6) extended it to **all 10** borg-op commands via the `precheck_repo` helper |
| 2.3 | Unit tests | ✅ Complete | 4 new (share-root extract/none, message, ssh+local no-op) |
| S1 | Second disk (`DISK2_SIZE`) + diskpart init | ✅ Implemented | Works on a *fresh* VM; dockur won't add it to a recreated persisted volume (see Deviations) |
| S2 | Standard user `borgstd` (oem + idempotent run.sh) | ✅ Complete | Created + SSH-validated on the VM |
| M1 | `multi_drive_cross_restore` | ✅ Implemented | Could not execute (no D: on the persisted VM) — see Deviations |
| N1/N2 | non-admin share-denied + fast-fail | ✅ Complete & **validated 3/3 on real Windows** | incl. `preflight_trigger_matches` (added in review #1) |
| W1 | run.sh/Makefile/README wiring | ✅ Complete | `provision-edge`, `validate-edge`, `edge-all` |

## Validation Results

| Level | Status | Notes |
|---|---|---|
| Static (fmt/clippy) | ✅ Pass | `clippy --workspace --all-targets -D warnings` clean (post-review) |
| Unit Tests | ✅ Pass | borg-core 132, borg-ui 27 (4 new preflight tests); cfg-gating keeps Linux behavior identical |
| Build | ✅ Pass | workspace builds; `bash -n run.sh`; validate-edge.ps1 ASCII |
| Integration (real Windows) | ◑ Partial | **non-admin edge: 3/3 PASS** on the VM as `borgstd` (share-denied + preflight-trigger-matches + fast-fail); **multi-drive: SKIP** (no D: — dockur didn't attach DISK2 to the recreated persisted volume; admin mode reports Failed:0) |
| Edge Cases | ✅ Pass (testable ones) | SSH/local/non-Windows preflight no-op; non-admin fast-fail (no hang) |

## Files Changed

| File | Action | Notes |
|---|---|---|
| `crates/borg-core/src/config.rs` | UPDATED | preflight method + 3 helpers + 4 tests; `share_unreachable` errno logging (review #3) |
| `app-tauri/src-tauri/src/commands.rs` | UPDATED | `precheck_repo` async helper (validate + `spawn_blocking` preflight) wired into all 10 borg-op entry points (review #2/#6) |
| `tests/smoke-windows/validate-edge.ps1` | CREATED | `-Mode admin|nonadmin` edge validation |
| `tests/smoke-windows/run.sh` | UPDATED | borgstd identity, `provision_edge`, `run_validate_edge`, subcommands |
| `tests/smoke-windows/docker-compose.yml` | UPDATED | `DISK2_SIZE: 8G` |
| `tests/smoke-windows/oem/install.bat` | UPDATED | `borgstd` + diskpart D: init (fresh VMs) |
| `tests/smoke-windows/Makefile` + `README.md` + `.gitignore` | UPDATED | targets, docs, `validate-edge.log` |
| `HANDOFF.md` | UPDATED | preflight done + edge-validation status |

## Deviations from Plan
1. **Preflight scope** (plan 2, Task 5): initially wired into the 5 first-contact borg commands, on the rationale that the others were only reachable transitively. The devil's-advocate review (#6) rejected this as fragile — callers can hit any command directly — and extended the preflight to **all 10** via `precheck_repo`. Net result: more coverage than the plan, not less.
2. **Multi-drive not executed** (plan 4): the plan's top risk materialized — dockur/windows provisions a second disk only on a *fresh* install, not when recreating a container over a persisted `win-storage` volume. `DISK2_SIZE=8G` reached the container but no `data2.img`/raw disk appeared, and manually creating `data2.img` + restarting did not get dockur to attach it. The harness (compose + oem diskpart + run.sh provision + the test) is complete and would run on a clean-volume VM. The cross-drive property is otherwise covered borg-mechanically by `validate.ps1::borg_local_repo_via_unc` (UNC repo resolved from a different cwd — drive-independent).

## Devil's-Advocate Review Fixes
A `/devils-advocate` pass over the branch diff surfaced 7 action items. All applied (commit `bebf5a4`) and re-validated on the live VM (non-admin 3/3 PASS, admin Failed:0):

| # | Concern | Fix | Validated by |
|---|---|---|---|
| 1 | Preflight's PermissionDenied-only trigger might not fire for a real non-admin (unproven end-to-end) | Added `preflight_trigger_matches` edge check | **PASS**: `\\localhost\C$` raises ERROR_ACCESS_DENIED (HResult `-2147024891` = `0x80070005`) → Rust `PermissionDenied` → preflight fires |
| 2 | Blocking SMB stat ran on the async runtime | `precheck_repo` runs `local_repo_preflight()` via `tokio::spawn_blocking` | clippy + 132/27 unit tests; non-admin VM run |
| 3 | Non-PermissionDenied probe errors silently treated as "reachable" | `share_unreachable` logs raw OS errno on inconclusive probe | code review; trigger confirmed correct by #1 |
| 4 | `provision_edge` could collide with an in-use D: letter | Guard before `Initialize-Disk` | `bash -n run.sh` |
| 5 | `non_admin_local_repo_fast_fail` accepted a null exit code as success (couldn't tell "denied" from "never launched") | Now requires non-empty stderr | **PASS**: stderr `Permission denied to \\localhost\C$\...` |
| 6 | Preflight only on 5 of 10 commands (see Deviation 1) | Extended to all 10 via `precheck_repo` | grep: 10 call sites |
| 7 | Admin edge mode would FAIL (not skip) on a single-disk VM | Skip multi-drive checks when no D: | **PASS**: admin run reports Skipped:2, Failed:0 |

## Issues Encountered
- A non-ASCII em-dash would have broken PS parsing (prior lesson) — kept `validate-edge.ps1` ASCII; verified.
- `Test-Path \\localhost\C$` raises a (harmless, expected) access-denied error for a standard user — suppressed with `-ErrorAction SilentlyContinue` after the first run.
- dockur disk hot-add limitation (above) — not resolved; documented + offered as a clean-volume run.

## Tests Written
| Test File | Tests | Coverage |
|---|---|---|
| `crates/borg-core/src/config.rs` | 4 unit | unc_share_root extract/none; non_admin_message; preflight no-op for ssh+local-on-Linux |
| `tests/smoke-windows/validate-edge.ps1` | 5 checks | admin: second_drive_present, multi_drive_cross_restore; nonadmin: non_admin_admin_share_denied, preflight_trigger_matches, non_admin_local_repo_fast_fail |

## Next Steps
- [ ] (optional) Run the multi-drive half on a clean-volume VM: `docker compose down -v && make edge-all`
- [ ] `/code-review` then `/prp-pr`

> Devil's-advocate action items (7/7) applied + re-validated. Branch ready for PR.
