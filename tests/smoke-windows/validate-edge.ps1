# BorgUI Windows edge-case validation.
#
# Validates two cases for native drive-letter repositories:
#
#   -Mode admin     (run as the admin user): a real second drive D: exists, and a
#                   repo on D: restores to C: (cross-drive round-trip - the case a
#                   relative repo can't satisfy).
#   -Mode nonadmin  (run as a STANDARD user): a local C: repo completes a full
#                   round-trip without requiring an administrative share.
#
# Mirrors validate.ps1 (Pass/Fail/JSON/exit, Invoke-Borg timeout). ASCII only
# (Windows PowerShell 5.1 reads UTF-8-without-BOM as ANSI and breaks parsing).

param([ValidateSet("admin", "nonadmin")] [string]$Mode = "admin")

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
$script:Results = @()

function Write-TestHeader($name) { Write-Host "`n--- VALIDATE($Mode): $name ---" -ForegroundColor Cyan }
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

# Non-interactive borg environment (mirrors borg.rs::base_command_with).
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

$borgDir = "C:\borg"
$script:BorgExe = (Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
$borgVersion = if ($script:BorgExe) { (& $script:BorgExe --version 2>&1 | Out-String).Trim() } else { "" }
if ($Mode -eq "admin" -and $borgVersion -notmatch "1\.4\.4\+win7") {
    Write-TestHeader "borg_win7_install"
    try {
        $zip = Join-Path $env:TEMP "borg-windows-win7.zip"
        $url = "https://github.com/marcpope/borg-windows/releases/download/v1.4.4-win7/borg-windows.zip"
        Remove-Item -Recurse -Force $borgDir -ErrorAction SilentlyContinue
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne "c19e37475bad7ddc7db3e8f4f414b8a7824e920067f84d06998d4771bf12ee06") {
            throw "download checksum mismatch"
        }
        New-Item -ItemType Directory -Force -Path $borgDir | Out-Null
        Expand-Archive -Path $zip -DestinationPath $borgDir -Force
        $script:BorgExe = (Get-ChildItem $borgDir -Recurse -Filter borg.exe | Select-Object -First 1).FullName
        $borgVersion = (& $script:BorgExe --version 2>&1 | Out-String).Trim()
        if ($borgVersion -notmatch "1\.4\.4\+win7") { throw "unexpected version: $borgVersion" }
        Pass "borg_win7_install" "$borgVersion"
    } catch {
        Fail "borg_win7_install" "$_"
    }
}

