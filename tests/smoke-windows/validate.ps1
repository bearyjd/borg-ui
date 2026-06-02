# BorgUI Windows validation pass.
#
# Unlike smoke-test.ps1 (which compiles the workspace and runs the Rust unit
# tests), this script validates the *Windows-specific runtime surfaces* the app
# depends on, by driving the real native tools directly from PowerShell:
#
#   - borg.exe        full backup -> restore round-trip (the backup engine)
#   - reg.exe         HKCU\...\Run round-trip (autostart at login)
#   - schtasks.exe    task create/query/delete round-trip (scheduled backups)
#
# Why PowerShell-native instead of `cargo test`: the bundled borg.exe is a
# PyInstaller bundle that hangs at spawn when launched by the Rust test binary
# under a console-less SSH session (documented in HANDOFF.md). The same borg.exe
# works when launched from a real console, so this pass drives it from
# PowerShell — which is also closer to how the shipped GUI (with a window
# station) actually runs it. The Rust-side argument construction for reg/schtasks
# is unit-tested separately in borg-platform-win; here we confirm the operations
# themselves succeed and round-trip on a real Windows build.

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Results = @()

function Write-TestHeader($name) {
    Write-Host "`n--- VALIDATE: $name ---" -ForegroundColor Cyan
}

function Pass($name, $detail) {
    $script:Passed++
    $script:Results += @{ Name = $name; Status = "PASS"; Detail = $detail }
    Write-Host "  PASS: $name" -ForegroundColor Green
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}

function Fail($name, $detail) {
    $script:Failed++
    $script:Results += @{ Name = $name; Status = "FAIL"; Detail = $detail }
    Write-Host "  FAIL: $name" -ForegroundColor Red
    if ($detail) { Write-Host "        $detail" -ForegroundColor Yellow }
}

$work = Join-Path $env:TEMP "borgui-validate"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work | Out-Null

# ------------------------------------------------------------------
# Locate (or download) the Windows borg.exe the app bundles.
# ------------------------------------------------------------------
Write-TestHeader "borg_install"

$borgDir = "C:\borg"
$borgExe = $null
$existing = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if ($existing) {
    $borgExe = $existing.FullName
} else {
    try {
        $zip = "$env:TEMP\borg-windows.zip"
        $url = "https://github.com/marcpope/borg-windows/releases/download/v1.4.4-win6/borg-windows.zip"
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        New-Item -ItemType Directory -Force -Path $borgDir | Out-Null
        Expand-Archive -Path $zip -DestinationPath $borgDir -Force
        $found = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $borgExe = $found.FullName }
    } catch {
        Fail "borg_install" "Download/extract failed: $_"
    }
}
if ($borgExe) {
    $ver = (& $borgExe --version 2>&1 | Out-String).Trim()
    Pass "borg_install" "borg.exe at $borgExe ($ver)"
} elseif ($script:Results.Count -eq 0) {
    Fail "borg_install" "borg.exe not found"
}

# The same non-interactive environment the app sets (borg.rs::base_command_with),
# so borg never blocks on a console prompt during the headless validation.
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"

function Get-Sha($path) { (Get-FileHash -Algorithm SHA256 -Path $path).Hash }

# ------------------------------------------------------------------
# borg backup -> restore round-trip, unencrypted (the core engine path).
# ------------------------------------------------------------------
Write-TestHeader "borg_roundtrip_unencrypted"

if (-not $borgExe) {
    Fail "borg_roundtrip_unencrypted" "borg.exe unavailable"
} else {
    try {
        $env:BORG_PASSPHRASE = ""
        $repo = Join-Path $work "repo-plain"
        $src = Join-Path $work "src-plain"
        $out = Join-Path $work "out-plain"
        New-Item -ItemType Directory -Force -Path $src, $out | Out-Null
        "alpha contents" | Out-File -FilePath (Join-Path $src "alpha.txt") -Encoding ascii -NoNewline
        [IO.File]::WriteAllBytes((Join-Path $src "beta.bin"), ([byte[]](0,1,2,3,255,254)))

        & $borgExe init --encryption none $repo 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "init failed (rc=$LASTEXITCODE)" }
        Push-Location $src
        & $borgExe create "$repo::arch1" . 2>&1 | Out-Null
        $createRc = $LASTEXITCODE
        Pop-Location
        if ($createRc -ne 0) { throw "create failed (rc=$createRc)" }

        $list = & $borgExe list $repo 2>&1 | Out-String
        if ($list -notmatch "arch1") { throw "archive 'arch1' missing from list" }

        Push-Location $out
        & $borgExe extract "$repo::arch1" 2>&1 | Out-Null
        $extractRc = $LASTEXITCODE
        Pop-Location
        if ($extractRc -ne 0) { throw "extract failed (rc=$extractRc)" }

        # Byte-for-byte verify the restored files match the originals.
        $okText = (Get-Sha (Join-Path $src "alpha.txt")) -eq (Get-Sha (Join-Path $out "alpha.txt"))
        $okBin = (Get-Sha (Join-Path $src "beta.bin")) -eq (Get-Sha (Join-Path $out "beta.bin"))
        if ($okText -and $okBin) {
            Pass "borg_roundtrip_unencrypted" "init -> create -> list -> extract -> byte-verify OK"
        } else {
            Fail "borg_roundtrip_unencrypted" "restored bytes differ (text=$okText bin=$okBin)"
        }
    } catch {
        Fail "borg_roundtrip_unencrypted" "$_"
    }
}

# ------------------------------------------------------------------
# borg round-trip, encrypted (repokey-blake2 + passphrase) — the recommended
# production configuration. Also proves encryption is real: listing without the
# passphrase must fail.
# ------------------------------------------------------------------
Write-TestHeader "borg_roundtrip_encrypted"

