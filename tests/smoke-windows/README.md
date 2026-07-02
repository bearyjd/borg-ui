# Windows Smoke Tests

Automated smoke tests for BorgUI running on a real Windows instance via
[dockur/windows](https://github.com/dockur/windows) (Windows in Docker, KVM-backed).

## Prerequisites

On the **host** machine (not in a container):

- Docker with compose plugin
- `/dev/kvm` accessible (bare-metal or nested virt enabled)
- `sshpass` installed (`dnf install sshpass` / `apt install sshpass`)
- ~20 GB free disk for the Windows image on first run

Optional (for cross-compiling the app from Linux):
- `cross` (`cargo install cross`) or `x86_64-pc-windows-gnu` target

## Quick Start

```bash
cd tests/smoke-windows

# Compile smoke: boot Windows → install toolchain → build + unit-test → teardown
make smoke

# Runtime validation: boot Windows → drive borg.exe / reg / schtasks (no
# toolchain or source needed). This is the real backup->restore + autostart +
# scheduling check.
make validate-all

# Edge validation: non-admin account + multi-drive (repo on D:, restore to C:).
# Recreates the VM so the DISK2 from docker-compose attaches, provisions a
# standard user + D:, then validates.
make edge-all

# GUI validation: the five real-desktop items (keychain in Credential Manager,
# scheduled task fires a real backup, window/tray, --minimized, console flash).
# Needs borg-ui.exe (see "GUI validation" below); missing prereqs SKIP cleanly.
make gui-all

# Tray-menu validation (#34): right-click the tray icon, assert the menu is
# exactly Show BorgUI / Backup now / Quit, and exercise Show + Quit via UIA.
# Needs borg-ui.exe + an interactive desktop; brittle Win11 tray icons SKIP.
make tray-all

# Interactive GUI flows: tray->backup nav, settings profile switch, GUI restore
# round-trip (byte-verified), and cancel-mid-backup. Drives the Svelte UI via UIA.
# Needs a PRODUCTION tauri-build exe + an interactive desktop.
make gui-flows-all

# Large-archive (#35) GUI smoke: stage a 100,000-file archive and drive the
# streaming/virtualized ArchiveBrowser via UIA -- full-tree stream, windowed DOM,
# Select-all to 100k, scroll-recycle, and a byte-verified selective folder
# restore. Needs a PRODUCTION tauri-build exe + an interactive desktop.
make archive-smoke-all

# Autostart login-cycle: register the HKCU\Run value the app writes, REBOOT the
# guest, and verify borg-ui.exe auto-started (interactive session, --minimized)
# i.e. the Run key actually fires at login. Needs a PRODUCTION tauri-build exe.
make autostart-login-all

# Installed-app updater: install an updater-capable lower-version NSIS package,
# accept the published update in the real UI, and verify the installed exe was
# replaced. The original public v0.1.0 package predates updater support.
BASELINE_INSTALLER=/path/to/BorgUI_0.1.0_x64-setup.exe \
EXPECTED_UPDATE_VERSION=0.2.0 make updater-all

# Or step-by-step:
make vm             # Boot Windows container
make ssh            # Wait for SSH (inspect with `make shell`)
make test           # Run the compile smoke (smoke-test.ps1)
make validate       # Run the runtime validation (validate.ps1) on the running VM
make provision-edge # Ensure standard user borgstd + D: drive (idempotent)
make validate-edge  # Run edge validation on a provisioned VM
make validate-gui   # Run GUI validation (keychain + scheduled-firing + signals)
make validate-tray  # Run tray-menu validation (#34: menu contents + Show/Quit)
make validate-gui-flows # Run interactive GUI flows (restore round-trip, cancel, etc.)
make validate-archive-smoke # Run the large-archive (#35) GUI smoke (100k stream + virtualization)
make validate-autostart-login # Register the Run key, reboot, verify the app auto-starts at login
make down           # Tear down
```

## How It Works

1. **`docker-compose.yml`** boots a full Windows 11 instance using KVM passthrough
2. **`oem/install.bat`** runs on first boot to install OpenSSH and create a test user
3. **`run.sh`** orchestrates: wait for boot → wait for SSH → deploy → test / validate
4. **`smoke-test.ps1`** — the *compile smoke*. Installs Rust + MinGW + WebView2, then:
   - Rust toolchain present, source deployed
   - `cargo test` for `borg-core` and `borg-platform-win` pass on Windows
   - release build succeeds, WebView2 runtime present, env sanity
   - borg.exe downloads and reports its version
   - the real-borg `cargo test` e2e is **skipped** (see the caveat below) — use the validation pass instead
5. **`validate.ps1`** — the *runtime validation*. Needs only a booted VM (no
   toolchain/source). Drives the real native tools directly from PowerShell:
   - **borg.exe** full backup → restore round-trip, unencrypted and encrypted
     (repokey-blake2), including byte-for-byte verification and a wrong-passphrase
     rejection check
   - **reg.exe** `HKCU\…\Run` add/query/delete round-trip (autostart at login)
   - **schtasks.exe** create/query/delete round-trip (scheduled backups)
6. **`validate-edge.ps1`** — the *edge validation* for the local-repo UNC fix, in
   two modes. Needs a VM with a second disk (`DISK2_SIZE` in `docker-compose.yml`)
   and a standard user (`borgstd`); `make edge-all` recreates + provisions both.
   - `-Mode admin` (run as the admin user): asserts `D:` exists (NTFS) and a repo
     on `D:` restores to `C:` — the cross-drive round-trip a relative repo can't do.
   - `-Mode nonadmin` (run as `borgstd`, a standard user): asserts `\\localhost\C$`
     is denied and a local-repo `init` fails **fast** (no hang) — the anti-hang
     guarantee for non-admins. (Pairs with the `RepoConfig::local_repo_preflight`
     friendly-error path; the preflight's decision logic is unit-tested in `borg-core`.)

### Why two scripts / the borg.exe spawn caveat

The bundled `borg.exe` is a PyInstaller bundle that **hangs at spawn when launched
by the Rust test binary under a console-less SSH session** — so the
`cargo test … e2e_backup_restore` step cannot be driven to green headlessly. The
same `borg.exe` works when launched from a real console (and from the shipped
GUI, which has a window station). `validate.ps1` therefore drives `borg.exe`
directly from PowerShell, which is both reliable and closer to how the app runs
it. The Rust-side argument construction for `reg`/`schtasks` is unit-tested
separately in `borg-platform-win`.

7. **`validate-gui.ps1`** — the *GUI validation* for the five real-desktop
   items (`make validate-gui` / `make gui-all`). Tiered so each item is checked
   as rigorously as it can be:
   - **Tier A — keychain (item 5).** The env-gated Rust round-trip test
     (`keychain::tests::windows_credential_manager_roundtrip`): a passphrase set
     through the app's keychain module is persisted to Windows Credential Manager
     (a fresh `Entry` reads it back), is visible to `cmdkey`, and is removed on
     clear. **Credential Manager is unreachable from the SSH session**
     (`ERROR_NO_SUCH_LOGON_SESSION`), so the harness compiles the test over SSH
     but **runs it in the interactive desktop (session 1) via an `/IT` task** —
     it needs `borgtest` logged in (the dockur VM auto-logs-in). SKIPs cleanly if
     the toolchain/source isn't on the VM or there's no interactive session.
   - **Tier B — scheduled firing (item 3), scriptable result.** Stages a profile
     with a local repo (via the UNC fix) + an enabled schedule, registers an
     interactive `borg-ui.exe --scheduled-backup` task, `/Run`s it, and asserts a
     `history.json` success event + a new archive in the repo. SKIPs without
     `borg-ui.exe`.
   - **Tier C — window/tray (1), `--minimized` (2), console flash (4): SIGNAL
     only.** A GUI launched over SSH renders in no desktop, so these print
     best-effort process/window-handle signals and never gate the exit code —
     finish the verdict with the VNC checklist below.

8. **`validate-tray.ps1`** — the *tray right-click menu* validation (#34;
   `make validate-tray` / `make tray-all`), the last Tier C interaction PR #33
   left as code-only. It drives the live notification area via **UI Automation**:
   right-clicks the BorgUI tray icon (opening the Win11 overflow flyout if the
   icon is hidden there), reads the popup menu, and asserts it contains **exactly**
   `Show BorgUI`, `Backup now`, `Quit`; then exercises **Show BorgUI** (a visible
   window appears) and **Quit** (the process exits). The tray menu is a native
   Win32 popup built in `tray.rs`, so contents + Show/Quit work even with a
   dev-mode `cargo build` exe; only **Backup now** (which emits to the JS frontend)
   needs the real `tauri build` UI, so it's a SIGNAL deferred to the checklist
   (Tier B already proves the backup engine fires). The notification area + UIA
   need a real desktop, so the script **relaunches itself in session 1 via an
   `/IT` task** (like the keychain test). Locating a Win11 tray icon is brittle;
   if the icon can't be found the checks **SKIP** (never a false FAIL) and the VNC
   checklist below is the verdict. Results JSON at `%USERPROFILE%\tray-results.json`;
   console output at `validate-tray.log`.

9. **`validate-gui-flows.ps1`** — the *interactive GUI flows* validation
   (`make validate-gui-flows` / `make gui-flows-all`), driving the live Svelte UI
   via UI Automation. Four flows, each PASS/FAIL/SKIP: **tray "Backup now" →
   navigates to the Backup page**; **Settings profile switch repopulates fields**
   (stages two profiles, switches the PROFILE combobox, asserts the repo path
   changes); **GUI restore round-trip** (stages a fresh repo + known archive,
   clicks the archive's Restore, picks a destination, byte-verifies the extracted
   file); **cancel mid-backup** (stages a ~400 MB source, adds it, Start Backup,
   clicks Cancel, asserts the UI returns to ready with no completed archive).
   **Two hard-won enablers** (both documented in the script): WebView2 only exposes
   its UIA tree when launched with
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-renderer-accessibility`; and the
   native folder picker nests under the Tauri window, only opens when the app is
   **foreground**, and is driven via **Ctrl+L → path → Enter** (its confirm button
   is a Pane with AutomationId `1`). **REQUIRES a PRODUCTION `borg-ui.exe`** (real
   `tauri build`, embedded frontend) — a dev-mode `cargo build` exe shows the
   localhost-error page and the WebView UI isn't reachable. Self-relaunches in
   session 1; stages its own repos/profiles and restores config after. Results JSON
   at `%USERPROFILE%\gui-flows-results.json`; console output at
   `validate-gui-flows.log`. **All four ran PASS on the KVM VM** (Failed: 0).

10. **`validate-archive-smoke.ps1`** — the *large-archive (#35) GUI smoke*
   (`make validate-archive-smoke` / `make archive-smoke-all`), proving the
   streaming + virtualized `ArchiveBrowser` holds up on a genuinely huge archive.
   The wrapper (SSH context) stages a **100,000-file / 200-folder** borg archive
   with a Defender-excluded compiled-C# file factory (staging is ~seconds), points
   an active profile at it, then relaunches the inner run in session 1 and restores
   the profile + removes the staging on the way out. Five checks, each
   PASS/FAIL/SKIP: **streams + builds the full tree** (header shows
   `100000 / 100000 files`); **virtualizes** (with ~201 logical rows expanded the
   DOM holds only ~22 windowed rows — counted via the per-row checkboxes);
   **Select all scales** to the full 100k (read from the `Restore selected (N)`
   button name); **scroll recycles the window** (wheel/ScrollPattern advances the
   visible directory range toward the end); **selective restore** ticks one folder,
   clicks Restore selected, picks a destination, and byte-verifies the extracted
   subset. Same enablers as `validate-gui-flows` (force-renderer-accessibility +
   the nested folder picker). **REQUIRES a PRODUCTION `borg-ui.exe`** (real
   `tauri build`). Results JSON at `%USERPROFILE%\archive-smoke-results.json`;
   console output at `validate-archive-smoke.log`. **All five ran PASS on the KVM
   VM** (Failed: 0).

11. **`validate-autostart-login.ps1`** — the *autostart login-cycle* validation
   (`make validate-autostart-login` / `make autostart-login-all`), the one
   autostart piece that needs a real reboot: does the `HKCU\...\Run` value the app
   registers actually launch it at login? The `reg` round-trip and
   `--minimized`→tray are validated elsewhere (`validate.ps1` + PR #33); this is
   the login-firing. Two phases driven by `run.sh` across a guest reboot (a single
   script can't survive it): **set** registers the exact value
   `borg-platform-win::autostart::enable` writes (`BorgUI` = `"<exe>" --minimized`)
   after killing any running instance; `run.sh` then `shutdown /r`'s the guest
   (real login cycle — reboot → dockur auto-login → Explorer processes Run keys),
   waits for SSH to drop and return; **verify** polls for `borg-ui.exe` and asserts
   it auto-started in an interactive session (>=1) with `--minimized`, parented by
   the shell. Restores any prior Run value + kills the app on the way out.
   **REQUIRES a PRODUCTION `borg-ui.exe`.** Results JSON at
   `%USERPROFILE%\autostart-login-results.json`; console output at
   `validate-autostart-login.log`. **Ran 1/1 PASS on the KVM VM** (pid auto-started
   in session 1, `--minimized`, parent `explorer`).

12. **`validate-updater.ps1`** — the installed-app updater validation. It
    silently installs an updater-capable baseline NSIS package, relaunches itself
    in the interactive desktop, waits for the signed published update prompt,
    invokes **Download and install**, and verifies the installed
    `borg-ui.exe` product version changes to `EXPECTED_UPDATE_VERSION`. The
    baseline must be older than the target and must already contain the updater;
    the original public `v0.1.0` installer is not suitable because it predates
    updater support. Results are written to
    `%USERPROFILE%\updater-smoke-result.json` and `validate-updater.log`.

### Still needs manual confirmation (Tier C — the VNC checklist)

Tier A (keychain) and Tier B (scheduled firing) are now asserted by
`validate-gui.ps1`. What remains strictly visual — the Tauri window/tray
rendering, `--minimized` landing in the tray, and the console-flash being gone
(`CREATE_NO_WINDOW`) — must be confirmed on a real desktop session (a GUI
launched over SSH renders in no window station). See **GUI validation** below
for the procedure and checklist.

## GUI validation (the five real-desktop items)

`validate-gui.ps1` confirms what the engine-level `validate.ps1` can't, because
these need the actual Tauri binary and/or a real Credential Manager.

### Prerequisites

- **`borg-ui.exe`** (Tier B/C). Either:
  - **Route 1 — build on the VM:** `make build-env` + `make deploy`, then, in
    `C:\borgui-test\app-tauri`, `pnpm install` and **`pnpm tauri build --no-bundle`**
    (the `--no-bundle` skips the installer; you only need the exe). Output under
    `target/release/borg-ui.exe`. **pnpm note:** the VM's corepack pnpm shim had a
    key-verification bug; it's fixed (`corepack disable` removed the broken
    `C:\Program Files\nodejs\` shims so `pnpm` resolves to a working standalone
    binary — persists in the `win-storage` volume). A real `pnpm tauri build
    --no-bundle` now produces a production exe (~13 MB, embedded frontend). **Use
    `tauri build`, NOT `cargo build --release`** —
    a plain `cargo build` produces a *dev-mode* binary whose window loads the Vite
    dev server (`devUrl` `http://localhost:5173`) and shows "localhost refused to
    connect" instead of the embedded UI (`frontendDist` `../build`). It is fine for
    the headless Tier B scheduled-backup path (which never loads the WebView), but
    Tier C window/tray rendering needs the real `tauri build`.
  - **Route 2 — drop a pre-built exe:** build `borg-ui.exe` on any Windows box
    and place it at `tests/smoke-windows/shared/borg-ui.exe`. `run.sh` uploads it
    to the VM home (dockur does **not** surface `./shared` inside Windows, so the
    upload is how it gets there). `validate-gui.ps1` also copies the **whole borg
    distribution** (`borg.exe` **and** its `_internal\` folder) next to
    `borg-ui.exe` automatically — borg 1.4.4+win7 is a PyInstaller onedir bundle
    that dies with "Failed to load Python DLL `_internal\python311.dll`" if only
    the `.exe` is present. The same applies when packaging the real app (the app
    resolves borg from its own directory; `tauri.conf.json` `resources` is empty,
    so borg is shipped as an external step — include `_internal\` too).
- **Rust toolchain + deployed source** (Tier A keychain test): `make build-env`
  + `make deploy`. Missing → the keychain check SKIPs (it never fails falsely).
- **`borgtest` logged in** at the interactive desktop (auto-login is on) so the
  scheduled `/IT` task and any GUI launch get a session-1 window station.

### Run it

```bash
cd tests/smoke-windows
KEEP_VM=1 make gui-all     # boot (if needed) + run validate-gui.ps1
# or, on an already-running VM:
KEEP_VM=1 make validate-gui
```

Tier A/B report PASS/FAIL/SKIP and gate the exit code (grep `Failed: 0`); Tier C
prints `SIGNAL:` lines for the manual checklist. Results JSON lands at
`%USERPROFILE%\gui-results.json`; console output at `validate-gui.log`.

### Tier C manual checklist (over VNC: http://localhost:8006, or RDP :3389)

Log in as `borgtest` (password `Password1!`), then:

- [ ] **Item 1 — window/tray:** launch `borg-ui.exe`; the window opens (title
      "BorgUI — Backup Manager"); a tray icon appears with **Show BorgUI /
      Backup now / Quit**; closing the window hides it to the tray (the process
      keeps running).
- [ ] **Item 2 — `--minimized`:** launch `borg-ui.exe --minimized`; no window
      appears; the tray icon is present; **Show BorgUI** restores the window.
- [ ] **Item 4 — console flash:** trigger a backup from the app and watch for a
      black console window flashing when borg spawns — expect **none**
      (`CREATE_NO_WINDOW`). Record a short screen capture if possible; save
      screenshots under `tests/smoke-windows/shared/`.
- [ ] **Item 1b — tray right-click menu (#34):** right-click the tray icon (on
      Windows 11, click the "Show hidden icons" chevron first if it's in the
      overflow). The menu shows **exactly** `Show BorgUI`, `Backup now`, `Quit`.
      Click **Show BorgUI** → a hidden/minimized window is restored and focused.
      Click **Backup now** → the window surfaces and a backup starts (a history
      event / new archive appears). Click **Quit** → the process exits and the
      tray icon disappears. `make validate-tray` automates the contents + Show +
      Quit checks via UI Automation; this manual pass is the fallback when the
      Win11 tray icon can't be located automatically (the script SKIPs then).

### Tray menu validation (#34)

```bash
cd tests/smoke-windows
KEEP_VM=1 make tray-all       # boot (if needed) + run validate-tray.ps1
# or, on an already-running, logged-in VM:
KEEP_VM=1 make validate-tray
```

`validate-tray.ps1` relaunches itself in session 1 (an `/IT` task) to reach the
real desktop, then drives the tray menu via UIA. PASS/FAIL on
`tray_menu_contents` / `tray_show_action` / `tray_quit_action` gate the exit code
(grep `Failed: 0`); `tray_backup_now` is a SIGNAL. If the tray icon can't be
located the checks SKIP — fall back to the **Item 1b** manual checklist above.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KEEP_VM` | `0` | Set to `1` to keep Windows running after tests |
| `FORCE_BUILD` | `0` | Set to `1` to force rebuild even if .exe exists |
| `INSTALLER_DIR` | `tests/smoke-windows/installers` | Local dir holding the built `*.msi` / `*-setup.exe` for `validate-installer` |

## Installer validation

`make validate-installer` (or `installer-all` to boot first) silently installs the
built Windows installer **and** verifies the bundled borg lands correctly — the one
thing the other GUI checks never cover (they run a loose `tauri build` exe, never an
installed layout). `validate-installer.ps1`:

1. Silent-installs each installer it finds (NSIS `/S`, per-user, no elevation; MSI
   `msiexec /quiet`, which SKIPs if UAC blocks it over SSH).
2. Asserts `borg-ui.exe` + `borg.exe` + `_internal\python311.dll` are co-located in
   the install dir (the bundling contract — `lib.rs` resolves borg beside the exe,
   and borg dies without `_internal`).
3. Runs the **installed** `borg.exe` through `--version` + a real
   init→create→delete→extract→byte-verify round-trip.
4. Silently uninstalls and asserts the app is gone.

Build the installers in CI (the `Release` workflow uploads a `borgui-windows-installers`
artifact on every run), then `gh run download <run-id> -n borgui-windows-installers -D
tests/smoke-windows/installers` before running the target.

## Pre-built Binary

If cross-compilation isn't set up, build on a Windows machine and place the
output at:

```
tests/smoke-windows/shared/borg-ui.exe
```

## Results

After a run, results are in:
- `results.log` — compile-smoke console output (`smoke-test.ps1`)
- `validate.log` — runtime-validation console output (`validate.ps1`)
- `C:\borgui-test\smoke-results.json` / `%USERPROFILE%\validate-results.json` in the VM — machine-readable JSON

## First Run

The first boot takes 10-15 minutes (Windows installation). Subsequent boots
are ~60 seconds since the disk state is preserved.

## Troubleshooting

```bash
# View Windows boot logs
make logs

# SSH in manually
make ssh

# Check if KVM is available
ls -la /dev/kvm

# VNC into Windows (for debugging GUI issues)
# Open http://localhost:8006 in a browser
```
