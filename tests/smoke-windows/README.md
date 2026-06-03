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

# Or step-by-step:
make vm             # Boot Windows container
make ssh            # Wait for SSH (inspect with `make shell`)
make test           # Run the compile smoke (smoke-test.ps1)
make validate       # Run the runtime validation (validate.ps1) on the running VM
make provision-edge # Ensure standard user borgstd + D: drive (idempotent)
make validate-edge  # Run edge validation on a provisioned VM
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

### Still needs manual confirmation (cannot be asserted headlessly)

The Tauri window actually appearing, the tray icon/menu, `--minimized` landing in
the tray, a scheduled task firing the headless backup, the console-flash being
gone (`CREATE_NO_WINDOW`), and the OS keychain storing the passphrase in Windows
Credential Manager — all require eyes on a real desktop session.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KEEP_VM` | `0` | Set to `1` to keep Windows running after tests |
| `FORCE_BUILD` | `0` | Set to `1` to force rebuild even if .exe exists |

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
