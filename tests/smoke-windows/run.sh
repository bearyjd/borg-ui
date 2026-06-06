#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Make the local container-engine shim (.bin/docker) resolvable from this
# non-interactive script. Inside a distrobox the `docker`->host-podman alias only
# exists in interactive shells, so without this `docker compose` wouldn't resolve
# here. Harmless elsewhere: when no shim is present this is a no-op and we fall
# through to the real docker/podman on PATH.
if [[ -x "$SCRIPT_DIR/.bin/docker" ]]; then
    PATH="$SCRIPT_DIR/.bin:$PATH"
fi

SSH_USER="borgtest"
SSH_PASS="Password1!"
SSH_PORT=2222
SSH_HOST="localhost"
MAX_BOOT_WAIT=1800
MAX_SSH_WAIT=1200

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}[smoke]${NC} $*"; }
warn() { echo -e "${YELLOW}[smoke]${NC} $*"; }
fail() { echo -e "${RED}[smoke]${NC} $*" >&2; exit 1; }

SSH_CMD="sshpass -p $SSH_PASS ssh -o StrictHostKeyChecking=no -o ConnectTimeout=15 -o ServerAliveInterval=30 -o ServerAliveCountMax=240 -o TCPKeepAlive=yes -p $SSH_PORT $SSH_USER@$SSH_HOST"
SCP_CMD="sshpass -p $SSH_PASS scp -o StrictHostKeyChecking=no -P $SSH_PORT"

# A STANDARD (non-admin) identity for the non-admin edge validation. Same password;
# created by provision_edge / oem/install.bat. SSH lets any local user in.
STD_USER="borgstd"
SSH_STD_CMD="sshpass -p $SSH_PASS ssh -o StrictHostKeyChecking=no -o ConnectTimeout=15 -o ServerAliveInterval=30 -o ServerAliveCountMax=240 -o TCPKeepAlive=yes -p $SSH_PORT $STD_USER@$SSH_HOST"

cleanup() {
    if [[ "${KEEP_VM:-}" != "1" ]]; then
        log "Tearing down Windows container..."
        docker compose -f "$SCRIPT_DIR/docker-compose.yml" down 2>/dev/null || true
    else
        warn "KEEP_VM=1 — leaving container running (borgui-smoke-win)"
    fi
}

trap cleanup EXIT

# --- Phase 1: Start Windows container ---
start_vm() {
    log "Starting Windows Docker container..."
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" up -d

    log "Waiting for Windows to boot (up to ${MAX_BOOT_WAIT}s)..."
    local elapsed=0
    while ! docker logs borgui-smoke-win 2>&1 | grep -qi "started successfully\|ready"; do
        sleep 15
        elapsed=$((elapsed + 15))
        if [[ $elapsed -ge $MAX_BOOT_WAIT ]]; then
            docker logs borgui-smoke-win 2>&1 | tail -20
            fail "Windows did not boot within ${MAX_BOOT_WAIT}s"
        fi
        printf "."
    done
    echo ""
    log "Windows container booted after ${elapsed}s"
}

# --- Phase 2: Wait for SSH ---
wait_for_ssh() {
    log "Waiting for SSH (up to ${MAX_SSH_WAIT}s)..."
    log "If SSH takes too long, install it via QEMU monitor (make setup-ssh)"
    local elapsed=0
    while ! $SSH_CMD "Write-Host ok" &>/dev/null; do
        sleep 15
        elapsed=$((elapsed + 15))
        if [[ $elapsed -ge $MAX_SSH_WAIT ]]; then
            fail "SSH not available after ${MAX_SSH_WAIT}s. Run 'make setup-ssh' to install OpenSSH via QEMU monitor."
        fi
        printf "."
    done
    echo ""
    log "SSH connection established"
}