if (-not $borgExe) {
    Fail "borg_roundtrip_encrypted" "borg.exe unavailable"
} else {
    try {
        $pass = "correct horse battery staple"
        $repo = Join-Path $work "repo-enc"
        $src = Join-Path $work "src-enc"
        $out = Join-Path $work "out-enc"
        New-Item -ItemType Directory -Force -Path $src, $out | Out-Null
        "top secret data" | Out-File -FilePath (Join-Path $src "secret.txt") -Encoding ascii -NoNewline

        $env:BORG_PASSPHRASE = $pass
        $env:BORG_NEW_PASSPHRASE = $pass
        & $borgExe init --encryption repokey-blake2 $repo 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "encrypted init failed (rc=$LASTEXITCODE)" }
        Push-Location $src
        & $borgExe create "$repo::enc1" . 2>&1 | Out-Null
        $createRc = $LASTEXITCODE
        Pop-Location
        if ($createRc -ne 0) { throw "encrypted create failed (rc=$createRc)" }

        # Listing WITHOUT the passphrase must fail (proves the repo is encrypted).
        $env:BORG_PASSPHRASE = "wrong-passphrase"
        & $borgExe list $repo 2>&1 | Out-Null
        $wrongRc = $LASTEXITCODE
        $env:BORG_PASSPHRASE = $pass
        if ($wrongRc -eq 0) { throw "listing succeeded with a wrong passphrase — encryption not enforced" }

        Push-Location $out
        & $borgExe extract "$repo::enc1" 2>&1 | Out-Null
        $extractRc = $LASTEXITCODE
        Pop-Location
        if ($extractRc -ne 0) { throw "encrypted extract failed (rc=$extractRc)" }

        if ((Get-Sha (Join-Path $src "secret.txt")) -eq (Get-Sha (Join-Path $out "secret.txt"))) {
            Pass "borg_roundtrip_encrypted" "encrypted round-trip OK; wrong passphrase correctly rejected"
        } else {
            Fail "borg_roundtrip_encrypted" "restored bytes differ after encrypted round-trip"
        }
    } catch {
        Fail "borg_roundtrip_encrypted" "$_"
    } finally {
        Remove-Item Env:\BORG_NEW_PASSPHRASE -ErrorAction SilentlyContinue
        $env:BORG_PASSPHRASE = ""
    }
}

# ------------------------------------------------------------------
# Autostart: reg.exe round-trip on the HKCU Run key (mirrors
# borg-platform-win::autostart). Uses a throwaway value name so a real install's
# entry is never touched. The shipped value is the quoted exe path plus
# --minimized; the Rust side's exact quoting is unit-tested separately, so here
# we keep the value simple and assert the operations round-trip.
# ------------------------------------------------------------------
Write-TestHeader "autostart_registry_roundtrip"

try {
    $runKey = "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
    $testName = "BorgUI-ValidateSmoke"
    $value = "C:\fake\BorgUI.exe --minimized"

    & reg.exe add $runKey /V $testName /T REG_SZ /D $value /F 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg add failed (rc=$LASTEXITCODE)" }

    $query = & reg.exe query $runKey /V $testName 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "reg query failed (rc=$LASTEXITCODE)" }
    if ($query -notmatch "minimized") { throw "stored value missing '--minimized': $query" }

    & reg.exe delete $runKey /V $testName /F 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg delete failed (rc=$LASTEXITCODE)" }

    & reg.exe query $runKey /V $testName 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "value still present after delete" }

    Pass "autostart_registry_roundtrip" "add -> query -> delete on HKCU Run key OK"
} catch {
    # Leave no test value behind even on failure.
    & reg.exe delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /V "BorgUI-ValidateSmoke" /F 2>&1 | Out-Null
    Fail "autostart_registry_roundtrip" "$_"
}

# ------------------------------------------------------------------
# Scheduling: schtasks.exe round-trip (mirrors borg-platform-win::scheduler).
# Uses a throwaway task name; the action mirrors the real "--scheduled-backup"
# command the app registers.
# ------------------------------------------------------------------
Write-TestHeader "schtasks_roundtrip"

try {
    $taskName = "BorgUI-ValidateSmoke-Backup"
    $tr = "C:\fake\BorgUI.exe --scheduled-backup"

    & schtasks.exe /Create /F /TN $taskName /TR $tr /SC HOURLY /MO 1 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "schtasks /Create failed (rc=$LASTEXITCODE)" }

    & schtasks.exe /Query /TN $taskName 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "schtasks /Query failed (rc=$LASTEXITCODE)" }

    & schtasks.exe /Delete /F /TN $taskName 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "schtasks /Delete failed (rc=$LASTEXITCODE)" }

    Pass "schtasks_roundtrip" "create -> query -> delete OK"
} catch {
    & schtasks.exe /Delete /F /TN "BorgUI-ValidateSmoke-Backup" 2>&1 | Out-Null
    Fail "schtasks_roundtrip" "$_"
}

# ------------------------------------------------------------------
# Cleanup + summary
# ------------------------------------------------------------------
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

Write-Host "`n========================================" -ForegroundColor White
Write-Host "  WINDOWS VALIDATION RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Total:  $($script:Passed + $script:Failed)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$resultsPath = Join-Path $env:USERPROFILE "validate-results.json"
$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath $resultsPath -Encoding UTF8
Write-Host "Results written to $resultsPath"

# NOTE: still requires manual confirmation on real hardware (cannot be asserted
# headlessly): the Tauri window actually appears, the tray icon + menu work,
# `--minimized` lands in the tray, a scheduled task fires the headless run, the
# console-window flash is gone (CREATE_NO_WINDOW), and the OS keychain stores the
# passphrase in Windows Credential Manager.

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
