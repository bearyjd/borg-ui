# BorgUI Windows GUI-validation pass (the five real-desktop items).
#
# Confirms behaviours that the engine-level validate.ps1 can't: they need the
# actual Tauri binary (borg-ui.exe) and/or a real Credential Manager. The five
# HANDOFF "still needs a real desktop" items, by tier:
#
#   Tier A (scriptable, but needs the interactive session):
#     5. keychain  -> the keychain module persists a passphrase to Windows
#                     Credential Manager. Compiled over SSH, then RUN in session 1
#                     via an /IT task -- Credential Manager is unreachable from the
#                     SSH session (ERROR_NO_SUCH_LOGON_SESSION), verified on the VM.
#   Tier B (interactive launch, file-checkable result):
#     3. scheduled -> a registered Task Scheduler entry running
#                     `borg-ui.exe --scheduled-backup` actually produces a backup
#                     (a history.json success event + a new archive in the repo).
#   Tier C (interactive launch + visual confirm -> SIGNAL only, never gates):
#     1. window/tray, 2. --minimized, 4. console flash -> best-effort process /
#                     window-handle signals; the verdict is the VNC checklist in
#                     README.md (a GUI launched over SSH renders in no desktop).
#
# Mirrors validate.ps1 / validate-edge.ps1: Pass/Fail/Skip + JSON + exit code,
# every borg call hard-bounded by Invoke-Borg so a hang can never block the run.
# Tier A/B that cannot run (no borg-ui.exe / no toolchain) SKIP -- they never
# fail falsely and never pass falsely. ASCII only (Windows PowerShell 5.1 reads
# UTF-8-without-BOM as ANSI and breaks parsing).

param([int]$AppTimeoutSec = 20, [int]$ScheduledPollSec = 150)

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
$script:Results = @()

