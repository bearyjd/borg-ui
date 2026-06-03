# BorgUI Windows edge-case validation.
#
# Validates two cases the admin/single-disk VM couldn't exercise for the local-
# repo UNC fix (RepoConfig::location -> \\localhost\X$\...):
#
#   -Mode admin     (run as the admin user): a real second drive D: exists, and a
#                   repo on D: restores to C: (cross-drive round-trip - the case a
#                   relative repo can't satisfy).
#   -Mode nonadmin  (run as a STANDARD user): the \\localhost\C$ admin share is
#                   denied, and a local-repo init fails FAST (no hang).
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

$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName

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
# Mirror RepoConfig::location(): X:\rest -> \\localhost\X$\rest
function To-Unc($absPath) { "\\localhost\" + $absPath.Substring(0, 1) + "$" + $absPath.Substring(2) }

if (-not $script:BorgExe) {
    Fail "borg_install" "borg.exe not found under C:\borg"
}

# ==================================================================
if ($Mode -eq "admin") {
    # --- D: drive present + admin share reachable ---
    Write-TestHeader "second_drive_present"
    $dOk = (Test-Path "D:\") -and ((Get-Volume -DriveLetter D -EA SilentlyContinue).FileSystem -eq "NTFS")
    $dShare = Test-Path "\\localhost\D$\"
    if ($dOk -and $dShare) {
        Pass "second_drive_present" "D: is NTFS and \\localhost\D$ is reachable"
    } else {
        # Not a failure of the code under test: dockur only provisions a second
        # disk on a FRESH install, not when recreating a persisted volume. Skip.
        Skip "second_drive_present" "no D: drive (NTFS=$dOk share=$dShare). dockur adds DISK2 only on a fresh install: run 'docker compose down -v && make edge-all'"
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
            $uncRepo = To-Unc $absRepo   # \\localhost\D$\borgui-edge\repo

            $r = Invoke-Borg @("init", "--encryption", "none", $uncRepo) 40
            if ($r.TimedOut) { throw "init on D: hung" }
            if (-not (Test-Path $absRepo)) { throw "repo not created on D: (stderr: $($r.Stderr))" }
            # create with cwd on C:, extract with cwd on C: (a DIFFERENT drive than the repo)
            $r = Invoke-Borg @("create", "$uncRepo::a1", "src") 60 $work
            if ($r.TimedOut) { throw "create hung" }
            $r = Invoke-Borg @("extract", "$uncRepo::a1") 60 $out
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
    Write-TestHeader "non_admin_admin_share_denied"
    # A standard user must NOT be able to reach the C$ admin share. The access
    # denial is the expected result, so suppress the (noisy) error it raises.
    $cAccessible = Test-Path "\\localhost\C$\" -ErrorAction SilentlyContinue
    if (-not $cAccessible) {
        Pass "non_admin_admin_share_denied" "\\localhost\C$ is correctly inaccessible as $(whoami)"
    } else {
        Fail "non_admin_admin_share_denied" "\\localhost\C$ is reachable as $(whoami) - user is not actually standard/non-admin"
    }

    # Confirm the preflight's TRIGGER actually fires for a non-admin: accessing
    # \\localhost\C$ must raise ERROR_ACCESS_DENIED (Win32 5 / HResult 0x80070005,
    # surfaced by .NET as UnauthorizedAccessException). Rust's std::fs::metadata
    # deterministically maps ERROR_ACCESS_DENIED -> io::ErrorKind::PermissionDenied,
    # which is exactly RepoConfig::share_unreachable()'s trigger. So this proves
    # local_repo_preflight() returns the friendly error for this user.
    Write-TestHeader "preflight_trigger_matches"
    $denied = $false; $hr = 0
    try { [System.IO.Directory]::GetFileSystemEntries("\\localhost\C$\") | Out-Null }
    catch [System.UnauthorizedAccessException] { $denied = $true; $hr = $_.Exception.HResult }
    catch { $hr = $_.Exception.HResult }
    if ($denied -or $hr -eq -2147024891) {
        # -2147024891 == 0x80070005 (ERROR_ACCESS_DENIED) as a signed Int32
        Pass "preflight_trigger_matches" "C\$ raises ERROR_ACCESS_DENIED -> Rust PermissionDenied -> preflight fires (HResult=$hr)"
    } else {
        Fail "preflight_trigger_matches" "C\$ access did NOT raise ERROR_ACCESS_DENIED (HResult=$hr); local_repo_preflight may not fire - widen share_unreachable to match this errno"
    }

    Write-TestHeader "non_admin_local_repo_fast_fail"
    # borg init on the UNC form the app produces for a local repo -> must fail
    # FAST (admin share denied), never hang. A timeout here is a regression.
    if ($script:BorgExe) {
        $absRepo = Join-Path $env:TEMP "edge_na\repo"   # under this user's own temp, on C:
        $uncRepo = To-Unc $absRepo                      # \\localhost\C$\Users\<user>\...\repo
        $r = Invoke-Borg @("init", "--encryption", "none", $uncRepo) 25
        $stderr = "$($r.Stderr)".Trim()
        # Require evidence borg actually RAN and errored (non-empty stderr), not just
        # a non-zero/null exit code - distinguishes "ran and was denied" from
        # "never launched" (which would also leave the repo uncreated).
        if ($r.TimedOut) {
            Fail "non_admin_local_repo_fast_fail" "borg HUNG on a non-admin local repo (regression - the anti-hang guarantee is broken)"
        } elseif ($stderr.Length -gt 0 -and -not (Test-Path $absRepo)) {
            Pass "non_admin_local_repo_fast_fail" "borg ran and failed fast, no hang, no repo: $(($stderr -split [char]10)[0])"
        } else {
            Fail "non_admin_local_repo_fast_fail" "no evidence of a fast denial (rc=$($r.ExitCode), repoCreated=$(Test-Path $absRepo), stderr='$stderr')"
        }
        Remove-Item -Recurse -Force (Join-Path $env:TEMP "edge_na") -EA SilentlyContinue
    } else {
        Fail "non_admin_local_repo_fast_fail" "borg.exe unavailable"
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
