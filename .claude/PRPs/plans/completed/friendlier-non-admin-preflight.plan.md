# Plan: Friendlier non-admin preflight for local Windows repos

## Summary
The Windows local-repo fix (#31) rewrites a local drive-letter path `C:\repo` to the admin-share UNC `\\localhost\C$\repo` so borg treats it as local. That share requires an **administrator account**; a standard (non-admin) user now gets borg's cryptic access error (`WinError`/access denied) instead of a hang. This adds a **preflight** that detects the definitively-inaccessible admin-share case and returns a clear, actionable message ("run as admin, or use an SSH repo") before borg runs.

## User Story
As a non-admin Windows user configuring a local backup folder, I want a clear explanation when BorgUI can't reach my chosen repo location, so that I know to run as an administrator or use an SSH repository — instead of seeing a cryptic borg error.

## Problem → Solution
Non-admin user on a local repo → borg fails with a cryptic `\\localhost\C$` access error (or, if `location()` were bypassed, a hang). **→** A Windows-only preflight on local repos checks the admin-share root and, *only when it is definitively inaccessible*, returns a friendly `InvalidConfig` error with guidance, surfaced by the existing error UI.

## Metadata
- **Complexity**: Small (1 core file + N command call sites; ~80 lines incl. tests)
- **Source PRD**: N/A (free-form follow-up from PR #31 / HANDOFF "Open items")
- **PRD Phase**: N/A
- **Estimated Files**: 2 (`crates/borg-core/src/config.rs`, `app-tauri/src-tauri/src/commands.rs`)

---

## UX Design

### Before
```
Settings → local repo C:\Backups\repo  (standard, non-admin user)
  → "Create repository" / first backup
  → Error: borg create failed: [WinError 5] Access is denied: '\\localhost\C$\...'
            (cryptic; user has no idea it's an admin-share/permissions issue)
```

### After
```
Settings → local repo C:\Backups\repo  (standard, non-admin user)
  → "Create repository" / first backup
  → Error: "BorgUI can't access this local folder without administrator rights
            (it reaches local drives via the \\localhost\C$ share). Run BorgUI
            as an administrator, or use an SSH repository instead."
```

### Interaction Changes
| Touchpoint | Before | After | Notes |
|---|---|---|---|
| Create repo / backup / any borg op on a local repo (non-admin) | cryptic borg `WinError` | clear guidance message | Same error-surfacing UI; only the message text differs |
| Admin user, or SSH repo, or non-Windows | unchanged | unchanged | Preflight is a no-op in these cases |

---

## Mandatory Reading

| Priority | File | Lines | Why |
|---|---|---|---|
| P0 | `crates/borg-core/src/config.rs` | 84-137 | `location()` and `to_windows_unc_local()` — the UNC rewrite this preflight pairs with; the new code lives here |
| P0 | `crates/borg-core/src/config.rs` | 29-72 | `validate()` — the existing pure per-repo check the preflight sits beside (do NOT add IO to `validate`) |
| P0 | `crates/borg-core/src/error.rs` | 24-25, 85-90 | `BorgError::InvalidConfig { message }` — the variant + Display used for config errors |
| P1 | `app-tauri/src-tauri/src/commands.rs` | 114-126, 197-225, 243-298 | `get_repo_info`, `init_repo`, `create_backup` — the `repo.validate().map_err(|e| e.to_string())?` entry-point pattern to mirror |
| P1 | `crates/borg-core/src/config.rs` | 531-587 | existing `#[cfg(test)]` `local_repo()` helper + `location`/`is_local` tests to mirror for new tests |
| P2 | `crates/borg-core/src/proc.rs` | 1-30 | example of the `#[cfg(windows)]` / `#[cfg(any(windows, test))]` gating idiom used in this repo |

## External Documentation

| Topic | Source | Key Takeaway |
|---|---|---|
| Windows admin shares (`C$`) | Microsoft docs | `\\<host>\C$` is an *administrative* share — only members of the local Administrators group can access it. A standard user gets ERROR_ACCESS_DENIED (5). This is exactly the case to detect. |
| `std::io::ErrorKind::PermissionDenied` | Rust std | `std::fs::metadata` on an inaccessible admin share maps `ERROR_ACCESS_DENIED` to `PermissionDenied`. Use it as the "definitively inaccessible" signal; treat other errors (e.g. `NotFound`) as ambiguous and do NOT block. |

KEY_INSIGHT: Admin-share access over loopback depends on group membership + UAC token filtering; it can be finicky.
APPLIES_TO: the probe in `share_unreachable`.
GOTCHA: To avoid regressing a setup that would actually work, the preflight must be **conservative** — only return the friendly error on a *definitive* `PermissionDenied`. On success or any other/ambiguous error, return `Ok(())` and let borg run (it now fails fast, not hangs).

---

## Patterns to Mirror

### NAMING_CONVENTION (module-private helpers, cfg-gated)
```rust
// SOURCE: crates/borg-core/src/config.rs:117-137
#[cfg(any(windows, test))]
fn to_windows_unc_local(path: &str) -> String { ... }
```

### ERROR_HANDLING (typed config error)
```rust
// SOURCE: crates/borg-core/src/config.rs:30-34
return Err(BorgError::InvalidConfig {
    message: "repo_path cannot be empty".into(),
});
```

### CFG GATING (Windows-only call, pure helper kept for tests)
```rust
// SOURCE: crates/borg-core/src/config.rs:84-101 (location)
#[cfg(windows)]
{
    to_windows_unc_local(&self.repo_path)
}
#[cfg(not(windows))]
{
    self.repo_path.clone()
}
```

### COMMAND ENTRY-POINT GUARD (the line the preflight is added next to)
```rust
// SOURCE: app-tauri/src-tauri/src/commands.rs:204 (init_repo) and 14 other sites
repo.validate().map_err(|e| e.to_string())?;
```

### TEST_STRUCTURE (cfg(test) module with a local_repo helper)
```rust
// SOURCE: crates/borg-core/src/config.rs:531-587
fn local_repo(path: &str) -> RepoConfig {
    RepoConfig { ssh_host: String::new(), ssh_port: 0, ssh_user: String::new(),
                 repo_path: path.into(), ssh_key_path: None }
}
#[test]
fn location_returns_path_for_local_and_url_for_ssh() { ... }
```

---

## Files to Change

| File | Action | Justification |
|---|---|---|
| `crates/borg-core/src/config.rs` | UPDATE | Add `RepoConfig::local_repo_preflight()` + pure helpers `unc_share_root()` / `non_admin_message()` + Windows-only `share_unreachable()` + unit tests |
| `app-tauri/src-tauri/src/commands.rs` | UPDATE | Call the preflight right after `repo.validate()` in the borg-running commands |

## NOT Building
- **No probe for genuine network-share repos** (`\\nas\share\...`) — the preflight only fires for the `\\localhost\X$` admin-share form `location()` produces from a drive letter. A real `\\nas\...` failure is a different problem and borg's own error is appropriate.
- **No frontend change** — the friendly message is a returned command error string, shown by the existing error UI (settings page result banner; backup/archives `… failed: {e}`).
- **No admin-elevation / auto-relaunch-as-admin** — out of scope; we only *inform*.
- **No change to SSH repos, non-Windows, or admin-user behavior** — preflight is a no-op there.
- **No new `borg_local_repo_via_unc` regression-test changes** — that test stays (it runs on the admin VM); the non-admin path needs a non-admin account (manual follow-up, see Risks).

---

## Step-by-Step Tasks

### Task 1: Add the pure UNC-share-root parser (`config.rs`)
- **ACTION**: Add a module-private `unc_share_root(unc: &str) -> Option<String>` after `to_windows_unc_local` (~line 137).
- **IMPLEMENT**: Return `Some("\\\\localhost\\<L>$\\")` only when `unc` starts with `\\localhost\` followed by a single ASCII letter, `$`, then `\`. Otherwise `None` (so genuine network-share UNCs and non-UNC paths are skipped).
```rust
/// The admin-share root of a `\\localhost\X$\...` path (e.g.
/// `\\localhost\C$\Backups\repo` -> `\\localhost\C$\`). `None` for anything that
/// is not a localhost admin-share UNC, so the preflight ignores real network
/// shares. Pure; unit-tested on all platforms.
#[cfg(any(windows, test))]
fn unc_share_root(unc: &str) -> Option<String> {
    let prefix = r"\\localhost\";
    let rest = unc.strip_prefix(prefix)?;
    let b = rest.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b'$' && b[2] == b'\\' {
        return Some(format!(r"{prefix}{}$\", b[0] as char));
    }
    None
}
```
- **MIRROR**: NAMING_CONVENTION / CFG GATING patterns above.
- **IMPORTS**: none new.
- **GOTCHA**: Keep `#[cfg(any(windows, test))]` so it compiles in Linux test builds (for unit tests) but is absent from non-test non-Windows builds (no dead-code warning — clippy runs `-D warnings`).
- **VALIDATE**: `cargo test -p borg-core --lib unc_share_root` (after Task 4).

### Task 2: Add the friendly message + Windows probe (`config.rs`)
- **ACTION**: Add `non_admin_message()` (pure, `#[cfg(any(windows, test))]`) and `share_unreachable()` (`#[cfg(windows)]`).
- **IMPLEMENT**:
```rust
/// Actionable guidance shown when a local repo's admin share is inaccessible.
#[cfg(any(windows, test))]
fn non_admin_message() -> String {
    "This local repository is on a drive BorgUI can't reach without administrator \
     rights (it accesses local drives via the \\\\localhost\\C$ share). Run BorgUI \
     as an administrator, or use an SSH repository instead."
        .to_string()
}

/// True only when the admin-share root is *definitively* inaccessible
/// (permission denied). Any other result (reachable, not-found, other IO error)
/// returns false so the preflight never blocks a possibly-working setup.
#[cfg(windows)]
fn share_unreachable(share_root: &str) -> bool {
    matches!(
        std::fs::metadata(share_root),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied
    )
}
```
- **MIRROR**: ERROR_HANDLING message style (sentence, user-facing).
- **GOTCHA**: Backslashes — `non_admin_message` is a normal (non-raw) string so `\\\\` → `\\` and `\\C$` → `\C$`; the rendered text shows `\\localhost\C$`. Double-check the rendered output in the unit test.
- **VALIDATE**: covered by Task 4 message test.

### Task 3: Add `RepoConfig::local_repo_preflight()` (`config.rs`)
- **ACTION**: Add a public method on `impl RepoConfig` (next to `location()`, ~line 101).
- **IMPLEMENT**:
```rust
/// Verify a local repo is reachable before borg runs, returning a clear error
/// instead of borg's cryptic failure. No-op for SSH repos and on non-Windows.
/// Conservative: errors only when the admin share is definitively inaccessible.
pub fn local_repo_preflight(&self) -> Result<()> {
    #[cfg(windows)]
    {
        if self.is_local()
            && let Some(share_root) = unc_share_root(&to_windows_unc_local(&self.repo_path))
            && share_unreachable(&share_root)
        {
            return Err(BorgError::InvalidConfig { message: non_admin_message() });
        }
    }
    Ok(())
}
```
- **MIRROR**: `location()` cfg-gating; `validate()` returning `Result<()>` with `BorgError::InvalidConfig`.
- **IMPORTS**: none new (`BorgError`, `Result` already in scope at config.rs top).
- **GOTCHA**: Use a `let`-chain (`if … && let … && …`) — this repo enables let-chains (see `scheduler.rs:100`, `borg.rs`); a nested `if let` would trip clippy's `collapsible_if`. On non-Windows the whole block compiles out and the method just returns `Ok(())`.
- **VALIDATE**: `cargo clippy --workspace --all-targets -- -D warnings` clean on Linux; method returns `Ok(())` for SSH + local repos on Linux (Task 4 test).

### Task 4: Unit tests (`config.rs` `#[cfg(test)] mod tests`)
- **ACTION**: Add tests near the existing `unc_*` tests (~line 590).
- **IMPLEMENT**:
```rust
#[test]
fn unc_share_root_extracts_localhost_admin_share() {
    assert_eq!(unc_share_root(r"\\localhost\C$\Backups\repo"), Some(r"\\localhost\C$\".into()));
    assert_eq!(unc_share_root(r"\\localhost\D$\x"), Some(r"\\localhost\D$\".into()));
}
#[test]
fn unc_share_root_none_for_non_admin_share() {
    assert_eq!(unc_share_root(r"\\nas\backups\repo"), None);   // real network share
    assert_eq!(unc_share_root(r"C:\Backups\repo"), None);      // not UNC
    assert_eq!(unc_share_root(r"\\localhost\share\repo"), None); // not X$
}
#[test]
fn non_admin_message_mentions_admin_and_ssh() {
    let m = non_admin_message();
    assert!(m.contains(r"\\localhost\C$"));
    assert!(m.to_lowercase().contains("administrator"));
    assert!(m.to_lowercase().contains("ssh"));
}
#[test]
fn preflight_ok_for_ssh_and_local_on_linux() {
    // SSH repo: always Ok. Local repo on non-Windows: Ok (no-op).
    let ssh = RepoConfig { ssh_host: "h".into(), ssh_port: 22, ssh_user: "u".into(),
                           repo_path: "/r".into(), ssh_key_path: None };
    assert!(ssh.local_repo_preflight().is_ok());
    assert!(local_repo("/mnt/usb/repo").local_repo_preflight().is_ok());
}
```
- **MIRROR**: TEST_STRUCTURE (`local_repo` helper, plain `#[test]` fns).
- **GOTCHA**: `preflight_ok_for_ssh_and_local_on_linux` only asserts the non-Windows no-op path on Linux CI; the Windows `share_unreachable` branch is exercised by manual validation (see Risks), not unit tests.
- **VALIDATE**: `cargo test -p borg-core --lib` all green.

### Task 5: Wire the preflight into the borg-running commands (`commands.rs`)
- **ACTION**: Immediately after each `repo.validate().map_err(|e| e.to_string())?;` in the **borg-running** commands, add:
  ```rust
  repo.local_repo_preflight().map_err(|e| e.to_string())?;
  ```
- **APPLY TO** (the commands that actually invoke borg against the repo):
  `get_repo_info` (~119), `list_archives` (~133), `list_archive_contents` (~148), `prune_repo` (~186), `init_repo` (~204), `delete_archive` (~233), `diff_archives`, `compact_repo`, `create_backup` (uses `repo` then moves it into a profile — call the preflight *before* it's moved), `restore_archive`.
- **DO NOT** add it to the profile/config-CRUD commands that don't run borg: `save_repo_config`, `create_profile`, `rename_profile`, `import_profile`, etc. (lines ~494, 503, 509, 544, 583, 646).
- **MIRROR**: COMMAND ENTRY-POINT GUARD pattern (inline, right after `validate`).
- **IMPORTS**: none — method is on `RepoConfig`, already imported.
- **GOTCHA**: In `create_backup`, `repo` is moved into the `BackupProfile` partway through; place the preflight near the top with the other `validate`/`validate_*` calls, before the move.
- **VALIDATE**: `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build -p borg-ui`.

---

## Testing Strategy

### Unit Tests
| Test | Input | Expected Output | Edge Case? |
|---|---|---|---|
| `unc_share_root_extracts_localhost_admin_share` | `\\localhost\C$\Backups\repo` | `Some("\\localhost\C$\")` | no |
| `unc_share_root_none_for_non_admin_share` | `\\nas\backups\repo`, `C:\…`, `\\localhost\share\…` | `None` (skip preflight) | yes — real network share must not be probed |
| `non_admin_message_mentions_admin_and_ssh` | — | message contains `\\localhost\C$`, "administrator", "ssh" | no |
| `preflight_ok_for_ssh_and_local_on_linux` | SSH repo; local repo on Linux | `Ok(())` | yes — no-op paths |

### Edge Cases Checklist
- [x] SSH repo → preflight no-op (`Ok`)
- [x] Non-Windows → preflight no-op (`Ok`)
- [x] Genuine network-share UNC (`\\nas\...`) → not probed (`unc_share_root` None)
- [x] Admin Windows user → share reachable → `Ok` (borg proceeds)
- [x] Ambiguous error (NotFound, other) → `Ok` (conservative, no false-positive block)
- [ ] Standard (non-admin) Windows user → `PermissionDenied` → friendly error (manual — see Risks)

---

## Validation Commands

### Static Analysis
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```
EXPECT: formatted; zero warnings (let-chain + cfg-gating must not trip dead-code/collapsible-if).

### Unit Tests
```bash
cargo test -p borg-core --lib
```
EXPECT: all pass, including the 4 new tests.

### Full Test Suite
```bash
cargo test --workspace
LD_LIBRARY_PATH=/tmp/borgcompat BORG_TEST_BIN=/tmp/borg.bin cargo test -p borg-core --test e2e_backup_restore
```
EXPECT: no regressions (Linux local-repo behavior is unchanged — preflight is a no-op off Windows). See [[run-e2e-borg-harness]] for the borg binary setup.

### Windows Validation (admin path — confirms no false positive)
```bash
cd tests/smoke-windows && KEEP_VM=1 make validate
```
EXPECT: still 5/5 — `borg_local_repo_via_unc` passes (admin VM: share reachable → preflight `Ok` → round-trip works). Confirms the preflight doesn't block the working admin case.

### Manual Validation (the non-admin path — cannot be automated on the admin VM)
- [ ] On a **standard (non-admin)** Windows account, configure a local repo (`C:\…`) and attempt "Create repository"/backup → expect the friendly message, NOT a cryptic `WinError` and NOT a hang.
- [ ] As **admin**, the same flow still works (no false-positive block).
- [ ] An **SSH** repo is unaffected.

---

## Acceptance Criteria
- [ ] Preflight returns the friendly `InvalidConfig` message only for a local Windows repo whose admin share is definitively inaccessible.
- [ ] No-op for SSH repos, non-Windows, admin users, and genuine network shares.
- [ ] Friendly message names the admin requirement AND the SSH alternative.
- [ ] All validation commands pass; Windows `make validate` stays 5/5.

## Completion Checklist
- [ ] Code follows the `location()`/`to_windows_unc_local` cfg-gating idiom
- [ ] Error uses `BorgError::InvalidConfig` (matches `validate`)
- [ ] Pure helpers unit-tested cross-platform; impure probe isolated under `#[cfg(windows)]`
- [ ] Preflight added only to borg-running commands, after `validate`
- [ ] Conservative: ambiguous probe results never block
- [ ] HANDOFF.md "Open items" updated (non-admin preflight → done; note the manual non-admin check still pending)
- [ ] No frontend change; message surfaces via existing error UI

## Risks
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| False positive blocks a setup that would actually work | Low | High (can't create repo) | Only block on definitive `PermissionDenied`; all other results proceed and let borg run |
| Admin-share access semantics (UAC token filtering) make the probe unreliable | Medium | Low | Probe is advisory; worst case borg still runs and fails fast with its own (now non-hanging) error |
| Non-admin path can't be tested on the admin KVM VM | High | Medium | Unit-test the pure decision; document a manual check on a standard account (carried in HANDOFF open items) |

## Notes
- Pairs directly with the UNC fix in `RepoConfig::location()` (PR #31) and the matching plan `.claude/PRPs/plans/fix-windows-local-repo-path.plan.md`. The real long-term fix is upstream (marcpope/borg-windows#7); this just makes the interim admin-share requirement legible.
- Deliberately conservative by design: the goal is to *inform*, never to block a working configuration.
