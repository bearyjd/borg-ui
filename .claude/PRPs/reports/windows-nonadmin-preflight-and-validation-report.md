# Implementation Report: non-admin preflight (plan 2) + non-admin/multi-drive validation (plan 4)

## Summary
Implemented two coupled plans together: (2) a Windows-only `RepoConfig::local_repo_preflight()` that turns the inaccessible-admin-share case into a clear "run as admin / use SSH" error instead of a cryptic borg failure, wired into the borg-running commands; and (4) a smoke-harness edge pass (`validate-edge.ps1` + `provision-edge`/`validate-edge`/`edge-all`) that validates the non-admin behavior and the multi-drive (repo on D:, restore to C:) case. The non-admin half was validated 2/2 on the real Windows VM; the multi-drive half is harness-complete but blocked from running here by a dockur disk-provisioning limitation (details below).

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
| 2.2 | Wire preflight into commands | ✅ Complete | **Deviation**: scoped to the 5 first-contact borg ops (get_repo_info, list_archives, init_repo, create_backup, restore_archive) rather than all 10 — the rest are reached only after list_archives, which preflights |
| 2.3 | Unit tests | ✅ Complete | 4 new (share-root extract/none, message, ssh+local no-op) |
| S1 | Second disk (`DISK2_SIZE`) + diskpart init | ✅ Implemented | Works on a *fresh* VM; dockur won't add it to a recreated persisted volume (see Deviations) |
| S2 | Standard user `borgstd` (oem + idempotent run.sh) | ✅ Complete | Created + SSH-validated on the VM |
| M1 | `multi_drive_cross_restore` | ✅ Implemented | Could not execute (no D: on the persisted VM) — see Deviations |
| N1/N2 | non-admin share-denied + fast-fail | ✅ Complete & **validated 2/2 on real Windows** | |
| W1 | run.sh/Makefile/README wiring | ✅ Complete | `provision-edge`, `validate-edge`, `edge-all` |

## Validation Results

| Level | Status | Notes |
|---|---|---|
| Static (fmt/clippy) | ✅ Pass | `clippy --workspace --all-targets -D warnings` clean |
| Unit Tests | ✅ Pass | borg-core 132 (4 new preflight tests); cfg-gating keeps Linux behavior identical |
| Build | ✅ Pass | workspace builds; `bash -n run.sh`; validate-edge.ps1 ASCII |
| Integration (real Windows) | ◑ Partial | **non-admin edge: 2/2 PASS** on the VM as `borgstd`; **multi-drive: blocked** (dockur didn't attach DISK2 to the recreated persisted volume) |
| Edge Cases | ✅ Pass (testable ones) | SSH/local/non-Windows preflight no-op; non-admin fast-fail (no hang) |

## Files Changed

| File | Action | Notes |
|---|---|---|
| `crates/borg-core/src/config.rs` | UPDATED | preflight method + 3 helpers + 4 tests |
| `app-tauri/src-tauri/src/commands.rs` | UPDATED | preflight wired into 5 borg-op entry points |
| `tests/smoke-windows/validate-edge.ps1` | CREATED | `-Mode admin|nonadmin` edge validation |
| `tests/smoke-windows/run.sh` | UPDATED | borgstd identity, `provision_edge`, `run_validate_edge`, subcommands |
| `tests/smoke-windows/docker-compose.yml` | UPDATED | `DISK2_SIZE: 8G` |
| `tests/smoke-windows/oem/install.bat` | UPDATED | `borgstd` + diskpart D: init (fresh VMs) |
| `tests/smoke-windows/Makefile` + `README.md` + `.gitignore` | UPDATED | targets, docs, `validate-edge.log` |
| `HANDOFF.md` | UPDATED | preflight done + edge-validation status |

## Deviations from Plan
1. **Preflight scope** (plan 2, Task 5): wired into the 5 first-contact borg commands, not all ~10. Rationale: the others (list_contents, prune, delete, diff, compact) are only reachable after `list_archives`, which preflights — so coverage is transitive with half the edit surface.
2. **Multi-drive not executed** (plan 4): the plan's top risk materialized — dockur/windows provisions a second disk only on a *fresh* install, not when recreating a container over a persisted `win-storage` volume. `DISK2_SIZE=8G` reached the container but no `data2.img`/raw disk appeared, and manually creating `data2.img` + restarting did not get dockur to attach it. The harness (compose + oem diskpart + run.sh provision + the test) is complete and would run on a clean-volume VM. The cross-drive property is otherwise covered borg-mechanically by `validate.ps1::borg_local_repo_via_unc` (UNC repo resolved from a different cwd — drive-independent).

## Issues Encountered
- A non-ASCII em-dash would have broken PS parsing (prior lesson) — kept `validate-edge.ps1` ASCII; verified.
- `Test-Path \\localhost\C$` raises a (harmless, expected) access-denied error for a standard user — suppressed with `-ErrorAction SilentlyContinue` after the first run.
- dockur disk hot-add limitation (above) — not resolved; documented + offered as a clean-volume run.

## Tests Written
| Test File | Tests | Coverage |
|---|---|---|
| `crates/borg-core/src/config.rs` | 4 unit | unc_share_root extract/none; non_admin_message; preflight no-op for ssh+local-on-Linux |
| `tests/smoke-windows/validate-edge.ps1` | 4 checks | second_drive_present, multi_drive_cross_restore, non_admin_admin_share_denied, non_admin_local_repo_fast_fail |

## Next Steps
- [ ] (optional) Run the multi-drive half on a clean-volume VM: `docker compose down -v && make edge-all`
- [ ] `/code-review` then `/prp-pr`