function Write-TestHeader($name) { Write-Host "`n--- VALIDATE-GUI: $name ---" -ForegroundColor Cyan }
function Pass($name, $detail) {
    $script:Passed++; $script:Results += @{ Name = $name; Status = "PASS"; Detail = $detail }
    Write-Host "  PASS: $name" -ForegroundColor Green
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
function Fail($name, $detail) {
    $script:Failed++; $script:Results += @{ Name = $name; Status = "FAIL"; Detail = $detail }
    Write-Host "  FAIL: $name" -ForegroundColor Red
    if ($detail) { Write-Host "        $detail" -ForegroundColor Yellow }
}
function Skip($name, $detail) {
    $script:Skipped++; $script:Results += @{ Name = $name; Status = "SKIP"; Detail = $detail }
    Write-Host "  SKIP: $name" -ForegroundColor Yellow
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
# Tier-C visual items: recorded for the record but never gate the exit code.
function Signal($name, $detail) {
    $script:Results += @{ Name = $name; Status = "SIGNAL"; Detail = $detail }
    Write-Host "  SIGNAL: $name" -ForegroundColor Magenta
    if ($detail) { Write-Host "          $detail" -ForegroundColor DarkGray }
}

# Non-interactive borg environment (mirrors borg.rs::base_command_with).
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName

# Locate the Tauri app binary. Tried in order: a conventional drop dir, the SSH
# home (run.sh scp's shared/borg-ui.exe here), and an on-VM release build.
function Find-BorgUi {
    $candidates = @(
        "C:\borgui\borg-ui.exe",
        (Join-Path $env:USERPROFILE "borg-ui.exe"),
        "C:\borgui-test\app-tauri\src-tauri\target\release\borg-ui.exe"
    )
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    $found = Get-ChildItem C:\borgui-test -Recurse -Filter borg-ui.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { return $found.FullName }
    return $null
}
$script:BorgUiExe = Find-BorgUi

# borg-ui.exe resolves borg as <its own dir>\borg.exe (see lib.rs). The bundled
# borg.exe (1.4.4+win6) is a PyInstaller ONEDIR bundle: it needs its sibling
# _internal\ (python311.dll + deps) right next to it, or it dies at startup with
# "[PYI] Failed to load Python DLL ..._internal\python311.dll" (verified on the
# VM). So copy the WHOLE borg distribution beside borg-ui.exe, not just the .exe.
function Ensure-BorgBeside($appExe) {
    if (-not $script:BorgExe) { return $false }
    $appDir = Split-Path $appExe -Parent
    $beside = Join-Path $appDir "borg.exe"
    $internal = Join-Path $appDir "_internal"
    $borgDir = Split-Path $script:BorgExe -Parent
    if ($borgDir -ieq $appDir) { return (Test-Path $beside) -and (Test-Path $internal) }
    if (-not (Test-Path $beside) -or -not (Test-Path $internal)) {
        try { Copy-Item (Join-Path $borgDir "*") -Destination $appDir -Recurse -Force } catch { return $false }
    }
    return (Test-Path $beside) -and (Test-Path $internal)
}

# Run a borg subcommand with a hard timeout so a hang can never block the run.
function Invoke-Borg {
    param([string[]]$BorgArgs, [int]$TimeoutSec = 40, [string]$Cwd)
    $o = Join-Path $env:TEMP "gui-o.txt"; $e = Join-Path $env:TEMP "gui-e.txt"
    $params = @{
        FilePath = $script:BorgExe; ArgumentList = $BorgArgs; WindowStyle = "Hidden"
        PassThru = $true; RedirectStandardOutput = $o; RedirectStandardError = $e
    }
    if ($Cwd) { $params["WorkingDirectory"] = $Cwd }
    $p = Start-Process @params
    if (-not $p.WaitForExit($TimeoutSec * 1000)) {
        try { $p.Kill() } catch {}
        return @{ TimedOut = $true; ExitCode = $null; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
    }
    $p.WaitForExit()
    return @{ TimedOut = $false; ExitCode = $p.ExitCode; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
}
# Mirror RepoConfig::location(): X:\rest -> \\localhost\X$\rest
function To-Unc($absPath) { "\\localhost\" + $absPath.Substring(0, 1) + "$" + $absPath.Substring(2) }

if (-not $script:BorgExe) { Fail "borg_install" "borg.exe not found under C:\borg" }
if ($script:BorgUiExe) {
    Write-Host "borg-ui.exe: $script:BorgUiExe" -ForegroundColor DarkGray
} else {
    Write-Host "borg-ui.exe: NOT FOUND (Tier B/C will SKIP). Drop one in tests/smoke-windows/shared/ or build on the VM; see README." -ForegroundColor Yellow
}

# ==================================================================
# Tier A -- item 5: keychain persists to Windows Credential Manager.
# Authoritative check is the gated Rust round-trip test (real keyring code path).
# SKIP cleanly if the source tree / cargo toolchain isn't on this VM.
# ==================================================================
Write-TestHeader "keychain_credential_manager"
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$srcDir = "C:\borgui-test"
$kcTask = "BorgUI-KeychainS1"
$kcOut = Join-Path $env:USERPROFILE "kc-session1.txt"
$kcBat = Join-Path $env:USERPROFILE "kc-run.bat"
if (-not (Test-Path $cargo)) {
    Skip "keychain_credential_manager" "cargo not found at $cargo -- run 'make build-env' + 'make deploy', then re-run (toolchain-free engine checks live in validate.ps1)"
} elseif (-not (Test-Path (Join-Path $srcDir "Cargo.toml"))) {
    Skip "keychain_credential_manager" "source tree not deployed to $srcDir -- run 'make deploy' first"
} else {
    # CRITICAL: Windows Credential Manager is UNREACHABLE from this SSH session --
    # a network logon raises ERROR_NO_SUCH_LOGON_SESSION (verified on the VM). So
    # COMPILE the gated test here, then RUN it in the interactive desktop
    # (session 1) via an /IT scheduled task -- the only context where keyring can
    # reach Credential Manager. Needs borgtest logged in at the desktop (the
    # dockur VM auto-logs-in, so this normally holds).
    try {
        $bo = Join-Path $env:TEMP "kc-build-o.txt"; $be = Join-Path $env:TEMP "kc-build-e.txt"
        $env:CARGO_NET_OFFLINE = "true"   # avoid the sparse-index network stall (HANDOFF gotcha)
        # Build the test binary only (running it here over SSH would fail on CredMan).
        # stdout/stderr MUST be different files (Start-Process rejects identical).
        $b = Start-Process -FilePath $cargo `
            -ArgumentList @("test", "--no-run", "-p", "borg-ui", "--lib") `
            -WorkingDirectory $srcDir -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput $bo -RedirectStandardError $be
        if (-not $b.WaitForExit(900 * 1000)) { try { $b.Kill() } catch {}; throw "cargo test --no-run timed out (build hung)" }
        $b.WaitForExit()
        # Start-Process -PassThru .ExitCode is unreliable when redirecting (often
        # $null even on success -- same gotcha validate.ps1 calls out), so judge
        # the build by cargo's own output + the produced exe, not the exit code.
        $bout = (Get-Content $bo -Raw -EA SilentlyContinue) + (Get-Content $be -Raw -EA SilentlyContinue)
        if ($bout -match "could not compile") {
            $btail = ($bout -split "`n" | Select-Object -Last 6) -join " | "
            throw "test binary failed to compile: $btail"
        }
        $testExe = (Get-ChildItem (Join-Path $srcDir "target\debug\deps") -Filter "borg_ui_lib-*.exe" -EA SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
        if (-not $testExe) { throw "compiled test binary (borg_ui_lib-*.exe) not found under target\debug\deps" }

        # Batch the session-1 run; write to the user profile (session 1's unelevated
        # token can't write C:\ root). STARTED + BATCH_EXIT sentinels bracket it.
        Remove-Item $kcOut -EA SilentlyContinue
        @(
            "@echo off",
            "echo STARTED > `"$kcOut`"",
            "set BORGUI_KEYCHAIN_TEST=1",
            "`"$testExe`" keychain::tests::windows_credential_manager_roundtrip --exact --nocapture >> `"$kcOut`" 2>&1",
            "echo BATCH_EXIT=%ERRORLEVEL% >> `"$kcOut`""
        ) | Set-Content -Path $kcBat -Encoding Ascii

        & schtasks.exe /Create /F /TN $kcTask /TR "`"$kcBat`"" /SC ONCE /ST 23:59 /IT 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Create failed (rc=$LASTEXITCODE)" }
        & schtasks.exe /Run /TN $kcTask 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Run failed (rc=$LASTEXITCODE)" }

        # Bounded poll for the batch to finish (never hang).
        $deadline = (Get-Date).AddSeconds(120)
        while ((Get-Date) -lt $deadline) {
            if ((Test-Path $kcOut) -and (Select-String -Path $kcOut -Pattern "BATCH_EXIT" -EA SilentlyContinue)) { break }
            Start-Sleep -Seconds 3
        }
        $kc = if (Test-Path $kcOut) { Get-Content $kcOut -Raw } else { "" }

        # Order matters: a self-skipped #[test] also exits ok, so require the
        # real-path marker for a PASS. ERROR_NO_SUCH_LOGON_SESSION here means even
        # session 1 had no interactive logon -> SKIP (precondition), not a defect.
        if ($kc -match "KEYCHAIN_ROUNDTRIP_OK") {
            Pass "keychain_credential_manager" "keyring round-trip verified in Credential Manager (session 1: set -> get -> cmdkey -> clear)"
        } elseif ($kc -match "ERROR_NO_SUCH_LOGON_SESSION") {
            Skip "keychain_credential_manager" "Credential Manager unreachable (ERROR_NO_SUCH_LOGON_SESSION) -- no interactive desktop; log borgtest in at localhost:8006 and re-run"
        } elseif ($kc -match "SKIP: Windows-only") {
            Skip "keychain_credential_manager" "test self-skipped (BORGUI_KEYCHAIN_TEST not honored in session 1)"
        } elseif (-not $kc) {
            Skip "keychain_credential_manager" "session-1 task produced no output (no interactive desktop? borgtest not logged in?) -- needs a desktop session"
        } else {
            $ktail = ($kc -split "`n" | Select-Object -Last 6) -join " | "
            Fail "keychain_credential_manager" "session-1 keychain test did not pass: $ktail"
        }
    } catch {
        Fail "keychain_credential_manager" "$_"
    } finally {
        & schtasks.exe /Delete /F /TN $kcTask 2>&1 | Out-Null
        Remove-Item $kcBat, $kcOut -EA SilentlyContinue
        Remove-Item Env:\CARGO_NET_OFFLINE -EA SilentlyContinue
    }
}

# ==================================================================
# Tier B -- item 3: a scheduled task actually fires the headless backup.
# Stage a profile (local repo via the UNC fix) -> register an interactive
# `--scheduled-backup` task -> /Run it -> assert a history success event + a new
# archive. Highest-ROI scriptable confirmation of the whole scheduled path.
# ==================================================================
Write-TestHeader "scheduled_task_fires"
$taskName = "BorgUI-SmokeBackup"
if (-not $script:BorgUiExe) {
    Skip "scheduled_task_fires" "borg-ui.exe not available (see note above)"
} elseif (-not (Ensure-BorgBeside $script:BorgUiExe)) {
    Skip "scheduled_task_fires" "could not place borg.exe beside borg-ui.exe"
} else {
    $work = "C:\borgui-gui"
    $configDir = Join-Path $env:APPDATA "com.borgui.app"
    try {
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
        $src = Join-Path $work "src"; $repoAbs = Join-Path $work "repo"
        New-Item -ItemType Directory -Force -Path $src | Out-Null
        New-Item -ItemType Directory -Force -Path $repoAbs | Out-Null
        "scheduled-smoke-payload" | Out-File (Join-Path $src "data.txt") -Encoding ascii -NoNewline
        $repoUnc = To-Unc $repoAbs

        # Repo is intentionally `--encryption none`: the runner's passphrase
        # lookup (keychain by repo.ssh_url()) is expected to MISS, so the
        # ssh_url()-vs-location() keying detail is irrelevant here and the backup
        # still succeeds. Don't add encryption to this staged profile without
        # also storing a passphrase, or the run would silently use none.
        # The runner does `create`, not `init` -> initialise the repo first.
        $r = Invoke-Borg @("init", "--encryption", "none", $repoUnc) 40
        if ($r.TimedOut) { throw "borg init hung on $repoUnc (admin share unavailable?)" }
        if (-not (Test-Path $repoAbs)) { throw "repo not created (stderr: $($r.Stderr))" }

        # Stage the active profile the runner reads (profiles.rs shape). The
        # schedule's OWN source_paths are what a scheduled run backs up.
        New-Item -ItemType Directory -Force -Path $configDir | Out-Null
        $profiles = @{
            active_id = "default"
            profiles  = @(@{
                    id              = "default"
                    name            = "GuiSmoke"
                    repo            = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = $repoAbs; ssh_key_path = $null }
                    schedule        = @{ enabled = $true; source_paths = @($src); schedule = @{ type = "hourly" }; excludes = @() }
                    retention       = $null
                    archive_template = $null
                    pre_backup      = $null
                    post_backup     = $null
                })
        }
        # Clear any stale history so the success we assert is THIS run's. A
        # swallowed delete (locked file) would let a prior success false-pass, so
        # verify it is actually gone.
        $historyPath = Join-Path $configDir "history.json"
        Remove-Item -Force $historyPath -EA SilentlyContinue
        if (Test-Path $historyPath) { throw "could not clear stale history.json (file locked?)" }
        # -InputObject (not the pipeline) so a single-element nested array isn't
        # unwrapped to a scalar by PowerShell 5.1's ConvertTo-Json.
        $profilesJson = ConvertTo-Json -InputObject $profiles -Depth 8
        $profilesJson | Out-File (Join-Path $configDir "profiles.json") -Encoding ascii

        # Register an interactive task that runs as the logged-in user, mirroring
        # save_schedule_config's command shape (TR = "<exe>" --scheduled-backup).
        $tr = '"' + $script:BorgUiExe + '" --scheduled-backup'
        & schtasks.exe /Create /F /TN $taskName /TR $tr /SC ONCE /ST 00:00 /IT 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Create failed (rc=$LASTEXITCODE)" }
        & schtasks.exe /Run /TN $taskName 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Run failed (rc=$LASTEXITCODE)" }

        # Poll (bounded) for the runner to record a success event. Never hang.
        $deadline = (Get-Date).AddSeconds($ScheduledPollSec)
        $event = $null
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 5
            if (Test-Path $historyPath) {
                try {
                    $events = Get-Content $historyPath -Raw | ConvertFrom-Json
                    $event = @($events) | Where-Object { $_.kind -eq "backup" -and $_.outcome -eq "success" } | Select-Object -First 1
                    if ($event) { break }
                    $failEvt = @($events) | Where-Object { $_.outcome -eq "failure" } | Select-Object -First 1
                    if ($failEvt) { break }
                } catch {}
            }
        }

        # LastTaskResult via the cmdlet (an int; 0 = success) is locale-independent,
        # unlike grepping schtasks' localized "Last Result" label.
        $lastResult = try { "LastTaskResult=" + (Get-ScheduledTaskInfo -TaskName $taskName -EA Stop).LastTaskResult } catch { "LastTaskResult=unknown" }
        if ($event) {
            # Confirm the archive really exists by matching its name in the repo's
            # archive list (more robust than Start-Process's flaky ExitCode).
            $listOut = Invoke-Borg @("list", "--short", $repoUnc) 30
            $archiveOk = (-not $listOut.TimedOut) -and ("$($listOut.Stdout)" -match [regex]::Escape($event.archive_name))
            if ($archiveOk) {
                Pass "scheduled_task_fires" "task fired -> backup '$($event.archive_name)' succeeded and is listable in the repo ($lastResult)"
            } else {
                Fail "scheduled_task_fires" "history shows success '$($event.archive_name)' but borg could not list it (rc=$($listOut.ExitCode))"
            }
        } else {
            $failEvt = $null
            if (Test-Path $historyPath) {
                try { $failEvt = @((Get-Content $historyPath -Raw | ConvertFrom-Json)) | Where-Object { $_.outcome -eq "failure" } | Select-Object -First 1 } catch {}
            }
            if ($failEvt) {
                Fail "scheduled_task_fires" "runner recorded a FAILURE: $($failEvt.error_message)"
            } else {
                Fail "scheduled_task_fires" "no history event within ${ScheduledPollSec}s ($lastResult). App may not have launched (WebView2 missing? session 0?) -- run 'make build-env' and ensure borgtest is logged in."
            }
        }
    } catch {
        Fail "scheduled_task_fires" "$_"
    } finally {
        & schtasks.exe /Delete /F /TN $taskName 2>&1 | Out-Null
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
        # The scheduled `--scheduled-backup` instance exits itself (app.exit) --
        # don't kill borg-ui by name here; it could match a real app instance.
    }
}

# ==================================================================
# Tier C -- items 1/2/4: window/tray, --minimized, console flash.
# SIGNAL only. A GUI launched over SSH renders in no desktop, so these never
# gate the exit code; they record best-effort process/window-handle evidence and
# defer the verdict to the README VNC checklist (run in session 1).
# ==================================================================
function Probe-App($label, $appArgs) {
    if (-not $script:BorgUiExe) { Signal $label "borg-ui.exe not available"; return }
    $proc = $null
    try {
        if ($appArgs) {
            $proc = Start-Process -FilePath $script:BorgUiExe -ArgumentList $appArgs -PassThru -EA Stop
        } else {
            $proc = Start-Process -FilePath $script:BorgUiExe -PassThru -EA Stop
        }
        Start-Sleep -Seconds $AppTimeoutSec
        $live = Get-Process -Id $proc.Id -EA SilentlyContinue
        if (-not $live) {
            Signal $label "pid $($proc.Id) exited within ${AppTimeoutSec}s (expected over SSH with no desktop; launch in session 1 / VNC and use the README checklist)"
        } else {
            $h = $live.MainWindowHandle; $t = $live.MainWindowTitle
            Signal $label "pid $($proc.Id) alive; MainWindowHandle=$h MainWindowTitle='$t' (reflects the SSH desktop, not session 1 -- confirm via VNC)"
        }
    } catch {
        Signal $label "launch error: $_"
    } finally {
        # Only stop the instance THIS probe launched -- never kill borg-ui by
        # image name (it could match a real app the user is running in session 1).
        if ($proc -and (Get-Process -Id $proc.Id -EA SilentlyContinue)) {
            Stop-Process -Id $proc.Id -Force -EA SilentlyContinue
        }
    }
}
Write-TestHeader "gui_window_present (signal)"
Probe-App "gui_window_present" $null
Write-TestHeader "gui_minimized_hidden (signal)"
Probe-App "gui_minimized_hidden" @("--minimized")
Write-TestHeader "console_flash (manual)"
Signal "console_flash" "Not programmatically assertable (a borg console flash is sub-second). Trigger a backup from the app over VNC and confirm NO black console window appears -- see README checklist."

# ==================================================================
Write-Host "`n========================================" -ForegroundColor White
Write-Host "  GUI VALIDATION RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $script:Skipped" -ForegroundColor Yellow
Write-Host "  Signals (Tier C, manual verdict): see README VNC checklist" -ForegroundColor Magenta
Write-Host "  Total gating: $($script:Passed + $script:Failed + $script:Skipped)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath (Join-Path $env:USERPROFILE "gui-results.json") -Encoding UTF8

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
