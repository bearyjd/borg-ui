# Plan: Windows GUI validation pass (the 5 real-desktop items)

## Summary
Five behaviors can only be confirmed on a real Windows desktop and are still
unverified: (1) the Tauri window + tray render, (2) `--minimized` starts hidden
in the tray, (3) a registered Task Scheduler task actually fires the headless
backup end-to-end, (4) `CREATE_NO_WINDOW` suppresses the borg console flash, and
(5) the passphrase is stored in Windows Credential Manager. This plan extends the
`tests/smoke-windows/` KVM harness to validate each — automating what can be
scripted (keychain, scheduled-firing) and providing a precise interactive-session
+ VNC procedure for the inherently visual ones (window/tray, minimized, flash).

## User Story
As a maintainer shipping BorgUI to Windows, I want each GUI-only behavior
verified on a real Windows desktop (scripted where possible, a tight VNC
checklist where not), so that I can trust the app — not just the engine — before
a production release.

## Problem → Solution
All five behaviors are "unverified on hardware" (HANDOFF open items); the headless
SSH harness can't assert GUI rendering. **→** A tiered validation pass: a new
`validate-gui.ps1` automates keychain + scheduled-firing over SSH, and a
documented interactive-session procedure (auto-login desktop + VNC at
`localhost:8006`) covers window/tray render, `--minimized`, and the console flash,
with best-effort programmatic window/process checks to reduce the manual surface.

## Metadata
- **Complexity**: Large (harness scripts + a Rust keychain integration test + a
  Tauri-app build step on the VM + a manual VNC procedure; spans 5 validation items)
- **Source PRD**: N/A (free-form; the GUI items in `HANDOFF.md` "Open items" /
  "Do this before first production use")
- **PRD Phase**: N/A
- **Estimated Files**: 4–5 (`tests/smoke-windows/validate-gui.ps1` [new],
  `tests/smoke-windows/run.sh`, `tests/smoke-windows/Makefile`,
  `tests/smoke-windows/README.md`, `app-tauri/src-tauri/src/keychain.rs` [test only])

---

## The central constraint (read first)

dockurr/windows auto-logs-in the admin user **`borgtest`** into an interactive
desktop (viewable at `http://localhost:8006`, VNC/web; RDP on `:3389`). A process
launched **over SSH** (`:2222`) runs in a *different, non-interactive session with
no window station* — so a GUI (Tauri/WebView2) app launched that way **will not
render** on the `:8006` desktop and may fail to initialize its window at all.

Therefore the five items split into tiers:

| Tier | Items | Needs interactive desktop? | How |
|---|---|---|---|
| **A — SSH-scriptable** | (5) keychain | No (no Tauri) | Rust test of `keychain` module + `cmdkey /list` over SSH |
| **B — interactive launch, file-checkable result** | (3) scheduled firing | Yes (Tauri inits even hidden) | Run a task as the logged-in `borgtest` (session 1) → assert `history.json` + a new archive over SSH |
| **C — interactive launch + visual confirm** | (1) window/tray, (2) `--minimized`, (4) console flash | Yes | Launch app in session 1 (Startup folder / RDP / VNC) → VNC screenshot + best-effort process/window-handle checks |

Tier C items are inherently visual; the plan minimizes (does not eliminate) the
manual surface.

---

## Prerequisite: a runnable `borg-ui.exe` on the VM

Items 1–4 need the actual Tauri binary (item 5 does not). Two supported routes:

- **Route 1 (recommended): build on the VM.** Extend `run.sh::setup_env` to also
  install Node LTS + enable corepack/pnpm, then `pnpm install` + `pnpm tauri build`
  in `app-tauri`. Heavy (WebView2 + tauri-cli + frontend build) but self-contained.
  Output: `app-tauri/src-tauri/target/release/borg-ui.exe` (or the bundled exe).
- **Route 2: drop a pre-built exe.** Build `borg-ui.exe` on any Windows machine and
  place it at `tests/smoke-windows/shared/borg-ui.exe` (the `shared/` dir is mounted
  at `/shared` in the VM per `docker-compose.yml:25`; README "Pre-built Binary"
  already documents this drop point). Fastest; no VM toolchain bloat.

