#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
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
            echo "Usage: $0 {all|validate-all|quick|vm|ssh|setup-ssh|build-env|deploy|test|validate|status|down}"
            exit 1
            ;;
    esac
}

main "$@"