# --- Phase 3: Setup Windows build environment ---
setup_env() {
    log "Setting up Windows build environment..."

    $SSH_CMD '
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    if (Get-Command rustc -ErrorAction SilentlyContinue) {
        Write-Host "Rust already installed"
    } else {
        Write-Host "Installing Rust..."
        $url = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
        Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\rustup-init.exe" -UseBasicParsing
        Start-Process -FilePath "$env:TEMP\rustup-init.exe" -ArgumentList "-y --default-toolchain stable-x86_64-pc-windows-gnu --profile minimal" -Wait -NoNewWindow
        Write-Host "Rust installed"
    }

    if (Test-Path "C:\mingw64\bin\gcc.exe") {
        Write-Host "MinGW already installed"
    } else {
        Write-Host "Installing MinGW-w64..."
        $url = "https://github.com/niXman/mingw-builds-binaries/releases/download/14.2.0-rt_v12-rev1/x86_64-14.2.0-release-posix-seh-ucrt-rt_v12-rev1.7z"
        Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\mingw.7z" -UseBasicParsing

        if (-not (Test-Path "C:\Program Files\7-Zip\7z.exe")) {
            Write-Host "Installing 7zip..."
            Invoke-WebRequest -Uri "https://www.7-zip.org/a/7z2408-x64.msi" -OutFile "$env:TEMP\7z.msi" -UseBasicParsing
            Start-Process msiexec -ArgumentList "/i $env:TEMP\7z.msi /qn" -Wait
        }
        & "C:\Program Files\7-Zip\7z.exe" x "$env:TEMP\mingw.7z" -oC:\ -y | Select-Object -Last 1
        Write-Host "MinGW installed"
    }

    $wv2Bin = Get-ChildItem "C:\Program Files*\Microsoft\EdgeWebView" -Recurse -Filter "msedgewebview2.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($wv2Bin) {
        Write-Host "WebView2 already installed"
    } else {
        Write-Host "Installing WebView2..."
        Invoke-WebRequest -Uri "https://go.microsoft.com/fwlink/p/?LinkId=2124703" -OutFile "$env:TEMP\wv2.exe" -UseBasicParsing
        Start-Process -FilePath "$env:TEMP\wv2.exe" -ArgumentList "/silent /install" -Wait
        Write-Host "WebView2 installed"
    }

    Write-Host "Environment ready"
    '

    log "Environment setup complete"
}

# --- Phase 4: Deploy source code ---
deploy_source() {
    log "Packaging source code..."
    local tarball="/tmp/borg-smoke-src.tar.gz"
    (cd "$REPO_ROOT" && tar czf "$tarball" \
        --exclude='target' --exclude='node_modules' \
        --exclude='.svelte-kit' --exclude='.git' .)

    log "Uploading source to VM..."
    $SCP_CMD "$tarball" "$SSH_USER@$SSH_HOST:borg-src.tar.gz"

    $SSH_CMD '
    Remove-Item -Recurse -Force C:\borgui-test -ErrorAction SilentlyContinue
    mkdir C:\borgui-test -Force | Out-Null
    cd C:\borgui-test
    tar xzf $env:USERPROFILE\borg-src.tar.gz
    Write-Host "Source deployed to C:\borgui-test"
    '

    log "Source deployed"
}

# --- Phase 5: Run smoke tests ---
run_tests() {
    log "Uploading smoke test script..."
    $SCP_CMD "$SCRIPT_DIR/smoke-test.ps1" "$SSH_USER@$SSH_HOST:smoke-test.ps1"

    log "Running smoke tests..."
    # PowerShell stderr from cargo compile output causes non-zero exit even on success.
    # Capture output and check for our own pass/fail markers.
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\smoke-test.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/results.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "All smoke tests passed!"
        return 0
    else
        fail "Some smoke tests failed. See results.log"
    fi
}

# --- Phase 5b: Windows runtime validation (native tools, headless-safe) ---
# Drives borg.exe / reg.exe / schtasks.exe directly from PowerShell — unlike the
# smoke test it does NOT need the Rust source or toolchain, only a booted VM with
# SSH. Sidesteps the PyInstaller borg.exe spawn hang (see HANDOFF.md / validate.ps1).
run_validate() {
    log "Uploading validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate.ps1" "$SSH_USER@$SSH_HOST:validate.ps1"

    log "Running Windows validation pass..."
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "Windows validation passed!"
        return 0
    else
        fail "Windows validation failed. See validate.log"
    fi
}

