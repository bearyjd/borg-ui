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
        Fail "second_drive_present" "D: NTFS=$dOk  D`$ share=$dShare (provision the 2nd disk first)"
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
    } else {
        Fail "multi_drive_cross_restore" "prerequisite missing (borg.exe or D:)"
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

    Write-TestHeader "non_admin_local_repo_fast_fail"
    # borg init on the UNC form the app produces for a local repo -> must fail
    # FAST (admin share denied), never hang. A timeout here is a regression.
    if ($script:BorgExe) {
        $absRepo = Join-Path $env:TEMP "edge_na\repo"   # under this user's own temp, on C:
        $uncRepo = To-Unc $absRepo                      # \\localhost\C$\Users\<user>\...\repo
        $r = Invoke-Borg @("init", "--encryption", "none", $uncRepo) 25
        if ($r.TimedOut) {
            Fail "non_admin_local_repo_fast_fail" "borg HUNG on a non-admin local repo (regression - the anti-hang guarantee is broken)"
        } elseif ($r.ExitCode -ne 0 -and -not (Test-Path $absRepo)) {
            Pass "non_admin_local_repo_fast_fail" "fast non-zero failure (rc=$($r.ExitCode)), no hang, no repo - as expected for a standard user"
        } else {
            Fail "non_admin_local_repo_fast_fail" "unexpected: rc=$($r.ExitCode) repoCreated=$(Test-Path $absRepo)"
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
Write-Host "  Total: $($script:Passed + $script:Failed)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath (Join-Path $env:USERPROFILE "edge-results-$Mode.json") -Encoding UTF8

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
