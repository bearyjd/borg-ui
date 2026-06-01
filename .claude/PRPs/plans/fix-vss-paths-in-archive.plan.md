# Plan: Fix VSS Shadow-Copy Paths in Borg Archives

## Summary

Backups created with VSS snapshots store paths like `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy2\Users\me\docs\file.txt` verbatim in the borg archive. On restore, Windows refuses to create files at those paths because `?` is an illegal filename character — `borg extract` aborts with the I/O errors and 0 files land on disk. VSS is currently **disabled** in `commands.rs::create_backup` via a `FIXME` so backups complete and restore works, at the cost of losing the locked-file consistency guarantee. This plan restores VSS while ensuring stored paths are clean drive-relative paths (`Users/me/docs/file.txt`) that restore cleanly.

## User Story

As a BorgUI user backing up open files (Outlook PST, SQL databases, in-use Office documents), I want VSS snapshot consistency without sacrificing the ability to restore from the archive on the same machine or another Windows host.

## Problem → Solution

VSS shadow-copy paths in archive are restore-blockers because `?` is invalid in NTFS filenames → mount the shadow as a DOS-namespace path (drive letter or junction) before passing to borg, so stored paths are normal `C:\Users\...`-style or volume-relative.

## Metadata

- **Complexity**: Medium
- **Source PRD**: N/A — discovered during /smoke testing of archive-browsing feature
- **PRD Phase**: N/A
- **Estimated Files**: 4
- **Confidence**: 6/10 — multiple plausible approaches, each with tradeoffs

---

## Symptoms / Reproduction

1. Configure repo, init unencrypted
2. Backup any folder on `C:` from the app
3. Inspect: `borg list <repo>::<archive>` shows paths like `?/GLOBALROOT/Device/HarddiskVolumeShadowCopy2/borgui-test/tests/foo.txt`
4. Restore (full or selective) → `borg process failed: borg extract failed` and zero files at destination
5. Verified on tiny11 + marcpope/borg-windows 1.4.4+win6

## Investigation Done

| Attempt | Outcome |
|---|---|
| Set borg `current_dir` to `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN` and pass volume-relative paths | Windows rejects NT-namespace paths as a process CWD — `SetCurrentDirectory` returns `ERROR_DIRECTORY (267)`, backup fails with "The directory name is invalid" |
| Confirmed `\\?\GLOBALROOT\…` shadow path returned by `Win32_ShadowCopy.DeviceObject` is canonical (no alternate DOS form is automatically created by VSS) | Need to *explicitly* expose the shadow as a DOS path |

## Approaches to Evaluate

### A. `subst` virtual drive letter