# --- Edge provisioning: standard user + second drive (idempotent, over SSH) ---
# Works on a VM recreated with DISK2 (oem/install.bat only runs on a fresh first
# boot, so the warm/recreated VM is provisioned here as the admin user).
provision_edge() {
    log "Provisioning edge prerequisites (standard user + D: drive)..."
    $SSH_CMD '
    # Standard (non-admin) user borgstd.
    if (-not (Get-LocalUser -Name borgstd -ErrorAction SilentlyContinue)) {
        $p = ConvertTo-SecureString "Password1!" -AsPlainText -Force
        New-LocalUser -Name borgstd -Password $p -PasswordNeverExpires | Out-Null
        Write-Host "created borgstd (standard user)"
    } else { Write-Host "borgstd already exists" }

    # Second disk -> D: NTFS (idempotent: skip if D: already present).
    if (Test-Path D:\) {
        Write-Host "D: already present"
    } else {
        $raw = Get-Disk | Where-Object { $_.PartitionStyle -eq "RAW" } | Select-Object -First 1
        if (-not $raw) {
            Write-Host "WARNING: no raw disk found; recreate the VM with DISK2_SIZE set"
        } elseif ((Get-Volume -ErrorAction SilentlyContinue).DriveLetter -contains "D") {
            # D: is taken (commonly an optical drive). Dont fight it - the multi-drive
            # test will SKIP rather than mis-assign.
            Write-Host "WARNING: D: already in use (optical drive?); cannot assign the multi-drive volume to D:"
        } else {
            Initialize-Disk -Number $raw.Number -PartitionStyle GPT -PassThru |
                New-Partition -DriveLetter D -UseMaximumSize |
                Format-Volume -FileSystem NTFS -NewFileSystemLabel BORGD -Confirm:$false | Out-Null
            Write-Host "initialized D: from raw disk $($raw.Number)"
        }
    }
    '
    log "Edge provisioning complete"
}