**GOTCHA:** `lib.rs` resolves borg as `current_exe().parent()/"borg.exe"`. Wherever
`borg-ui.exe` runs from, **copy `C:\borg\borg.exe` next to it** first, or borg ops fail.

---

## Mandatory Reading

| Priority | File | Lines | Why |
|---|---|---|---|
| P0 | `tests/smoke-windows/validate.ps1` | all | Pass/Fail harness + `Invoke-Borg` timeout pattern + JSON results — `validate-gui.ps1` mirrors this exactly |
| P0 | `tests/smoke-windows/run.sh` | 73-119, 144-185, 224-262 | `setup_env` (add Node here for Route 1), `run_validate`/`run_tests`, the subcommand `case` — add a `validate-gui` subcommand mirroring `run_validate` |
| P0 | `app-tauri/src-tauri/src/lib.rs` | 14-58, 112-158 | Flag handling: `--scheduled-backup` → `start_scheduled_backup`, `--minimized` → window hidden; `borg_path` resolution; tray vs scheduled branch |
| P0 | `app-tauri/src-tauri/src/scheduled.rs` | all | What a fired task does: `run_scheduled_backup` (active profile's schedule → backup → `history.json`). Defines item 3's assertions |
| P0 | `app-tauri/src-tauri/src/keychain.rs` | all | `keyring` service `"borg-ui"`, `set/get/clear_passphrase(account, …)`; item 5 tests this directly |
| P1 | `app-tauri/src-tauri/src/commands.rs` | 359-400 | `save_schedule_config` → `schtasks` task name `"BorgUI-Backup"`, args `--scheduled-backup`; item 3 mirrors this registration |
| P1 | `app-tauri/src-tauri/src/profiles.rs` | 7-18, 59-81 | `Profile`/`ProfilesData` shape + `profiles.json` location/format; item 3 must write a valid one to the app config dir |
| P1 | `app-tauri/src-tauri/src/tray.rs` | all | Tray menu items ("Show BorgUI", "Backup now", "Quit") — what to look for in item 1's VNC confirm |
| P1 | `crates/borg-platform-win/src/scheduler.rs` | 23-78 | `schtasks /Create` arg shapes (already validated by `validate.ps1`); item 3 registers an interactive-user task |
| P2 | `tests/smoke-windows/docker-compose.yml` | 18-26 | Ports: 8006 (VNC/web), 3389 (RDP), 2222 (SSH); `shared/` + `oem/` mounts |
| P2 | `tests/smoke-windows/oem/install.bat` | 15-17 | `borgtest` is a local Administrator + auto-login user (the interactive session) |
| P2 | `crates/borg-core/src/config.rs` | 84-137 | `location()` UNC rewrite — item 3's repo must be a local path that now works (or an SSH repo) |

## External Documentation

| Topic | Source | Key Takeaway |
|---|---|---|
| Windows sessions & window stations | Microsoft docs | An SSH-spawned process is in a non-interactive session; GUI windows render only in the interactive logon session (session 1, the auto-login desktop). Launch GUI tests there. |
| `schtasks /Run`, `/RU`, `/IT` | Microsoft docs | `/IT` = run only when the user is logged on (interactive); `schtasks /Run /TN <name>` triggers immediately. Use an interactive `borgtest` task so the Tauri app gets a desktop. |
| Tauri v2 windowing / WebView2 | tauri.app | Tauri creates the window during setup even when later hidden; it needs a window station. WebView2 runtime is already installed by `setup_env`. |
| Windows Credential Manager + `keyring` crate | keyring docs / `cmdkey` | The `keyring` v3 `windows-native` backend stores a **Generic Credential**; `cmdkey /list` shows targets containing the service `borg-ui`. Use it to confirm item 5 hit Credential Manager. |

KEY_INSIGHT: A GUI app launched over SSH won't render on the auto-login desktop.
APPLIES_TO: items 1–4 (launch must happen in session 1).
GOTCHA: For item 3, prefer a `schtasks` task with `/RU borgtest /IT` (or trigger while logged in) so the Tauri app initializes; a session-0 task may fail to create its window.

KEY_INSIGHT: `borg-ui.exe` finds borg via its own directory.
APPLIES_TO: items 1–4.
GOTCHA: copy `C:\borg\borg.exe` next to `borg-ui.exe` before any run.

---

## UX Design
N/A — this is a validation/verification effort. No product UX changes. (It *confirms* the existing UX: window, tray, minimized-to-tray, scheduled backups, no console flash, secure passphrase storage.)

### What "correct" looks like (the things being confirmed)
| Item | Expected on a real desktop |
|---|---|
| 1 window/tray | App window opens (title "BorgUI — Backup Manager"); a tray icon with Show/Backup now/Quit; closing the window hides to tray (process stays) |
| 2 `--minimized` | No window appears; tray icon present; `Show BorgUI` restores it |
| 3 scheduled firing | Task Scheduler entry runs `borg-ui.exe --scheduled-backup`; a backup completes; `history.json` gains a success event; repo gains an archive; no window steals focus |
| 4 console flash | Running a backup spawns borg with **no** black console window flashing |
| 5 keychain | Passphrase set in-app appears as a Generic Credential in Windows Credential Manager; survives app restart; clearing removes it |

---

## Patterns to Mirror

### PS_VALIDATION_HARNESS (Pass/Fail + JSON + exit code)
```powershell
# SOURCE: tests/smoke-windows/validate.ps1:24-44, 230-246
function Pass($name, $detail) { $script:Passed++; $script:Results += @{ Name=$name; Status="PASS"; Detail=$detail }; Write-Host "  PASS: $name" -ForegroundColor Green }
function Fail($name, $detail) { $script:Failed++; ... }
# ...summary writes $env:USERPROFILE\validate-results.json; exit 1 if Failed>0
```

### PS_TIMEOUT_LAUNCH (bounded process; never hang the run)
```powershell
# SOURCE: tests/smoke-windows/validate.ps1 (Invoke-Borg)
$p = Start-Process -FilePath $exe -ArgumentList $args -WindowStyle Hidden -PassThru -RedirectStandardOutput $o -RedirectStandardError $e
if (-not $p.WaitForExit($TimeoutSec*1000)) { $p.Kill(); <timeout> } else { $p.WaitForExit(); <exit code> }
```

### RUNSH_SUBCOMMAND (deploy a script over scp, run it, grep "Failed: 0")
```bash
# SOURCE: tests/smoke-windows/run.sh (run_validate)
run_validate() {
    $SCP_CMD "$SCRIPT_DIR/validate.ps1" "$SSH_USER@$SSH_HOST:validate.ps1"
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate.log"
    if echo "$output" | grep -q "Failed: 0"; then return 0; else fail "..."; fi
}
# case: validate) run_validate ;;  + validate-all) ... ;;   (mirror for validate-gui)
```

### ASCII_ONLY (.ps1 must be ASCII)
```text
# SOURCE: HANDOFF.md / smoke-test.ps1 comment — Windows PowerShell 5.1 reads
# UTF-8-without-BOM as ANSI; a non-ASCII char breaks parsing. Keep validate-gui.ps1 ASCII.
```

### KEYCHAIN_API (what item 5's test calls)
```rust
// SOURCE: app-tauri/src-tauri/src/keychain.rs:9-29
pub fn set_passphrase(account: &str, passphrase: &str) -> Result<(), String> { entry(account)?.set_password(passphrase)... }
pub fn get_passphrase(account: &str) -> Result<Option<String>, String> { match entry(account)?.get_password() { Ok(p)=>Ok(Some(p)), Err(NoEntry)=>Ok(None), ... } }
pub fn clear_passphrase(account: &str) -> Result<(), String> { ... }
```

### SCHEDULED_RUNNER_CONTRACT (what item 3 asserts after the task fires)
```rust
// SOURCE: app-tauri/src-tauri/src/scheduled.rs (run_scheduled_backup + finish)
// loads active profile's schedule -> borg create -> records a BackupEvent {outcome:"success"} to history.json
```

### TEST_GATE (BORG_TEST_BIN-style skip + tokio test) — for the keychain Rust test
```rust
// SOURCE: app-tauri/src-tauri/src/scheduled.rs tests (borg_or_skip) + e2e_backup_restore.rs
// gate the Credential-Manager round-trip test on #[cfg(windows)] + an opt-in env var so it
// only runs in the Windows harness, never on Linux CI.
```

---

## Files to Change

| File | Action | Justification |
|---|---|---|
| `tests/smoke-windows/validate-gui.ps1` | CREATE | Tier A+B automation: keychain round-trip + Credential-Manager check; scheduled-task firing end-to-end; best-effort window/process checks for the launched app |
| `app-tauri/src-tauri/src/keychain.rs` | UPDATE | Add a `#[cfg(windows)]`, env-gated integration test: set → get → (cmdkey-visible) → clear round-trip against real Credential Manager |
| `tests/smoke-windows/run.sh` | UPDATE | Add `validate-gui` (+ `gui-all`) subcommand mirroring `run_validate`; optionally extend `setup_env` with Node + `pnpm tauri build` (Route 1) |
| `tests/smoke-windows/Makefile` | UPDATE | `make validate-gui` / `make gui-all` targets |
| `tests/smoke-windows/README.md` | UPDATE | Document the interactive-session/VNC procedure + the manual checklist for items 1, 2, 4 |
| `HANDOFF.md` | UPDATE | Tick off items as confirmed; record what remains strictly-manual |

## NOT Building
- **No headless GUI rendering / fully-automated pixel assertions** for items 1, 2, 4 — those require the interactive desktop and a human (or a screenshot diff) for final sign-off. We script the launch + process/window-handle checks, not the visual judgment.
- **No CI gating on the GUI pass** — it needs a desktop session + (Route 1) a heavy app build; keep it a manual/operator `make gui-all`, not part of `.github/workflows/ci.yml`.
- **No new product code** — except the keychain *test*. This is validation; if a check fails, fixes are separate follow-ups.
- **No replacement of the existing `validate.ps1`** (engine/autostart/scheduling command-shapes) — `validate-gui.ps1` is additive.

---

## Step-by-Step Tasks

### Item 5 — keychain in Credential Manager (Tier A, do first; highest ROI, fully scriptable)

#### Task 5.1: Windows-gated keychain round-trip test (`keychain.rs`)
- **ACTION**: Add a `#[cfg(test)] mod tests` to `keychain.rs` with one `#[cfg(windows)]` test gated on an opt-in env var (e.g. `BORGUI_KEYCHAIN_TEST=1`) so it never runs on Linux CI.
- **IMPLEMENT**: `set_passphrase("validate@smoke", "secret-123")` → assert `get_passphrase(...) == Ok(Some("secret-123"))` → `clear_passphrase(...)` → assert `get_passphrase(...) == Ok(None)`. Use a throwaway account string.
- **MIRROR**: TEST_GATE (env-gated like `borg_or_skip`); KEYCHAIN_API.
- **IMPORTS**: `use super::*;`.
- **GOTCHA**: Don't use the real `repo.ssh_url()` account — use a unique test account so a real stored passphrase is never touched; always `clear` at the end.
- **VALIDATE**: on the VM via SSH: `BORGUI_KEYCHAIN_TEST=1 cargo test -p borg-ui --lib keychain -- --nocapture`.

#### Task 5.2: Credential-Manager visibility check (`validate-gui.ps1`)
- **ACTION**: After the Rust test stores a credential, confirm it actually lives in **Windows Credential Manager** (not just round-tripped in memory). In `validate-gui.ps1`: store via a tiny invocation, then `cmdkey /list` and assert a target containing `borg-ui` exists; then clear and assert it's gone.
- **IMPLEMENT**: Pass/Fail entry `keychain_credential_manager`. Drive the store/clear through the keychain test or a small helper; parse `cmdkey /list` output for `borg-ui`.
- **MIRROR**: PS_VALIDATION_HARNESS.
- **GOTCHA**: `cmdkey /list` shows the *target* name; `keyring` formats it from service+account — match on the `borg-ui` substring, not an exact string. ASCII only.
- **VALIDATE**: `make validate-gui` shows `keychain_credential_manager` PASS.

### Item 3 — scheduled task actually fires the backup (Tier B; scriptable result)

#### Task 3.1: Stage a profile + repo + the app binary
- **ACTION**: In `validate-gui.ps1`, write a valid `profiles.json` to the app config dir `%APPDATA%\com.borgui.app\profiles.json` with one active profile: a **local** repo (a temp folder — now works via the UNC `location()` fix) + an **enabled** schedule with `source_paths` (a temp src dir with a file) and excludes `[]`. Ensure `borg-ui.exe` is present with `borg.exe` copied beside it; `borg init` the repo first.
- **MIRROR**: `profiles.rs` `Profile`/`ProfilesData` shape; SCHEDULED_RUNNER_CONTRACT.
- **GOTCHA**: identifier is `com.borgui.app` (`tauri.conf.json`) → config dir is `%APPDATA%\com.borgui.app`. The schedule's own `source_paths` are used (not the Backup page's). Repo must be init'd (the runner does create, not init).
- **VALIDATE**: file exists + parses; `borg list` shows an empty repo.

