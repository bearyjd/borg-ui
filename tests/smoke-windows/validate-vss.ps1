# BorgUI VSS validation: does the REAL production backup path take a VSS snapshot
# and store clean, restorable paths -- including a file that is exclusively locked?
#
# This drives the actual app entry point `borg-ui.exe --scheduled-backup` (which
# runs scheduled.rs::run_scheduled_backup -> vss::prepare_snapshot -> mount the
# shadow as a junction -> borg.create with cwd=junction -> release). The feasibility
# of the mechanism was proven by validate-vss-spike.ps1 with raw commands; THIS
# proves the shipped Rust does it through the production code path.
#
# The clincher is the LOCKED FILE. Before the backup runs we open one source file
# with FileShare::None (an exclusive lock). Then:
#   - VSS active  -> borg reads the file from the frozen snapshot -> it IS archived.
#   - live fallback (no VSS) -> borg cannot open the locked live file -> it is SKIPPED.
# So "the locked file is present in the archive" is a definitive signal that the
# production VSS path engaged end-to-end -- not merely that a backup ran.
#
# Requirements:
#   - a PRODUCTION-or-dev borg-ui.exe built from CURRENT source (the --scheduled-backup
#     path never opens the GUI window, so a `cargo build --release -p borg-ui` exe is
#     fine -- no tauri build / pnpm needed).
#   - the task runs /IT /RL HIGHEST: interactive session 1 (so borg spawns with a
#     window station, avoiding the console-less spawn hang) AND elevated (VSS snapshot
#     creation needs admin; the C$ admin-share repo rewrite needs admin too).
#   - borgtest logged in at the desktop (the dockur VM auto-logs-in).
#
# Mirrors validate-gui.ps1: Pass/Fail/Skip + JSON + exit code; every borg call hard-
# bounded by Invoke-Borg. ASCII only (Windows PowerShell 5.1 reads UTF-8-without-BOM
# as ANSI and breaks parsing).

param([int]$ScheduledPollSec = 180)

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
$script:Results = @()

function Write-TestHeader($name) { Write-Host "`n--- VALIDATE-VSS: $name ---" -ForegroundColor Cyan }
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

# Non-interactive borg environment (mirrors borg.rs::base_command_with), used only
# for our own out-of-band init/list/extract probes.
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName

function Find-BorgUi {
    $candidates = @(
        "C:\borgui-test\target\release\borg-ui.exe",
        "C:\borgui-test\app-tauri\src-tauri\target\release\borg-ui.exe",
        (Join-Path $env:USERPROFILE "borg-ui.exe"),
        "C:\borgui\borg-ui.exe"
    )
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    $found = Get-ChildItem C:\borgui-test -Recurse -Filter borg-ui.exe -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($found) { return $found.FullName }
    return $null
}
$script:BorgUiExe = Find-BorgUi

# borg-ui.exe resolves borg as <its own dir>\borg.exe; the PyInstaller ONEDIR
# bundle needs its sibling _internal\ too. Copy the whole borg dist beside it.
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