# Run a borg subcommand with a hard timeout so a hang can never block the run.
function Invoke-Borg {
    param([string[]]$BorgArgs, [int]$TimeoutSec = 40, [string]$Cwd)
    $o = Join-Path $env:TEMP "edge-o.txt"; $e = Join-Path $env:TEMP "edge-e.txt"
    $params = @{
        FilePath = $script:BorgExe; ArgumentList = $BorgArgs; WindowStyle = "Hidden"
        PassThru = $true; RedirectStandardOutput = $o; RedirectStandardError = $e
    }
    if ($Cwd) { $params["WorkingDirectory"] = $Cwd }
    $p = Start-Process @params
    if (-not $p.WaitForExit($TimeoutSec * 1000)) {
        try { $p.Kill() } catch {}
        return @{ TimedOut = $true; ExitCode = $null; Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
    }
    $p.WaitForExit()
    return @{ TimedOut = $false; ExitCode = $p.ExitCode; Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
}
function Get-Sha($path) { (Get-FileHash -Algorithm SHA256 -Path $path).Hash }

if (-not $script:BorgExe) {
    Fail "borg_install" "borg.exe not found under C:\borg"
} elseif ($borgVersion -notmatch "1\.4\.4\+win7") {
    Fail "borg_install" "expected 1.4.4+win7, found '$borgVersion'"
}

# ==================================================================
if ($Mode -eq "admin") {
    # --- D: drive present ---
    Write-TestHeader "second_drive_present"
    $dOk = (Test-Path "D:\") -and ((Get-Volume -DriveLetter D -EA SilentlyContinue).FileSystem -eq "NTFS")
    if ($dOk) {
        Pass "second_drive_present" "D: is NTFS"
    } else {
        # Not a failure of the code under test: dockur only provisions a second
        # disk on a FRESH install, not when recreating a persisted volume. Skip.
        Skip "second_drive_present" "no NTFS D: drive. dockur adds DISK2 only on a fresh install: run 'docker compose down -v && make edge-all'"
    }

    # --- multi-drive: repo on D:, restore to C: (cross-cwd, cross-drive) ---
    Write-TestHeader "multi_drive_cross_restore"
    if ($script:BorgExe -and $dOk) {
        try {
            $work = "C:\borgui-edge"; Remove-Item -Recurse -Force $work -EA SilentlyContinue
            $src = Join-Path $work "src"; $out = Join-Path $work "out"
            New-Item -ItemType Directory -Force -Path $src, $out | Out-Null
            "cross-drive-payload" | Out-File (Join-Path $src "data.txt") -Encoding ascii -NoNewline

            $absRepo = "D:\borgui-edge\repo"
            Remove-Item -Recurse -Force "D:\borgui-edge" -EA SilentlyContinue
            New-Item -ItemType Directory -Force -Path "D:\borgui-edge" | Out-Null
            $r = Invoke-Borg @("init", "--encryption", "none", $absRepo) 40
            if ($r.TimedOut) { throw "init on D: hung" }
            if (-not (Test-Path $absRepo)) { throw "repo not created on D: (stderr: $($r.Stderr))" }
            # create with cwd on C:, extract with cwd on C: (a DIFFERENT drive than the repo)
            $r = Invoke-Borg @("create", "$absRepo::a1", "src") 60 $work
            if ($r.TimedOut) { throw "create hung" }
            $r = Invoke-Borg @("extract", "$absRepo::a1") 60 $out
            if ($r.TimedOut) { throw "extract hung" }

            $restored = Join-Path $out "src\data.txt"
            if ((Test-Path $restored) -and ((Get-Sha (Join-Path $src "data.txt")) -eq (Get-Sha $restored))) {
                Pass "multi_drive_cross_restore" "repo on D:, restore to C: round-trips byte-for-byte"
            } else {
                Fail "multi_drive_cross_restore" "cross-drive restore did not byte-match (restored exists: $(Test-Path $restored))"
            }
        } catch {
            Fail "multi_drive_cross_restore" "$_"
        } finally {
            Remove-Item -Recurse -Force "C:\borgui-edge", "D:\borgui-edge" -EA SilentlyContinue
        }
    } elseif (-not $script:BorgExe) {
        Fail "multi_drive_cross_restore" "borg.exe unavailable"
    } else {
        Skip "multi_drive_cross_restore" "no D: drive to test cross-drive restore (see second_drive_present)"
    }
}

# ==================================================================
if ($Mode -eq "nonadmin") {
    Write-TestHeader "non_admin_admin_share_not_required"
    $cAccessible = Test-Path "\\localhost\C$\" -ErrorAction SilentlyContinue
    if (-not $cAccessible) {
        Pass "non_admin_admin_share_not_required" "\\localhost\C$ is inaccessible as $(whoami), as expected"
    } else {
        Fail "non_admin_admin_share_not_required" "\\localhost\C$ is reachable as $(whoami) - user is not actually standard/non-admin"
    }

    Write-TestHeader "non_admin_local_repo_roundtrip"
    if ($script:BorgExe) {
        $root = Join-Path $env:TEMP "edge_na"
        $absRepo = Join-Path $root "repo"
        $src = Join-Path $root "src"
        $out = Join-Path $root "out"
        Remove-Item -Recurse -Force $root -EA SilentlyContinue
        New-Item -ItemType Directory -Force -Path $src, $out | Out-Null
        "non-admin-payload" | Out-File (Join-Path $src "data.txt") -Encoding ascii -NoNewline
        try {
            $r = Invoke-Borg @("init", "--encryption", "none", $absRepo) 40
            if ($r.TimedOut) { throw "init hung" }
            if (-not (Test-Path $absRepo)) { throw "repo not created: $($r.Stderr)" }
            $r = Invoke-Borg @("create", "$absRepo::a1", "src") 60 $root
            if ($r.TimedOut) { throw "create hung" }
            $r = Invoke-Borg @("extract", "$absRepo::a1") 60 $out
            if ($r.TimedOut) { throw "extract hung" }
            $restored = Join-Path $out "src\data.txt"
            if ((Test-Path $restored) -and ((Get-Sha (Join-Path $src "data.txt")) -eq (Get-Sha $restored))) {
                Pass "non_admin_local_repo_roundtrip" "raw drive-letter repo works as standard user without C$ access"
            } else {
                throw "restored data did not byte-match"
            }
        } catch {
            Fail "non_admin_local_repo_roundtrip" "$_"
        }
        Remove-Item -Recurse -Force $root -EA SilentlyContinue
    } else {
        Fail "non_admin_local_repo_roundtrip" "borg.exe unavailable"
    }
}

# ==================================================================
Write-Host "`n========================================" -ForegroundColor White
Write-Host "  EDGE VALIDATION ($Mode)" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $script:Skipped" -ForegroundColor Yellow
Write-Host "  Total: $($script:Passed + $script:Failed + $script:Skipped)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath (Join-Path $env:USERPROFILE "edge-results-$Mode.json") -Encoding UTF8

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