#### Task 3.2: Register and fire the task as the interactive user
- **ACTION**: Register a task mirroring `save_schedule_config` but as the logged-in user: `schtasks /Create /F /TN BorgUI-SmokeBackup /TR '"<path>\borg-ui.exe" --scheduled-backup' /SC ONCE /ST 00:00 /RU borgtest /IT`. Then `schtasks /Run /TN BorgUI-SmokeBackup`. Poll (bounded) for completion.
- **IMPLEMENT**: Pass/Fail `scheduled_task_fires`.
- **MIRROR**: `scheduler.rs` `schtasks /Create` shape; commands.rs task name/args.
- **GOTCHA**: `/IT` + a logged-in `borgtest` gives the app a desktop so Tauri can init (even though the scheduled runner hides the window). Bound the poll; on timeout, Fail (don't hang). Clean up the task in a `finally`.
- **VALIDATE**: task `/Query` shows Last Run Result 0 after firing.

#### Task 3.3: Assert the backup really happened
- **ACTION**: After the task completes, assert: `%APPDATA%\com.borgui.app\history.json` contains a new `{"outcome":"success","kind":"backup"}` event, AND `borg list <repo>` shows one archive whose name matches it.
- **MIRROR**: SCHEDULED_RUNNER_CONTRACT (history shape from `history.rs`/`scheduled.rs`).
- **GOTCHA**: `history.json` is created by the runner; if absent, the task launched but the runner didn't reach `finish()` — capture the task's exit code for the failure message.
- **VALIDATE**: `scheduled_task_fires` PASS with archive name in the detail.

### Items 1, 2, 4 — window/tray, --minimized, console flash (Tier C; interactive + visual)

#### Task C.1: Establish an interactive-session launch + best-effort process checks
- **ACTION**: Document and script launching `borg-ui.exe` in session 1. Preferred scriptable hook: drop a shortcut/exe in the `borgtest` Startup folder (`shell:startup`) OR `schtasks /Run` an `/IT` task that launches it plain (no `--scheduled-backup`). Then over SSH, query the process and its main window: `Get-Process borg-ui | Select Id, MainWindowHandle, MainWindowTitle`.
- **IMPLEMENT**: Pass/Fail `gui_window_present` (item 1): assert a `borg-ui` process exists with a non-zero `MainWindowHandle` and title containing "BorgUI". Pass/Fail `gui_minimized_hidden` (item 2): launch with `--minimized`; assert the process exists but `MainWindowHandle == 0` / not visible.
- **MIRROR**: PS_VALIDATION_HARNESS; `lib.rs` flag handling (`--minimized` hides the window).
- **GOTCHA**: `MainWindowHandle` reflects the desktop the SSH query can see; a Tauri/WebView2 window may report 0 even when visible in some configs — treat a 0/!0 mismatch as a *signal to screenshot*, not a hard verdict. The tray icon is NOT exposed via `MainWindowHandle` — tray presence is visual-only.
- **VALIDATE**: process/handle checks recorded; followed by the screenshot step below.

#### Task C.2: VNC screenshot capture + manual checklist
- **ACTION**: Capture the `:8006` desktop for the record and provide a tight manual checklist. Scriptable capture: dockurr/windows exposes a web viewer; grab a screenshot via the viewer endpoint or RDP, save to `tests/smoke-windows/shared/gui-*.png`.
- **IMPLEMENT**: a `run.sh` helper note + README section; the actual visual judgment is the operator's.
- **MANUAL CHECKLIST** (README):
  - [ ] Item 1: window opens with title "BorgUI — Backup Manager"; tray icon present; right-click shows Show/Backup now/Quit; closing the window hides to tray (process stays).
  - [ ] Item 2: `--minimized` launch shows no window; tray icon present; "Show BorgUI" restores it.
  - [ ] Item 4: trigger a backup from the app; watch for a black console window flashing when borg spawns — expect **none** (record a short screen capture if possible).
- **GOTCHA**: Item 4 (console flash) is the least automatable — a flash is sub-second. Best programmatic signal: during a backup, poll for any new `conhost.exe`/console window owned by the borg child; absence is supporting evidence but the screen capture is authoritative. ASCII only in any script.
- **VALIDATE**: screenshots saved; checklist boxes ticked by the operator.

#### Task C.3: Wire up `validate-gui` in run.sh + Makefile + README
- **ACTION**: Add `run_validate_gui()` (mirror `run_validate`) and `validate-gui` / `gui-all` cases to `run.sh`; `make validate-gui` / `make gui-all` targets; README section documenting the interactive-session requirement, VNC access, the Route 1/2 binary options, and the manual checklist.
- **MIRROR**: RUNSH_SUBCOMMAND.
- **GOTCHA**: `validate-gui.ps1` Tier-A/B checks return a `Failed: N` summary (grep-able like `run_validate`); Tier-C visual items are reported as informational + a checklist, not auto-pass/fail (don't let an un-assertable visual gate the script's exit code — print guidance instead).
- **VALIDATE**: `make validate-gui` runs end-to-end on the warm VM; keychain + scheduled-firing PASS; window/minimized checks print signals; console-flash + tray prompt the manual checklist.

---

## Testing Strategy

### Automated assertions (Tier A/B — in `validate-gui.ps1` / keychain test)
| Test | Input | Expected | Tier |
|---|---|---|---|
| keychain round-trip (Rust) | set/get/clear test account | get==stored, then None after clear | A |
| `keychain_credential_manager` | stored credential | `cmdkey /list` shows a `borg-ui` target; gone after clear | A |
| `scheduled_task_fires` | registered `--scheduled-backup` task, `schtasks /Run` | `history.json` success event + one archive in repo | B |
| `gui_window_present` (signal) | launched app (session 1) | `borg-ui` process with non-zero MainWindowHandle + "BorgUI" title | C (signal) |
| `gui_minimized_hidden` (signal) | app `--minimized` | process alive, MainWindowHandle 0 / not visible | C (signal) |

### Manual (Tier C — VNC checklist)
- [ ] window renders + tray menu (Show/Backup now/Quit) + close-to-tray
- [ ] `--minimized` → no window, tray present, Show restores
- [ ] no console flash on a real backup

### Edge Cases Checklist
- [x] keychain test uses a throwaway account + always clears (no real secret touched)
- [x] scheduled task cleaned up in `finally`; poll bounded (no hang)
- [x] app run dir has `borg.exe` beside `borg-ui.exe`
- [x] `.ps1` files ASCII-only
- [ ] non-admin behavior — out of scope here (covered by the non-admin preflight plan)

---

## Validation Commands

### Static / hygiene (Linux, before deploy)
```bash
grep -nP '[^\x00-\x7F]' tests/smoke-windows/validate-gui.ps1 && echo "NON-ASCII!" || echo "ASCII OK"
bash -n tests/smoke-windows/run.sh
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test -p borg-ui --lib   # keychain test is env-gated off here -> compiles, doesn't run
```
EXPECT: ASCII OK; run.sh parses; clippy clean; Linux tests green (keychain test skipped).

### On the VM (warm; `KEEP_VM=1`)
```bash
cd tests/smoke-windows
# Prereq once: ensure borg-ui.exe present (Route 1 build, or drop in shared/) + borg.exe beside it
KEEP_VM=1 make validate-gui
```
EXPECT: `keychain_credential_manager` PASS, `scheduled_task_fires` PASS; window/minimized signals printed; console-flash + tray → manual checklist.

### Manual (VNC)
```text
Open http://localhost:8006  (or RDP localhost:3389, user borgtest / Password1!)
Work the Tier-C checklist; save screenshots to tests/smoke-windows/shared/
```

---

## Acceptance Criteria
- [ ] Item 5: keychain round-trip passes AND the credential is visible in Windows Credential Manager (`cmdkey`), then removed on clear.
- [ ] Item 3: a fired Task Scheduler task produces a real backup (history success event + a new archive).
- [ ] Items 1, 2: process/window-handle signals captured + VNC checklist ticked (window renders, tray works, close-to-tray; `--minimized` starts hidden, Show restores).
- [ ] Item 4: VNC/recording confirms no console flash on a real backup.
- [ ] `make validate-gui` runs end-to-end without hanging; HANDOFF items updated to reflect what's now confirmed.

## Completion Checklist
- [ ] `validate-gui.ps1` mirrors `validate.ps1` (Pass/Fail/JSON/exit) and is ASCII-only
- [ ] keychain test is `#[cfg(windows)]` + env-gated (off on Linux CI)
- [ ] scheduled-task and any Startup/shortcut artifacts cleaned up after the run
- [ ] No CI changes; GUI pass stays operator-run
- [ ] README documents the interactive-session requirement + VNC + binary routes + checklist
- [ ] HANDOFF "Open items"/"before first production use" updated

## Risks
| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Tauri app won't launch/render over SSH (no desktop) | High (expected) | High for items 1-4 | Launch in session 1 (Startup/`/IT` task/RDP/VNC); document it as the central constraint; SSH only for assertions, not GUI launch |
| Building `borg-ui.exe` on the VM (Node+tauri+WebView2) is heavy/flaky | Medium | Medium | Offer Route 2 (drop a pre-built exe in `shared/`); keep the build out of CI |
| Scheduled task runs in session 0 → Tauri window-init fails | Medium | Medium | Use `/RU borgtest /IT` and trigger while logged in; capture exit code on failure |
| Console flash (item 4) not programmatically assertable | High | Low | Best-effort conhost-window poll + authoritative screen capture; accept manual sign-off |
| `MainWindowHandle` unreliable for WebView2 windows | Medium | Low | Treat as a signal, not a verdict; back with a screenshot |
| Touching real Credential Manager entries | Low | Medium | Throwaway test account; always clear in `finally` |

## Notes
- Tier A+B (keychain, scheduled-firing) deliver the most confidence per unit effort and are fully scriptable — implement those first; they alone close two of the five open items with hard evidence.
- Tier C (window/tray, `--minimized`, console flash) is inherently visual; the plan reduces but cannot remove the manual VNC step. Pairs with `.claude/PRPs/plans/fix-windows-local-repo-path.plan.md` (the now-working local repo lets item 3 use a local repo) and the autostart/`--minimized` work already merged.
- The VM (`KEEP_VM=1`) is currently warm; `borgtest` is an admin with auto-login, so the interactive desktop is available at `localhost:8006`.