# --- Edge validation: multi-drive (admin) + non-admin fast-fail (standard user) ---
run_validate_edge() {
    log "Uploading edge validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate-edge.ps1" "$SSH_USER@$SSH_HOST:validate-edge.ps1"
    # borgstd needs its own copy in its home for the non-admin run.
    $SCP_CMD "$SCRIPT_DIR/validate-edge.ps1" "$STD_USER@$SSH_HOST:validate-edge.ps1"

    log "Running multi-drive validation (admin user)..."
    local admin_out
    admin_out=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-edge.ps1 -Mode admin' 2>&1) || true
    echo "$admin_out" | tee "$SCRIPT_DIR/validate-edge.log"

    log "Running non-admin validation (standard user borgstd)..."
    local std_out
    std_out=$($SSH_STD_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-edge.ps1 -Mode nonadmin' 2>&1) || true
    echo "$std_out" | tee -a "$SCRIPT_DIR/validate-edge.log"

    if echo "$admin_out" | grep -q "Failed: 0" && echo "$std_out" | grep -q "Failed: 0"; then
        log "Edge validation passed (multi-drive + non-admin)!"
        return 0
    else
        fail "Edge validation failed. See validate-edge.log"
    fi
}

# --- GUI validation: keychain (Tier A) + scheduled-firing (Tier B) + Tier-C signals ---
# Confirms the five real-desktop items the engine validation can't. Tier A/B that
# can't run (no borg-ui.exe / no toolchain) SKIP cleanly. Tier C (window/tray,
# --minimized, console flash) is inherently visual: this prints signals and the
# operator finishes the verdict with the README VNC checklist.
run_validate_gui() {
    log "Uploading GUI validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate-gui.ps1" "$SSH_USER@$SSH_HOST:validate-gui.ps1"

    # If a pre-built app binary was dropped in shared/, push it to the VM home so
    # validate-gui.ps1 finds it (dockur does not surface ./shared inside Windows).
    if [[ -f "$SCRIPT_DIR/shared/borg-ui.exe" ]]; then
        log "Uploading shared/borg-ui.exe to the VM..."
        $SCP_CMD "$SCRIPT_DIR/shared/borg-ui.exe" "$SSH_USER@$SSH_HOST:borg-ui.exe"
    else
        warn "no shared/borg-ui.exe -- Tier B/C will SKIP (drop one in shared/ or build on the VM; see README)"
    fi

    log "Running GUI validation pass..."
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-gui.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate-gui.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "GUI validation passed (no Tier A/B failures). Finish Tier C via the README VNC checklist."
        return 0
    else
        fail "GUI validation failed. See validate-gui.log"
    fi
}

# --- Tray-menu validation (#34): right-click menu contents + Show/Quit actions ---
# Drives the tray context menu via UI Automation. The script self-relaunches in
# session 1 (an /IT task) because the notification area + UIA need a real desktop;
# if the icon can't be located (Win11 overflow quirks) the checks SKIP, never
# false-fail, and the README VNC checklist is the verdict. Needs borg-ui.exe.
run_validate_tray() {
    log "Uploading tray-menu validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate-tray.ps1" "$SSH_USER@$SSH_HOST:validate-tray.ps1"

    if [[ -f "$SCRIPT_DIR/shared/borg-ui.exe" ]]; then
        log "Uploading shared/borg-ui.exe to the VM..."
        $SCP_CMD "$SCRIPT_DIR/shared/borg-ui.exe" "$SSH_USER@$SSH_HOST:borg-ui.exe"
    else
        warn "no shared/borg-ui.exe -- tray checks will SKIP (drop one in shared/ or build on the VM; see README)"
    fi

    log "Running tray-menu validation pass (relaunches itself in session 1)..."
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-tray.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate-tray.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "Tray validation: no failures. SKIPs mean the icon wasn't locatable -- finish via the README VNC checklist."
        return 0
    else
        fail "Tray validation failed. See validate-tray.log"
    fi
}

# --- Interactive GUI-flow validation: tray->backup nav, settings profile switch, ---
# --- GUI restore round-trip, cancel-mid-backup. Drives the live Svelte UI via UIA. ---
# REQUIRES a PRODUCTION borg-ui.exe (real `tauri build`, embedded frontend) at
# C:\borgui-test\target\release\borg-ui.exe -- a dev-mode `cargo build` exe shows
# the localhost-error page and the WebView UI isn't reachable. The script
# self-relaunches in session 1 and launches the app with
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-renderer-accessibility so UIA can
# see the Svelte content. Stages its own repos/profiles; restores config after.
run_validate_gui_flows() {
    log "Uploading GUI-flow validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate-gui-flows.ps1" "$SSH_USER@$SSH_HOST:validate-gui-flows.ps1"

    log "Running GUI-flow validation pass (relaunches itself in session 1)..."
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-gui-flows.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate-gui-flows.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "GUI-flow validation: no failures. (SKIPs mean no production exe / no desktop.)"
        return 0
    else
        fail "GUI-flow validation failed. See validate-gui-flows.log"
    fi
}

# --- Large-archive GUI smoke (#35): streaming + virtualized ArchiveBrowser ---
# Stages a ~100k-entry borg archive on the VM, then drives the production app's
# archive Browse view via UI Automation to prove the stream completes, the DOM
# stays windowed, "Select all" scales to the full count, scrolling recycles rows,
# and a selected subset restores byte-correct. The script self-relaunches in
# session 1 and stages/cleans up its own repo + profile. REQUIRES a PRODUCTION
# tauri-build exe at C:\borgui-test\target\release\borg-ui.exe.
run_validate_archive_smoke() {
    log "Uploading large-archive smoke script..."
    $SCP_CMD "$SCRIPT_DIR/validate-archive-smoke.ps1" "$SSH_USER@$SSH_HOST:validate-archive-smoke.ps1"

    log "Running large-archive (#35) smoke pass (stages 100k entries, relaunches in session 1)..."
    local output
    output=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-archive-smoke.ps1' 2>&1) || true
    echo "$output" | tee "$SCRIPT_DIR/validate-archive-smoke.log"

    if echo "$output" | grep -q "Failed: 0"; then
        log "Archive smoke: no failures. (SKIPs mean no production exe / no desktop / borg missing.)"
        return 0
    else
        fail "Archive smoke failed. See validate-archive-smoke.log"
    fi
}

# --- Autostart login-cycle validation: does the HKCU\Run value actually fire? ---
# Registers the Run value the app writes ("BorgUI" = "<exe>" --minimized), reboots
# the guest (a real login cycle: shutdown /r -> auto-login -> Explorer processes
# Run keys), then verifies borg-ui.exe auto-started in the interactive session
# with --minimized. The reg round-trip + --minimized->tray are validated elsewhere;
# this closes the "reboot actually launches it" gap. REQUIRES a production exe.
run_validate_autostart_login() {
    log "Uploading autostart-login validation script..."
    $SCP_CMD "$SCRIPT_DIR/validate-autostart-login.ps1" "$SSH_USER@$SSH_HOST:validate-autostart-login.ps1"

    log "Phase 1/2: registering the HKCU Run value (as the app's autostart writes it)..."
    local set_out
    set_out=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-autostart-login.ps1 -Phase set' 2>&1)
    echo "$set_out" | tee "$SCRIPT_DIR/validate-autostart-login.log"
    if ! echo "$set_out" | grep -q "SET-OK"; then
        if echo "$set_out" | grep -q "Failed: 0"; then
            log "Autostart-login SKIPPED (no production exe / value did not persist) -- nothing to verify."
            return 0
        fi
        fail "Autostart-login set phase failed. See output above."
    fi

    # Best-effort removal of our Run value, so a mid-run abort never leaves the
    # test exe auto-launching on the warm VM. No-op if SSH is unreachable.
    _clean_runkey() { $SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-autostart-login.ps1 -Phase clean' &>/dev/null || true; }

    log "Rebooting Windows (guest shutdown /r) to exercise a real login cycle..."
    # Record the guest boot time up front so we can PROVE the reboot actually
    # happened -- a no-op shutdown must never let us verify against the pre-reboot
    # system and report a false pass.
    local boot_before
    boot_before=$($SSH_CMD '(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.Ticks' 2>/dev/null | tr -d '[:space:]')
    # The shutdown command's own exit status is unreliable (the SSH session can drop
    # as the box goes down), so don't gate on it -- the drop + boot-time checks below
    # are the real proof.
    $SSH_CMD 'shutdown /r /t 0 /f' 2>&1 || true
    # Require sshd to actually drop; if it never does, the reboot did not happen
    # (e.g. shutdown needed elevation / was blocked).
    local i dropped=0
    for i in $(seq 1 36); do
        if ! $SSH_CMD "exit" &>/dev/null; then dropped=1; break; fi
        sleep 5
    done
    if [[ "$dropped" != "1" ]]; then
        _clean_runkey
        fail "Guest sshd never dropped after 'shutdown /r' -- reboot did not occur; refusing to verify against the pre-reboot system."
    fi
    log "Guest is rebooting; waiting for SSH to return..."
    wait_for_ssh
    # Confirm the boot time advanced; otherwise we'd be verifying the same boot.
    local boot_after
    boot_after=$($SSH_CMD '(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.Ticks' 2>/dev/null | tr -d '[:space:]')
    if [[ -n "$boot_before" && -n "$boot_after" && "$boot_after" == "$boot_before" ]]; then
        _clean_runkey
        fail "Guest boot time did not change ($boot_after) -- the machine did not actually reboot."
    fi
    log "SSH back (boot advanced); allowing the interactive auto-login + Run-key to fire..."
    sleep 30

    log "Phase 2/2: verifying borg-ui.exe was launched by the Run key (minimized)..."
    local out
    out=$($SSH_CMD 'powershell -ExecutionPolicy Bypass -File $env:USERPROFILE\validate-autostart-login.ps1 -Phase verify' 2>&1) || true
    echo "$out" | tee -a "$SCRIPT_DIR/validate-autostart-login.log"

    # Require a positive completion marker AND a real pass -- not merely the absence
    # of "Failed: 1". A SKIP (e.g. no interactive desktop) is surfaced but is NOT a
    # confirmed pass. The verify phase removes the Run value itself on every verdict.
    if echo "$out" | grep -q "VERIFY-COMPLETE"; then
        if echo "$out" | grep -Eq "VERIFY-COMPLETE Passed: [1-9]" && echo "$out" | grep -q "Failed: 0"; then
            log "Autostart-login validation passed -- the Run key fired the app at login."
            return 0
        elif echo "$out" | grep -q "Failed: 0"; then
            warn "Autostart-login SKIPPED (verify ran but confirmed nothing -- e.g. no interactive desktop). NOT a confirmed pass."
            return 0
        fi
    fi
    _clean_runkey
    fail "Autostart-login validation failed or did not complete. See validate-autostart-login.log"
}

# --- Setup SSH via QEMU monitor (for first boot) ---
setup_ssh() {
    log "Installing OpenSSH via QEMU monitor keystrokes..."
    docker exec borgui-smoke-win bash -c '
    send_string() {
        local str="$1"
        for (( i=0; i<${#str}; i++ )); do
            local c="${str:$i:1}"
            local key=""
            case "$c" in
                [a-z]) key="$c" ;;
                [A-Z]) key="shift-$(echo $c | tr A-Z a-z)" ;;
                " ") key="spc" ;;
                ".") key="dot" ;;
                "/") key="slash" ;;
                "\\") key="backslash" ;;
                "-") key="minus" ;;
                "=") key="equal" ;;
                ":") key="shift-semicolon" ;;
                ";") key="semicolon" ;;
                "!") key="shift-1" ;;
                [0-9]) key="$c" ;;
                "_") key="shift-minus" ;;
                ",") key="comma" ;;
                "~") key="shift-grave_accent" ;;
                *) continue ;;
            esac
            echo "sendkey $key" | nc -q1 -w1 localhost 7100 2>/dev/null
            sleep 0.1
        done
    }

    echo "sendkey meta_l-r" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 1
    send_string "powershell"
    sleep 0.3
    echo "sendkey ret" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 3
    send_string "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0"
    sleep 0.3
    echo "sendkey ret" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 15
    send_string "Start-Service sshd"
    sleep 0.3
    echo "sendkey ret" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 3
    send_string "Set-Service -Name sshd -StartupType Automatic"
    sleep 0.3
    echo "sendkey ret" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 2
    send_string "netsh advfirewall firewall add rule name=SSH dir=in action=allow protocol=TCP localport=22"
    sleep 0.3
    echo "sendkey ret" | nc -q1 -w1 localhost 7100 2>/dev/null
    sleep 2
    echo "DONE"
    '

    log "OpenSSH install keystrokes sent. Wait 30s then try SSH."
}

