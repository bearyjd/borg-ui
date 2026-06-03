# Plan: Validate the Windows local-repo fix — non-admin account + multi-drive

## Summary
The UNC local-repo fix (PR #31) was validated on the smoke VM, but two cases the
admin/single-disk VM couldn't exercise remain open: (1) a **standard (non-admin)**
account — confirm the inaccessible `\\localhost\C$` admin share yields a *fast,
clear failure*, not a hang; and (2) a **multi-drive** case — repo on `D:`, restore
to `C:`, confirming the UNC form's cross-drive restore (the case a relative repo
can't satisfy) actually round-trips. This plan extends `tests/smoke-windows/` with
a second disk, a standard user, and a `validate-edge.ps1` pass covering both.

## User Story
As a maintainer of the Windows local/USB-repo feature, I want the non-admin and
cross-drive cases validated on real Windows, so I can trust the UNC fix beyond the
single-disk admin VM it was first confirmed on.

## Problem → Solution
The fix's two riskiest claims are unverified: "a non-admin gets a fast error, not a
hang" and "cross-drive restore works." **→** Add a second NTFS volume (`D:`) and a
standard user (`borgstd`) to the VM, then a `validate-edge.ps1` that scripts:
`multi_drive_cross_restore` (repo on `D:` via `\\localhost\D$\…`, restore to `C:`,
byte-verify) and `non_admin_local_repo_fast_fail` (run as `borgstd` → admin share
denied → borg fails fast within a timeout, no hang).

## Metadata
- **Complexity**: Medium (harness/VM provisioning + one PowerShell validation script + run.sh/Makefile/README; no product code)
- **Source PRD**: N/A (free-form — the two manual follow-ups from PR #31 / HANDOFF)
- **PRD Phase**: N/A
- **Estimated Files**: 5 (`tests/smoke-windows/validate-edge.ps1` [new], `docker-compose.yml`, `oem/install.bat`, `run.sh`, `Makefile`/`README.md`)

---

## Relationship to other plans
- Builds on the merged fix in `crates/borg-core/src/config.rs` (`location()` → `to_windows_unc_local`).
- The **non-admin** case is the validation bed for `.claude/PRPs/plans/friendlier-non-admin-preflight.plan.md`. Until that preflight ships, this asserts the *borg-level* behavior ("fast access error, not a hang"); after it ships, extend the assertion to the app's friendly message (noted in Task N2).
- Mirrors the harness conventions from the merged `.claude/PRPs/plans/...` validation work (`validate.ps1`).

---

## Key constraints (read first)

1. **`\\localhost\D$` needs a real fixed NTFS volume.** Windows auto-creates the
   `X$` administrative share for each local fixed volume. A `subst`/mapped drive is
   NOT a volume and gets no `D$` share — so the multi-drive test **requires a real
   second virtual disk**, not a `subst` fake.
2. **Admin shares require admin, period.** `\\localhost\C$` is denied for a standard
   user *regardless* of the underlying folder's ACLs — that denial is exactly what
   the non-admin test confirms (fast, not a hang).
3. **The current VM is single-disk + admin-only** (`docker-compose.yml`: one disk;
   `oem/install.bat`: only `borgtest`, an Administrator). Both additions must be
   provisioned: `oem/install.bat` for a *fresh* VM, AND an idempotent `run.sh` setup
   step for the *already-warm* VM (`KEEP_VM=1`).
4. **`.ps1` files must be ASCII** (Windows PowerShell 5.1 reads UTF-8-without-BOM as
   ANSI; a non-ASCII char breaks parsing — learned the hard way in `validate.ps1`).

---

## Mandatory Reading

| Priority | File | Lines | Why |
|---|---|---|---|
| P0 | `tests/smoke-windows/validate.ps1` | all | Pass/Fail+JSON harness, `Invoke-Borg` (timeout-bounded), and esp. `borg_local_repo_via_unc` — the exact pattern `multi_drive_cross_restore` extends (repo on D: instead of C:) |
| P0 | `crates/borg-core/src/config.rs` | 84-137 | `location()` + `to_windows_unc_local` — the UNC rewrite both tests mirror (`X:\rest` → `\\localhost\X$\rest`) |
| P0 | `tests/smoke-windows/run.sh` | 22-23, 56-71, 121-161, 224-262 | `SSH_CMD`/`SCP_CMD` (single user today), `wait_for_ssh`, `run_validate`, the subcommand `case` — add a `borgstd` SSH identity + `run_validate_edge` + provisioning step |
| P0 | `tests/smoke-windows/docker-compose.yml` | 5-26 | Where to add the second disk + (if needed) a second storage volume; existing ports/mounts |
| P1 | `tests/smoke-windows/oem/install.bat` | 15-17 | The `net user borgtest … / net localgroup Administrators` pattern — add a NON-admin `borgstd` the same way (do NOT add to Administrators) |
| P1 | `app-tauri/src-tauri/src/commands.rs` | 197-225 | `init_repo` — the app path the non-admin assertion ultimately exercises (and where the friendly-preflight message will surface once that plan lands) |
| P2 | `crates/borg-core/src/borg.rs` | 127-156 | The non-interactive env vars borg needs (`BORG_PASSPHRASE`, `BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK`, etc.) — set them in the .ps1 so borg never blocks on a prompt |

## External Documentation

| Topic | Source | Key Takeaway |
|---|---|---|
| dockur/windows extra disks | github.com/dockur/windows | Additional disks are added via `DISK2_SIZE` (+ optional `/storage2` volume). The disk appears **raw** — Windows must `diskpart` it online → initialize → create primary → format NTFS → assign `D:`. |
| `diskpart` scripting | Microsoft docs | Bring a raw disk online and format non-interactively with a `diskpart /s script.txt` (`select disk 1` / `online disk` / `convert gpt` / `create partition primary` / `format fs=ntfs quick` / `assign letter=D`). |
| Windows administrative shares (`C$`/`D$`) | Microsoft docs | Auto-created per fixed volume; only Administrators can connect. A standard user → `ERROR_ACCESS_DENIED` (fast). |
| OpenSSH multiple local users | OpenSSH on Windows | `sshd` authenticates any local account; `borgstd` can SSH in with its own password. Default shell (PowerShell) is set machine-wide in `oem/install.bat:13`. |

KEY_INSIGHT: dockur's `DISK2_SIZE` exact name/behavior should be confirmed against the current dockur/windows README before relying on it.
APPLIES_TO: Task S1 (second disk).
GOTCHA: confirm the env var name + whether a `/storage2` volume mount is needed; the disk is raw and needs diskpart.

KEY_INSIGHT: a standard user hits `\\localhost\C$` denial immediately (SMB over loopback returns ACCESS_DENIED fast).
APPLIES_TO: Task N2 — bound it with a timeout anyway, so a (regression) hang is caught as a failure.

---

## UX Design
N/A — validation/verification only. No product UX change. (It *confirms* the UNC fix's two unverified claims.)

---

## Patterns to Mirror

### PS_VALIDATION_HARNESS + UNC ROUND-TRIP (extend this for multi-drive)
```powershell
# SOURCE: tests/smoke-windows/validate.ps1  (borg_local_repo_via_unc)
$absRepo = Join-Path $work "unc_repo"
$uncRepo = "\\localhost\" + $absRepo.Substring(0,1) + "$" + $absRepo.Substring(2)   # X:\rest -> \\localhost\X$\rest
$r = Invoke-Borg @("init","--encryption","none",$uncRepo) 40
$r = Invoke-Borg @("create","$uncRepo::a1","unc_src") 60 $work
$r = Invoke-Borg @("extract","$uncRepo::a1") 60 $out
# byte-verify restored file via Get-FileHash
```

### PS_TIMEOUT_LAUNCH (Invoke-Borg — bounded; never hangs the run)
```powershell
# SOURCE: tests/smoke-windows/validate.ps1 (Invoke-Borg)
$p = Start-Process -FilePath $borgExe -ArgumentList $args -WindowStyle Hidden -PassThru -RedirectStandardOutput $o -RedirectStandardError $e
if (-not $p.WaitForExit($TimeoutSec*1000)) { $p.Kill(); <TimedOut=true> } else { $p.WaitForExit(); <ExitCode> }
```

### NON_INTERACTIVE_BORG_ENV
```powershell
# SOURCE: crates/borg-core/src/borg.rs:132-151 (base_command_with)
$env:BORG_PASSPHRASE=""; $env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK="yes"
$env:BORG_DISPLAY_PASSPHRASE="no"; $env:BORG_RELOCATED_REPO_ACCESS_IS_OK="yes"
```

### RUNSH_SUBCOMMAND + SSH IDENTITY
```bash
# SOURCE: tests/smoke-windows/run.sh:22-23, run_validate()
SSH_CMD="sshpass -p $SSH_PASS ssh -o StrictHostKeyChecking=no ... -p $SSH_PORT $SSH_USER@$SSH_HOST"
# add: SSH_CMD_STD for borgstd; run_validate_edge() scp's validate-edge.ps1 and runs the
# admin part as borgtest and the non-admin probe as borgstd, greps "Failed: 0".
```

### OEM_USER_PROVISION
```bat
REM SOURCE: tests/smoke-windows/oem/install.bat:16-17
net user borgtest Password1! /add
net localgroup Administrators borgtest /add      REM <- borgstd OMITS this line (stays standard)
```

### ASCII_ONLY
```text
# SOURCE: HANDOFF.md — keep validate-edge.ps1 ASCII; non-ASCII breaks PS 5.1 parsing.
```

---

## Files to Change

| File | Action | Justification |
|---|---|---|
| `tests/smoke-windows/docker-compose.yml` | UPDATE | Add a second disk (`DISK2_SIZE`) so `D:` exists for the multi-drive test |
| `tests/smoke-windows/oem/install.bat` | UPDATE | First-boot: add standard user `borgstd` (no Administrators) + a diskpart init for the 2nd disk (fresh VMs) |
| `tests/smoke-windows/run.sh` | UPDATE | Idempotent provisioning for the warm VM (ensure `borgstd` + `D:`); a `borgstd` SSH identity; `run_validate_edge` + `validate-edge`/`edge-all` subcommands |
| `tests/smoke-windows/validate-edge.ps1` | CREATE | `multi_drive_cross_restore` (admin) + a `non_admin_probe` body invoked as `borgstd`; Pass/Fail/JSON like `validate.ps1` |
| `tests/smoke-windows/Makefile` + `README.md` | UPDATE | `make validate-edge` / `make edge-all`; document the 2nd disk, the standard user, and what each case proves |
| `HANDOFF.md` | UPDATE | Tick the two follow-ups once confirmed |

## NOT Building
- **No product code change** — pure validation. (The non-admin *friendly message* is a separate plan; here we validate borg-level fast-fail and leave a hook to extend once that lands.)
- **No `subst` fake drive** — it has no `D$` admin share, so it wouldn't exercise the UNC path faithfully (see Key Constraints #1).
- **No CI gating** — needs VM provisioning (2nd disk, extra user); stays an operator-run `make edge-all`.
- **No re-validation of the admin/same-drive path** — already green via `validate.ps1::borg_local_repo_via_unc`.
- **No GUI/app-level run** — borg-level assertions only (the app uses the same `location()` form, already covered).

---

## Step-by-Step Tasks

### Shared provisioning

#### Task S1: Add a second disk and initialize it as `D:`
- **ACTION**: Add `DISK2_SIZE: "8G"` to `docker-compose.yml` env (and a `/storage2` volume if dockur requires one — confirm against dockur/windows docs). Add a diskpart init that runs once: bring disk 1 online, GPT, primary partition, NTFS quick format, assign `D:`.
- **IMPLEMENT (fresh VM)**: in `oem/install.bat`, write a diskpart script and run `diskpart /s`. **(warm VM)**: an idempotent `run.sh` step over SSH that checks `Test-Path D:\` and, if absent, runs the same diskpart script.
- **MIRROR**: External Documentation (diskpart scripting).
- **GOTCHA**: The new disk is **raw**; if not initialized, `D:` won't exist. Make the init idempotent (skip if `D:\` already present). The disk index may not be `1` — select by "not the system disk" or size. Confirm `DISK2_SIZE` against dockur docs (External Documentation note).
- **VALIDATE**: over SSH, `Test-Path D:\` is `True`; `(Get-Volume -DriveLetter D).FileSystem` is `NTFS`; `Test-Path \\localhost\D$\` is `True` (admin share auto-created).

#### Task S2: Add a standard (non-admin) user `borgstd`
- **ACTION**: Create `borgstd` with a known password, NOT in Administrators.
- **IMPLEMENT (fresh)**: in `oem/install.bat`, `net user borgstd Password1! /add` (and explicitly NOT `net localgroup Administrators borgstd /add`). **(warm)**: idempotent `run.sh` step (`net user borgstd` check; create if missing).
- **MIRROR**: OEM_USER_PROVISION (omit the Administrators line).
- **GOTCHA**: `borgstd` must be able to read/execute `C:\borg\borg.exe` (default `Users` ACL on `C:\borg` allows read+execute — verify; if not, the test can copy borg.exe into borgstd's own `%TEMP%`). `borgstd` writes only to its own `%TEMP%`.
- **VALIDATE**: `sshpass -p Password1! ssh borgstd@localhost -p 2222 'whoami'` returns `…\borgstd`; `net localgroup Administrators` does NOT list borgstd.

### Multi-drive case (runs as admin `borgtest`)

#### Task M1: `multi_drive_cross_restore` in `validate-edge.ps1`
- **ACTION**: Repo on `D:` (via the UNC rewrite), source on `C:`, restore to `C:\…\out` — a full round-trip across drives.
- **IMPLEMENT**: `$absRepo = "D:\borgui-edge\repo"`; `$uncRepo = "\\localhost\D$\borgui-edge\repo"` (mirror `location()`); make a `C:\…\src\data.txt`; `Invoke-Borg init $uncRepo`; `Invoke-Borg create "$uncRepo::a1" src` (cwd = C: work dir); `Invoke-Borg extract "$uncRepo::a1"` (cwd = `C:\…\out`, a DIFFERENT drive than the repo); byte-verify restored file == source via `Get-FileHash`. Pass/Fail `multi_drive_cross_restore`.
- **MIRROR**: PS_VALIDATION_HARNESS + UNC ROUND-TRIP; NON_INTERACTIVE_BORG_ENV; PS_TIMEOUT_LAUNCH.
- **GOTCHA**: This is the exact property the relative-path form could NOT satisfy (extract cwd on C:, repo on D:). The UNC repo is absolute/location-independent so it must resolve regardless of cwd drive. Clean up `D:\borgui-edge` + the C: work dir in `finally`.
- **VALIDATE**: `make validate-edge` → `multi_drive_cross_restore` PASS with restored byte-match.

### Non-admin case (the probe body runs as `borgstd`)

#### Task N1: `non_admin_admin_share_denied`
- **ACTION**: As `borgstd`, confirm the admin share is inaccessible (the precondition for the friendly-error case).
- **IMPLEMENT**: in the borgstd-run section, `Test-Path \\localhost\C$\` (expect `False` / access denied) and `Get-Item \\localhost\C$\ -ErrorAction SilentlyContinue` is null. Pass/Fail `non_admin_admin_share_denied` (PASS when denied — that's the expected non-admin state).
- **MIRROR**: PS_VALIDATION_HARNESS.
- **GOTCHA**: A FALSE here (share reachable) would mean borgstd is unexpectedly privileged — fail the test (the user isn't actually standard). Confirms the test setup is valid.
- **VALIDATE**: PASS = share denied for borgstd.

#### Task N2: `non_admin_local_repo_fast_fail`
- **ACTION**: As `borgstd`, run borg `init` against the UNC form the app produces for a local repo (`\\localhost\C$\<borgstd-temp>\repo`) and confirm it fails **fast** (non-zero exit within a short timeout), NOT a hang.
- **IMPLEMENT**: `Invoke-Borg init \\localhost\C$\…\repo` with a tight timeout (e.g. 25s). Assert `TimedOut == $false` AND `ExitCode != 0` AND the repo was NOT created. Pass/Fail `non_admin_local_repo_fast_fail` (PASS = fast non-zero failure).
- **MIRROR**: PS_TIMEOUT_LAUNCH; NON_INTERACTIVE_BORG_ENV.
- **GOTCHA**: This validates the UNC fix's non-admin claim ("a fast error, not a hang"). **A `TimedOut == true` is a regression** (the hang is back) → FAIL. **EXTENSION (once the friendly preflight ships):** add an assertion that the *app's* `init_repo` command (or `RepoConfig::local_repo_preflight`) returns the friendly admin/SSH message — not just borg's raw error. Until then, the borg-level fast-fail is the assertion.
- **VALIDATE**: PASS = bounded non-zero failure (no timeout, no repo created).

### Wiring

#### Task W1: `run_validate_edge` + subcommands + Makefile/README
- **ACTION**: Add a `borgstd` SSH identity (`SSH_CMD_STD`) to `run.sh`; `run_validate_edge()` that (a) ensures provisioning (S1/S2 idempotent), (b) scp's `validate-edge.ps1`, (c) runs the admin part as `borgtest` and the non-admin probe as `borgstd`, (d) merges results and greps `Failed: 0`. Add `validate-edge` (warm VM) and `edge-all` (boot → ssh → provision → validate) cases. Add `make validate-edge`/`make edge-all`. Document in README.
- **MIRROR**: RUNSH_SUBCOMMAND + SSH IDENTITY; the existing `run_validate`/`validate`/`validate-all` structure.
- **GOTCHA**: Two SSH identities (borgtest admin, borgstd standard). The non-admin probe must run under `borgstd`'s session (its own `%TEMP%`). Bound everything; clean up disk/user artifacts the test created (but leave `D:` + `borgstd` provisioned for re-runs).
- **VALIDATE**: `bash -n run.sh`; `make validate-edge` runs end-to-end on the warm VM and reports a `Failed: N` summary.

---

## Testing Strategy

### Automated assertions (in `validate-edge.ps1`)
| Test | Runs as | Input | Expected | Edge case? |
|---|---|---|---|---|
| `multi_drive_cross_restore` | borgtest (admin) | repo `\\localhost\D$\…`, src on C:, restore to C: | full round-trip; restored bytes == source | yes — cross-drive (the relative-repo failure case) |
| `non_admin_admin_share_denied` | borgstd (standard) | `Test-Path \\localhost\C$\` | denied/`False` | yes — confirms the non-admin precondition |
| `non_admin_local_repo_fast_fail` | borgstd (standard) | `borg init \\localhost\C$\…\repo` (25s bound) | fast non-zero exit, no timeout, no repo | yes — the anti-hang regression guard |

### Edge Cases Checklist
- [x] Repo and restore destination on different drives (multi-drive core)
- [x] Standard user → admin share denied (fast)
- [x] Standard user → borg local repo fails fast, NOT a hang (regression guard)
- [x] All borg calls timeout-bounded; artifacts cleaned in `finally`
- [x] `.ps1` ASCII-only
- [ ] App-level friendly message for non-admin — deferred to the preflight plan (extension hook in N2)

---

## Validation Commands

### Static / hygiene (Linux, before deploy)
```bash
grep -nP '[^\x00-\x7F]' tests/smoke-windows/validate-edge.ps1 && echo "NON-ASCII!" || echo "ASCII OK"
bash -n tests/smoke-windows/run.sh
```
EXPECT: ASCII OK; run.sh parses.

### Provision + run on the VM
```bash
cd tests/smoke-windows
# warm VM (KEEP_VM=1): provisioning is idempotent inside run_validate_edge
KEEP_VM=1 make validate-edge
# or from cold: KEEP_VM=1 make edge-all   (boot -> ssh -> provision D: + borgstd -> validate)
```
EXPECT: `multi_drive_cross_restore` PASS; `non_admin_admin_share_denied` PASS; `non_admin_local_repo_fast_fail` PASS; summary `Failed: 0`.

### Manual sanity (optional, VNC `localhost:8006`)
- [ ] `Get-Volume` shows `C` and `D` (NTFS); `net localgroup Administrators` lists `borgtest` but not `borgstd`.

---

## Acceptance Criteria
- [ ] A second NTFS volume `D:` exists on the VM with a working `\\localhost\D$` admin share.
- [ ] A standard (non-admin) user `borgstd` exists and can SSH in.
- [ ] `multi_drive_cross_restore` passes: repo on `D:`, restore to `C:`, byte-identical.
- [ ] `non_admin_local_repo_fast_fail` passes: a standard user gets a fast non-zero failure (no hang) on a local-repo init.
- [ ] `make validate-edge` is `Failed: 0`; HANDOFF follow-ups ticked.

## Completion Checklist
- [ ] `validate-edge.ps1` mirrors `validate.ps1` (Pass/Fail/JSON/exit), ASCII-only
- [ ] Provisioning idempotent for both fresh (`oem/install.bat`) and warm (`run.sh`) VMs
- [ ] Non-admin probe genuinely runs as `borgstd` (verified via `whoami`)
- [ ] Timeouts on all borg calls; created artifacts cleaned up
- [ ] No product code change; no CI gating
- [ ] README documents the 2nd disk + standard user + what each case proves
- [ ] HANDOFF "before first production use" / open items updated

## Risks
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| dockur `DISK2_SIZE` var name / behavior differs | Medium | Medium | Confirm against dockur/windows docs (External Documentation note); the diskpart init is generic once a raw disk exists |
| 2nd disk not auto-initialized → `D:` missing | High (raw disk) | High | Idempotent diskpart init (online/GPT/format/assign) in provisioning; validate `Test-Path D:\` |
| `borgstd` can't read `C:\borg\borg.exe` (ACLs) | Low | Medium | Default `Users` ACL allows read+exec; fallback: copy borg.exe into borgstd `%TEMP%` |
| Non-admin borg call hangs instead of fast-failing | Low | Medium (would be a real regression) | Timeout-bound it; a timeout is a FAIL surfacing the regression |
| Modifying `docker-compose.yml`/`oem` forces a fresh VM provision | Medium | Low | Keep warm-VM provisioning in `run.sh` idempotent; `oem` changes only affect the next fresh boot |

## Notes
- This closes the two "manual hardware checks" flagged when the UNC fix (PR #31) merged: non-admin behavior and cross-drive restore — the cases the admin/single-disk VM couldn't reach.
- Highest-value ordering: provision (S1/S2) → `multi_drive_cross_restore` (proves the fix's headline cross-drive property) → non-admin fast-fail (regression guard + bed for the preflight plan).
- Pairs with `friendlier-non-admin-preflight.plan.md` (extends N2 to assert the friendly message) and `fix-windows-local-repo-path.plan.md` (the fix being validated).
