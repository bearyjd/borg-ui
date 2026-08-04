# Fix: local repository paths hang on Windows (borg drive-letter misparse)

> **SUPERSEDED — do not execute this plan (noted 2026-08-04).** The UNC
> workaround below shipped, then was **removed** in **#69** (`1941fc4`, "use
> native Windows repository paths") because upstream marcpope/borg-windows
> released **1.4.4+win7**, which parses drive-letter repository paths correctly.
> The root cause is gone, so the workaround is not just unnecessary — applying it
> today would be wrong.
>
> Current state: `RepoConfig::location()` returns `repo_path` verbatim for local
> repos (`crates/borg-core/src/config.rs:92`), and `local_repo_preflight()` is a
> no-op stub retained only as an API hook for callers. The regression test this
> plan cites (`borg_local_repo_via_unc`) was removed with the workaround and is
> replaced by `borg_local_drive_repo` in `validate.ps1` (raw drive-letter repo:
> init → create → cross-cwd extract → byte-verify), plus `borg_win7_install` and
> `non_admin_admin_share_not_required` in `validate-edge.ps1` — the latter
> asserting the admin share is no longer required at all.
>
> **Live dependency worth knowing:** the code now assumes borg ≥ 1.4.4+win7.
> `local_repo_preflight()` returns `Ok(())` unconditionally, so a regression to
> win6 would bring back the hang with no preflight to catch it. The guard is the
> version pin in `.github/workflows/release.yml` and the `1.4.4+win7` assertion in
> `validate-edge.ps1` — smoke-only. Re-check both if the borg pin ever moves.
>
> Everything below is the original 2026-06 record, kept as history.

**Status:** ✅ FIXED via Option 2 (UNC), validated on the KVM VM 2026-06-02 — `make validate` is 5/5 green. `RepoConfig::location()` now rewrites a local Windows drive-letter path `X:\rest` → `\\localhost\X$\rest` (admin-share UNC), which borg accepts as local. Regression test: `tests/smoke-windows/validate.ps1` → `borg_local_repo_via_unc` (full UNC round-trip). Remaining follow-ups: (1) report upstream to marcpope/borg-windows (filed); (2) a friendlier non-admin preflight (a standard user currently gets a fast borg access error, no longer a hang).

## Problem

The "Local folder / USB drive / network folder" repository feature (#22) almost
certainly **hangs on Windows**. The app's `RepoConfig::location()`
(`crates/borg-core/src/config.rs`) returns `repo_path` verbatim for a local repo,
so a user-picked folder like `C:\Backups\myrepo` is passed straight to borg.

## Root cause

The bundled `borg.exe` (marcpope/borg-windows **1.4.4+win6**, the latest release)
parses a repository argument with a drive-letter colon as an **SSH remote**:
`C:\Backups\repo` → `host="C"`, and borg then tries to `ssh` to host "C", which
spins/hangs (`ssh: Could not resolve hostname c`). borg upstream has Windows
drive-letter detection for the repo location; this fork's is absent/broken.

Note the asymmetry: borg **does** handle drive letters in *source* and
*extract-target* paths (the README documents `C:\Users\file` being stored as
`C/Users/file`). Only the **repository-location argument** misparses.

## Evidence matrix (all tested on the VM against borg 1.4.4+win6)

| Repo argument form | Result |
| --- | --- |
| `C:\repo`, `C:/repo`, `\\?\C:\repo` | parsed as ssh host "C" → **hang** |
| `file:///C:/repo`, `file://localhost/C:/repo` | `file://` not stripped → mangled to `<cwd>\file:\C:\…` → `WinError 123` |
| relative `repo` (no colon), cwd fixed | init/create OK, but `extract` must run with cwd = restore destination, where a relative repo resolves wrong (`Repository …\dest\repo does not exist`) |
| **leading-slash `/repo`** (colon-free abs, = current-drive root) | ✅ full init→create→extract→byte-verify round-trip. Location-independent **within a drive** (cwd's drive). |
| **UNC `\\localhost\C$\…\repo`** (and `//localhost/C$/…`) | ✅ init OK. Location-independent for **any** drive, but uses the **admin `C$` share** (needs an admin account). |

## Options (each has a real tradeoff)

1. **Upstream fix (cleanest, slowest).** Report to marcpope/borg-windows: the
   repo-location parser should detect `X:\`/`X:/` as a local Windows path.
   Then `location()` needs no change. Best long-term; out of our control.

2. **UNC workaround in `location()` (only general app-side fix).** On Windows,
   convert a local `X:\rest` → `\\localhost\X$\rest`. Location-independent, works
   cross-drive (so restore-to-another-drive works). **Caveat:** relies on the
   administrative `X$` share — fine for an admin account (personal Windows
   usually is), fails for a standard user. Would need a clear UI caveat / a
   preflight check.

3. **Leading-slash workaround (no admin, not general).** Convert `X:\rest` →
   `/rest` and run borg with cwd on drive X. Works for same-drive backup AND
   restore, but **cross-drive restore breaks** (extract's cwd must be the
   destination, which fixes the drive `/rest` resolves against). Acceptable only
   if we constrain repo and restore target to the same drive.

4. **Restrict Windows to SSH repos (interim).** Hide/disable local repos on
   Windows until 1–3 lands. Safe but removes a shipped feature.

## Recommendation

Pursue **(1) upstream** as the real fix, and in the meantime implement **(2) UNC**
behind a Windows-only `location()` branch with a preflight check that the target
share is reachable (falling back to a clear error, never a hang). Keep the
non-interactive env vars; they don't affect this. Whatever lands, the
`borg_local_absolute_repo` regression test in `validate.ps1` must go green.

## Validation

Re-run `make validate` (VM warm) after any change; `borg_local_absolute_repo`
must pass (init at an absolute local path completes < 40s and creates the repo),
and a full backup→restore-to-a-different-drive should round-trip. The borg engine
itself is already confirmed working on Windows (`borg_engine_create` passes).