After snapshot, run `subst Z: \\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN\` (find first free letter), use `Z:\Users\me\docs` as source. After backup, `subst /D Z:`.

- **Pros**: Tiny, no API calls, works on all Windows since XP.
- **Cons**: `subst` historically refuses NT-namespace targets in some configurations. Drive letter is process-global — race condition if another backup runs concurrently. Z: appears in stored paths (e.g. `Z:/Users/me/docs/...`), which still contains `:` (illegal in filenames on extract → same restore failure).
- **Verdict**: Likely doesn't fix the underlying `:`-in-stored-path issue. Skip.

### B. NTFS junction (mklink /J)

`mklink /J C:\BorgVSSMount<id> \\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN`. Use that path as a DOS-friendly CWD for borg, pass volume-relative source paths.

- **Pros**: Junctions are well-supported; CWD set to a normal `C:\...` path works fine.
- **Cons**: Junction target as `\\?\GLOBALROOT\…` may not be valid (junctions traditionally point to local mount points, not arbitrary NT device paths). Needs testing.
- **Verdict**: Worth trying — `cmd /c mklink /J` followed by setting CWD to that path and passing relative source paths.

### C. `IVssBackupComponents::ExposeSnapshot` via Win32 COM

Official VSS API: expose the snapshot as a drive letter or mount point. Returned path is a real DOS path borg can chdir into.

- **Pros**: Canonical Microsoft-supported approach. Used by every VSS-aware backup tool.
- **Cons**: Requires COM in Rust (`windows-rs` crate's `Win32_Storage_Vss`). More code, more dependencies. The `vss.rs` module currently shells out to PowerShell for `Win32_ShadowCopy.Create` — switching to COM affects that surface too.
- **Verdict**: Right long-term answer. Bigger lift.

### D. Drop VSS entirely

Document VSS as unsupported on Windows; rely on live-file backup. Note that open files may be skipped or partially captured.

- **Pros**: Simplest. Matches what we ship today via the `FIXME`.
- **Cons**: Loses the marketed differentiator vs Vorta (the README and TODO.md call out "VSS snapshots for consistent backup of locked files" as a BorgUI advantage).
- **Verdict**: Acceptable interim, not the final answer.

## Recommended Sequence

1. **Validate Approach B (junction)** as a quick win. Single PowerShell command, easy to test in the smoke VM. If it works, ship.
2. **If junction won't accept the NT target**, fall back to Approach C (COM `ExposeSnapshot`). Adds `windows` crate Vss features; rewrites the create_snapshot helper.
3. **Either way**, keep the current FIXME safety net: if mount/expose fails for any reason, fall through to non-VSS live backup with a `tracing::warn!`.

## Mandatory Reading

| Priority | File | Why |
|---|---|---|
| P0 | `crates/borg-platform-win/src/vss.rs` | Current snapshot + remap impl |
| P0 | `app-tauri/src-tauri/src/commands.rs::create_backup` | Current FIXME bypass |
| P0 | `crates/borg-core/src/borg.rs::create` | `cwd` parameter (already in place from the failed attempt) |
| P1 | This file | Investigation context |
| P1 | `crates/borg-platform-win/Cargo.toml` | Add `windows = { version = "...", features = ["Win32_Storage_Vss"] }` if going COM route |

## Implementation Tasks

### For Approach B (junction)

1. In `vss.rs`, add `mount_snapshot(handle: &SnapshotHandle) -> Result<PathBuf>` that:
   - Picks a unique temp dir under `%TEMP%\\BorgVSS-<snapshot-id-prefix>`
   - Shells out: `cmd /c mklink /J <temp> <shadow_path>`
   - Returns the temp dir path
2. Add `unmount_snapshot(path: &Path)` that `cmd /c rmdir <path>` (removes the junction, not the shadow).
3. Re-introduce `SnapshotPlan { paths, cwd, handles, mounts }` returned from `snapshot_sources`. For single-volume successful snapshot+mount: cwd = mount point, paths = volume-relative. For multi-volume or any failure: live-files fallback.
4. Wire into `commands.rs::create_backup` (replace the FIXME).
5. Always unmount on completion (success or failure).
6. Tests: unit-mock `mount_snapshot`, integration where possible.

### For Approach C (COM ExposeSnapshot) — if B fails

1. Add `windows` crate to `borg-platform-win/Cargo.toml` with `Win32_Storage_Vss` feature.
2. Replace PowerShell `Win32_ShadowCopy.Create` with `IVssBackupComponents` directly (`InitializeForBackup`, `StartSnapshotSet`, `AddToSnapshotSet`, `DoSnapshotSet`).
3. Use `ExposeSnapshot` with `VSS_VOLSNAP_ATTR_EXPOSED_LOCALLY` to get a DOS mount path.
4. Same `SnapshotPlan` shape as Approach B in the caller.

## Acceptance Criteria

- [ ] Backup of `C:\foo` from the app stores paths like `foo/bar.txt` (or `Users/.../foo/bar.txt`) — no `?` or `GLOBALROOT` in `borg list` output
- [ ] Restore (full and selective via Browse → Restore selected) actually writes files to the destination
- [ ] Multi-volume backups still complete (with non-VSS fallback) and log a warning
- [ ] VSS snapshot is released even on backup failure
- [ ] All existing `vss.rs` tests pass
- [ ] New test: snapshot_sources falls back to live paths when mount fails

## Test Plan

- **Unit**: mock the mount step, assert plan shape (cwd set vs None, paths relative vs absolute)
- **Integration (smoke VM)**:
  1. Backup `C:\borgui-test\tests` → archive stored with clean paths
  2. `borg list <archive>` confirms no shadow-copy prefix
  3. Restore subset via Browse modal to fresh empty dir → files appear
  4. Quit app between snapshot release and mount unmount — verify no leftover junctions on next launch (or document the leak and how to clean up)

## Risks

- Junctions to NT device paths may not be supported on all Windows versions (tested on tiny11; not validated on Windows 11 Pro / Server).
- `subst` and `mklink` both leave artifacts if process crashes mid-backup. Need cleanup on next startup.
- COM route adds a non-trivial Rust dep and increases binary size.

## Out of Scope

- Multi-volume VSS (single `IVssBackupComponents` session can do multi-volume, but coordinating multiple borg invocations against one archive isn't supported by borg — defer).
- Restoring an archive made *before* this fix. Old archives with `\\?\GLOBALROOT\…` paths remain un-restorable; users will need to re-backup.
