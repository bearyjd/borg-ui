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

# Full run: build → boot Windows → run tests → teardown
make smoke

# Or step-by-step:
make build    # Build/check for Windows binary
make vm       # Boot Windows container
make ssh      # SSH in manually to inspect
make test     # Run smoke tests
make down     # Tear down
```

## How It Works

1. **`docker-compose.yml`** boots a full Windows 11 instance using KVM passthrough
2. **`oem/install.bat`** runs on first boot to install OpenSSH and create a test user
3. **`run.sh`** orchestrates: wait for boot → wait for SSH → deploy → test
4. **`smoke-test.ps1`** runs inside Windows and validates:
   - Binary exists and is a valid PE
   - App launches without crashing
   - WebView2 runtime is available
   - Config directory gets created on first run
   - Clean exit (no file locks)
   - Multi-instance behavior

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
- `results.log` — full console output
- `shared/smoke-results.json` — machine-readable JSON

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