function Invoke-Borg {
    param([string[]]$BorgArgs, [int]$TimeoutSec = 40)
    $o = Join-Path $env:TEMP "vss-o.txt"; $e = Join-Path $env:TEMP "vss-e.txt"
    $p = Start-Process -FilePath $script:BorgExe -ArgumentList $BorgArgs -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $o -RedirectStandardError $e
    if (-not $p.WaitForExit($TimeoutSec * 1000)) {
        try { $p.Kill() } catch {}
        return @{ TimedOut = $true; ExitCode = $null; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
    }
    $p.WaitForExit()
    return @{ TimedOut = $false; ExitCode = $p.ExitCode; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
}
function Get-Sha($path) { (Get-FileHash -Algorithm SHA256 -Path $path).Hash }
function To-Unc($absPath) { "\\localhost\" + $absPath.Substring(0, 1) + "$" + $absPath.Substring(2) }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

Write-TestHeader "preconditions"
if (-not $script:BorgExe) {
    Fail "preconditions" "borg.exe not found under C:\borg"
} elseif (-not $script:BorgUiExe) {
    Skip "preconditions" "borg-ui.exe (current build) not found -- run 'make deploy' then build 'cargo build --release -p borg-ui' on the VM"
} elseif (-not (Ensure-BorgBeside $script:BorgUiExe)) {
    Skip "preconditions" "could not place borg.exe + _internal beside borg-ui.exe"
} else {
    Pass "preconditions" "borg-ui.exe: $script:BorgUiExe; borg beside it OK; admin=$isAdmin"
}

# ==================================================================
# The real production VSS path: --scheduled-backup with a locked source file.
# ==================================================================
$taskName = "BorgUI-VssBackup"
$work = "C:\borgui-vss"
$configDir = Join-Path $env:APPDATA "com.borgui.app"
$profilesPath = Join-Path $configDir "profiles.json"
$profilesBak = "$profilesPath.vss-bak"
$lockStream = $null

if ($script:Skipped -gt 0 -or $script:Failed -gt 0) {
    Skip "vss_scheduled_backup_locked_file" "preconditions not met"
    Skip "vss_clean_paths" "preconditions not met"
    Skip "vss_restore_roundtrip" "preconditions not met"
} else {
    Write-TestHeader "vss_scheduled_backup_locked_file"
    try {
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
        $src = Join-Path $work "src"; $repoAbs = Join-Path $work "repo"; $out = Join-Path $work "out"
        New-Item -ItemType Directory -Force -Path $src, $repoAbs, $out | Out-Null

        $normalFile = Join-Path $src "normal.txt"
        $lockedFile = Join-Path $src "locked.txt"
        "vss-normal-payload" | Out-File $normalFile -Encoding ascii -NoNewline
        "vss-locked-payload-" + (Get-Date).Ticks | Out-File $lockedFile -Encoding ascii -NoNewline
        $normalSha = Get-Sha $normalFile
        $lockedSha = Get-Sha $lockedFile
        $repoUnc = To-Unc $repoAbs

        # The runner does `create`, not `init` -> initialise the repo first.
        $r = Invoke-Borg @("init", "--encryption", "none", $repoUnc) 40
        if ($r.TimedOut) { throw "borg init hung on $repoUnc (admin share unavailable?)" }
        if (-not (Test-Path $repoAbs)) { throw "repo not created (stderr: $($r.Stderr))" }

        # Stage the active profile the runner reads (profiles.rs shape). The
        # schedule's OWN source_paths are what a scheduled run backs up.
        New-Item -ItemType Directory -Force -Path $configDir | Out-Null
        if ((Test-Path $profilesPath) -and -not (Test-Path $profilesBak)) { Copy-Item $profilesPath $profilesBak -Force }
        $profiles = @{
            active_id = "default"
            profiles  = @(@{
                    id               = "default"
                    name             = "VssSmoke"
                    repo             = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = $repoAbs; ssh_key_path = $null }
                    schedule         = @{ enabled = $true; source_paths = @($src); schedule = @{ type = "hourly" }; excludes = @() }
                    retention        = $null
                    archive_template = $null
                    pre_backup       = $null
                    post_backup      = $null
                })
        }
        $historyPath = Join-Path $configDir "history.json"
        Remove-Item -Force $historyPath -EA SilentlyContinue
        if (Test-Path $historyPath) { throw "could not clear stale history.json (file locked?)" }
        $profilesJson = ConvertTo-Json -InputObject $profiles -Depth 8
        $profilesJson | Out-File $profilesPath -Encoding ascii

        # Register the REAL command shape (TR = "<exe>" --scheduled-backup), but
        # /RL HIGHEST so the session-1 instance is ELEVATED -- VSS snapshot
        # creation (and the C$ admin share) need admin. /IT = interactive session.
        $tr = '"' + $script:BorgUiExe + '" --scheduled-backup'
        & schtasks.exe /Create /F /TN $taskName /TR $tr /SC ONCE /ST 00:00 /IT /RL HIGHEST 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Create failed (rc=$LASTEXITCODE)" }

        # Exclusively lock locked.txt on the LIVE volume BEFORE the backup runs.
        # A live-file backup cannot open it (share violation -> skipped); only a
        # VSS snapshot read (the frozen copy) can capture it. Held across the run.
        $lockStream = [System.IO.File]::Open($lockedFile, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)

        & schtasks.exe /Run /TN $taskName 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "schtasks /Run failed (rc=$LASTEXITCODE)" }

        # Bounded poll for the runner to record a success (or failure) event.
        $deadline = (Get-Date).AddSeconds($ScheduledPollSec)
        $successEvt = $null; $failEvt = $null
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 5
            if (Test-Path $historyPath) {
                try {
                    $events = @(Get-Content $historyPath -Raw | ConvertFrom-Json)
                    $successEvt = $events | Where-Object { $_.kind -eq "backup" -and $_.outcome -eq "success" } | Select-Object -First 1
                    if ($successEvt) { break }
                    $failEvt = $events | Where-Object { $_.outcome -eq "failure" } | Select-Object -First 1
                    if ($failEvt) { break }
                } catch {}
            }
        }

        # Release the lock now the backup has run (so list/extract can read it).
        if ($lockStream) { $lockStream.Close(); $lockStream.Dispose(); $lockStream = $null }

        if (-not $successEvt) {
            if ($failEvt) { throw "runner recorded a FAILURE: $($failEvt.error_message)" }
            throw "no history event within ${ScheduledPollSec}s -- app may not have launched (session 0? not elevated? not logged in?)"
        }
        $archiveName = $successEvt.archive_name

        # Read the stored listing once; reused by the clean-path + locked-file checks.
        $listing = Invoke-Borg @("list", "$repoUnc::$archiveName") 40
        if ($listing.TimedOut) { throw "borg list hung" }
        $listOut = "$($listing.Stdout)"

        # THE CLINCHER: the exclusively-locked file is in the archive => the
        # snapshot (frozen copy) was used; a live fallback would have skipped it.
        if ($listOut -notmatch 'normal\.txt') {
            throw "archive is missing normal.txt -- backup did not capture the sources: $listOut"
        }
        if ($listOut -match 'locked\.txt') {
            Pass "vss_scheduled_backup_locked_file" "the exclusively-locked file is in the archive '$archiveName' -- the production --scheduled-backup path took a VSS snapshot (a live backup would have skipped it)"
        } else {
            throw "the locked file is ABSENT from the archive -- the backup fell back to live files (no VSS snapshot); listing: $listOut"
        }

        # ----- clean, restorable paths (no shadow-device markers) -----
        Write-TestHeader "vss_clean_paths"
        if ($listOut -match 'GLOBALROOT' -or $listOut -match '\?' -or $listOut -match 'HarddiskVolumeShadowCopy') {
            Fail "vss_clean_paths" "stored paths still contain shadow-copy markers (un-restorable): $listOut"
        } else {
            Pass "vss_clean_paths" "stored paths are clean (no GLOBALROOT/?/HarddiskVolumeShadowCopy): $(($listOut -split "`n" | Where-Object { $_ -match 'locked\.txt' } | Select-Object -First 1))"
        }

        # ----- restore round-trip: both files extract byte-correct -----
        Write-TestHeader "vss_restore_roundtrip"
        # Extract with cwd=out (borg writes the volume-relative tree under it).
        $rx = Start-Process -FilePath $script:BorgExe -ArgumentList @("extract", "$repoUnc::$archiveName") `
            -WindowStyle Hidden -PassThru -WorkingDirectory $out `
            -RedirectStandardOutput (Join-Path $env:TEMP "vss-x-o.txt") -RedirectStandardError (Join-Path $env:TEMP "vss-x-e.txt")
        if (-not $rx.WaitForExit(60000)) { $rx.Kill(); throw "extract hung" }
        # borg stores volume-relative paths; find the restored files by leaf name.
        $rn = Get-ChildItem $out -Recurse -Filter normal.txt -EA SilentlyContinue | Select-Object -First 1
        $rl = Get-ChildItem $out -Recurse -Filter locked.txt -EA SilentlyContinue | Select-Object -First 1
        if (-not $rn -or -not $rl) {
            Fail "vss_restore_roundtrip" "restore did not write both files (normal=$([bool]$rn) locked=$([bool]$rl))"
        } elseif ((Get-Sha $rn.FullName) -eq $normalSha -and (Get-Sha $rl.FullName) -eq $lockedSha) {
            Pass "vss_restore_roundtrip" "both files (incl. the locked one) restored byte-correct from the VSS archive"
        } else {
            Fail "vss_restore_roundtrip" "restored bytes did not match the originals"
        }
    } catch {
        Fail "vss_scheduled_backup_locked_file" "$_"
    } finally {
        if ($lockStream) { try { $lockStream.Close(); $lockStream.Dispose() } catch {} }
        & schtasks.exe /Delete /F /TN $taskName 2>&1 | Out-Null
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
        if (Test-Path $profilesBak) { Move-Item $profilesBak $profilesPath -Force }
        else { Remove-Item $profilesPath -EA SilentlyContinue }
        # The app's SnapshotPlan::release() deletes its own snapshot + junction;
        # surface a warning if any VSS snapshot lingered (a release leak), but do
        # not delete it blindly -- we cannot tell ours from a system snapshot.
        $lingering = @(Get-WmiObject Win32_ShadowCopy -EA SilentlyContinue).Count
        if ($lingering -gt 0) { Write-Host "  NOTE: $lingering VSS snapshot(s) present after the run (verify the app released its own)" -ForegroundColor DarkYellow }
    }
}

Write-Host "`n========================================" -ForegroundColor White
Write-Host "  VSS VALIDATION RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $script:Skipped" -ForegroundColor Yellow
Write-Host "  Total: $($script:Passed + $script:Failed + $script:Skipped)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath (Join-Path $env:USERPROFILE "vss-results.json") -Encoding UTF8

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