# --- Main ---
main() {
    log "=== BorgUI Windows Smoke Test ==="
    log "Repo: $REPO_ROOT"

    case "${1:-all}" in
        build-env) setup_env ;;
        vm)        start_vm ;;
        ssh)       wait_for_ssh ;;
        setup-ssh) setup_ssh ;;
        deploy)    deploy_source ;;
        test)      run_tests ;;
        validate)  run_validate ;;
        provision-edge) provision_edge ;;
        validate-edge)  run_validate_edge ;;
        validate-gui)   run_validate_gui ;;
        validate-tray)  run_validate_tray ;;
        validate-gui-flows) run_validate_gui_flows ;;
        validate-archive-smoke) run_validate_archive_smoke ;;
        validate-autostart-login) run_validate_autostart_login ;;
        all)
            start_vm
            wait_for_ssh
            setup_env
            deploy_source
            run_tests
            ;;
        validate-all)
            # Runtime validation only — no source deploy / toolchain needed.
            start_vm
            wait_for_ssh
            run_validate
            ;;
        edge-all)
            # Edge validation (non-admin + multi-drive). Recreates the VM so the
            # DISK2 from docker-compose attaches, provisions, then validates.
            start_vm
            wait_for_ssh
            provision_edge
            run_validate_edge
            ;;
        gui-all)
            # GUI validation (keychain + scheduled-firing + Tier-C signals) on a
            # running/booted VM. Needs borg-ui.exe (shared/ drop or on-VM build)
            # for Tier B/C and the deployed source + toolchain for the keychain
            # test; missing prerequisites SKIP rather than fail.
            start_vm
            wait_for_ssh
            run_validate_gui
            ;;
        tray-all)
            # Tray-menu validation (#34) on a running/booted VM. Needs borg-ui.exe
            # (shared/ drop or on-VM build) and an interactive desktop (session 1);
            # if the icon isn't locatable the checks SKIP -- finish via the VNC
            # checklist. Best run after the VM has auto-logged in to the desktop.
            start_vm
            wait_for_ssh
            run_validate_tray
            ;;
        gui-flows-all)
            # Interactive GUI-flow validation on a running/booted VM. Needs a
            # PRODUCTION borg-ui.exe (real tauri build) + an interactive desktop;
            # stages its own repos/profiles. SKIPs cleanly if prereqs are missing.
            start_vm
            wait_for_ssh
            run_validate_gui_flows
            ;;
        archive-smoke-all)
            # Large-archive (#35) GUI smoke on a running/booted VM. Needs a
            # PRODUCTION borg-ui.exe (real tauri build) + an interactive desktop;
            # stages its own 100k-entry repo + profile and cleans up. SKIPs
            # cleanly if prereqs are missing.
            start_vm
            wait_for_ssh
            run_validate_archive_smoke
            ;;
        autostart-login-all)
            # Autostart login-cycle validation on a running/booted VM: registers
            # the Run value, reboots the guest, and verifies the app auto-started.
            # Needs a PRODUCTION borg-ui.exe; reboots the VM (still warm after).
            start_vm
            wait_for_ssh
            run_validate_autostart_login
            ;;
        quick)
            # Skip VM boot — assume already running with SSH
            setup_env
            deploy_source
            run_tests
            ;;
        status)
            docker compose -f "$SCRIPT_DIR/docker-compose.yml" ps
            ;;
        down)
            KEEP_VM=0 cleanup
            trap - EXIT
            ;;
        *)
            echo "Usage: $0 {all|validate-all|edge-all|gui-all|tray-all|gui-flows-all|archive-smoke-all|autostart-login-all|quick|vm|ssh|setup-ssh|build-env|deploy|test|validate|provision-edge|validate-edge|validate-gui|validate-tray|validate-gui-flows|validate-archive-smoke|validate-autostart-login|status|down}"
            exit 1
            ;;
    esac
}

main "$@"
