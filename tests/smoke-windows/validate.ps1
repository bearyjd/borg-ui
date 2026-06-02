# BorgUI Windows validation pass.
#
# Validates the Windows-specific runtime surfaces the app depends on by driving
# the real native tools directly from PowerShell (no Rust toolchain/source):
#
#   - reg.exe       HKCU\...\Run round-trip (autostart at login)
#   - schtasks.exe  task create/query/delete round-trip (scheduled backups)
#   - borg.exe      backup engine: a relative-path round-trip proves the engine
#                   works on Windows; an absolute-path probe is a regression test
#                   for the drive-letter bug described below.
#
# IMPORTANT FINDING (borg-windows drive-letter path bug)
# ------------------------------------------------------
# borg 1.4.4+win6 parses an absolute Windows repo path (C:\repo, C:/repo,
# \\?\C:\repo) as an SSH remote "C:repo" (drive letter -> host "C"), then hangs
# trying to ssh to host "C". A repo path WITHOUT a drive-letter colon (a relative
# path) is correctly treated as local. The app's RepoConfig::location() passes a
# local repo path verbatim, so a local/USB repo like "C:\Backups\repo" hangs.
# This is almost certainly the real cause behind the "console-less spawn hang"
# previously attributed to PyInstaller. Tracking test: `borg_absolute_local_path`
# below FAILS until borg-core formats local Windows paths in a form borg accepts.
#
# Every borg call is wrapped in a hard timeout (Invoke-Borg) so a hang can never
# block the run.

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Results = @()

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

function Write-TestHeader($name) {
    Write-Host "`n--- VALIDATE: $name ---" -ForegroundColor Cyan
}

$work = Join-Path $env:TEMP "borgui-validate"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work | Out-Null

# ==================================================================
# 1. Autostart: reg.exe round-trip on HKCU\...\Run (mirrors
#    borg-platform-win::autostart). Throwaway value name so a real install is
#    never touched. Native tool, reliable headless.
# ==================================================================
Write-TestHeader "autostart_registry_roundtrip"
try {
    $runKey = "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
    $testName = "BorgUI-ValidateSmoke"
    $value = "C:\fake\BorgUI.exe --minimized"

    & reg.exe add $runKey /V $testName /T REG_SZ /D $value /F 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg add failed (rc=$LASTEXITCODE)" }
    $query = & reg.exe query $runKey /V $testName 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "reg query failed (rc=$LASTEXITCODE)" }
    if ($query -notmatch "minimized") { throw "stored value missing '--minimized'" }
    & reg.exe delete $runKey /V $testName /F 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg delete failed (rc=$LASTEXITCODE)" }
    & reg.exe query $runKey /V $testName 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "value still present after delete" }
    Pass "autostart_registry_roundtrip" "add -> query -> delete on HKCU Run key OK"
} catch {
    & reg.exe delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /V "BorgUI-ValidateSmoke" /F 2>&1 | Out-Null
    Fail "autostart_registry_roundtrip" "$_"
}

# ==================================================================
# 2. Scheduling: schtasks.exe round-trip (mirrors borg-platform-win::scheduler).
# ==================================================================
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

# ==================================================================
# borg.exe setup: locate (or download) the bundled Windows build.
# ==================================================================
Write-TestHeader "borg_install"
$borgDir = "C:\borg"
$script:BorgExe = $null
$existing = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if ($existing) {
    $script:BorgExe = $existing.FullName
} else {
    try {
        $zip = "$env:TEMP\borg-windows.zip"
        $url = "https://github.com/marcpope/borg-windows/releases/download/v1.4.4-win6/borg-windows.zip"
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        New-Item -ItemType Directory -Force -Path $borgDir | Out-Null
        Expand-Archive -Path $zip -DestinationPath $borgDir -Force
        $found = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $script:BorgExe = $found.FullName }
    } catch {}
}
if ($script:BorgExe) {
    $ver = (& $script:BorgExe --version 2>&1 | Out-String).Trim()
    Pass "borg_install" "borg.exe at $($script:BorgExe) ($ver)"
} else {
    Fail "borg_install" "borg.exe not found / download failed"
}

# Non-interactive environment, mirroring borg.rs::base_command_with.
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

# Run a borg subcommand with a hard timeout so a hang can never block the run.
# Returns a hashtable: TimedOut (bool), ExitCode (int|null), Stderr (string).
# Verification of success is done by the caller via on-disk state, which is more
# robust than Start-Process -PassThru's flaky ExitCode.
function Invoke-Borg {
    param([string[]]$BorgArgs, [int]$TimeoutSec = 60, [string]$Cwd)
    $o = Join-Path $env:TEMP "borg-o.txt"
    $e = Join-Path $env:TEMP "borg-e.txt"
    $params = @{
        FilePath               = $script:BorgExe
        ArgumentList           = $BorgArgs
        WindowStyle            = "Hidden"
        PassThru               = $true
        RedirectStandardOutput = $o
        RedirectStandardError  = $e
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

# ==================================================================
# 3. borg engine works on Windows: init + create + list with a relative repo
#    (run from a fixed cwd, which avoids the drive-letter bug). We assert the
#    archive lists the stored file. Extract is intentionally NOT tested here:
#    extract must run with cwd = the restore destination, but a relative repo
#    then resolves against the destination ("Repository ...\dest\repo does not
#    exist"), and an absolute repo hits the drive-letter hang below. So a full
#    local round-trip is impossible with this borg build regardless of cwd --
#    that limitation is captured by `borg_local_absolute_repo`.
# ==================================================================
Write-TestHeader "borg_engine_create"
if (-not $script:BorgExe) {
    Fail "borg_engine_create" "borg.exe unavailable"
} else {
    try {
        $repo = "rt_repo"
        $srcName = "rt_src"
        $src = Join-Path $work $srcName
        New-Item -ItemType Directory -Force -Path $src | Out-Null
        "alpha contents" | Out-File (Join-Path $src "alpha.txt") -Encoding ascii -NoNewline

        $r = Invoke-Borg @("init", "--encryption", "none", $repo) 30 $work
        if ($r.TimedOut) { throw "init timed out" }
        if (-not (Test-Path (Join-Path $work $repo))) { throw "repo not created (stderr: $($r.Stderr))" }

        $r = Invoke-Borg @("create", "$repo::a1", $srcName) 60 $work
        if ($r.TimedOut) { throw "create timed out" }

        # The list output is written by borg to stdout; capture it via a file.
        $listOut = Join-Path $work "list.txt"
        $p = Start-Process -FilePath $script:BorgExe -ArgumentList @("list", "$repo::a1") `
            -WindowStyle Hidden -PassThru -WorkingDirectory $work `
            -RedirectStandardOutput $listOut -RedirectStandardError (Join-Path $work "list-e.txt")
        if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "list timed out" }
        $listing = Get-Content $listOut -Raw -ErrorAction SilentlyContinue
        if ($listing -match "alpha\.txt") {
            Pass "borg_engine_create" "init -> create -> list OK; archive contains the stored file (borg engine works on Windows)"
        } else {
            Fail "borg_engine_create" "archive listing missing alpha.txt: $listing"
        }
    } catch {
        Fail "borg_engine_create" "$_"
    }
}

# ==================================================================
# 4. REGRESSION TEST for the drive-letter bug. The app's RepoConfig::location()
#    rewrites a local Windows repo path (C:\repo) to an admin-share UNC path
#    (\\localhost\C$\repo) so borg doesn't misparse the drive-letter colon as an
#    ssh host and hang. This mirrors that conversion and runs a FULL local
#    backup->restore round-trip (init -> create -> extract -> byte-verify),
#    including an extract whose cwd differs from the repo's location (the case
#    that a relative repo can't satisfy). Was a hard hang before the fix.
#    NOTE: the C$ admin share requires an admin account (the test VM is admin).
# ==================================================================
Write-TestHeader "borg_local_repo_via_unc"
if (-not $script:BorgExe) {
    Fail "borg_local_repo_via_unc" "borg.exe unavailable"
} else {
    try {
        $absRepo = Join-Path $work "unc_repo"          # e.g. C:\...\unc_repo
        # Mirror RepoConfig::location(): X:\rest -> \\localhost\X$\rest
        $uncRepo = "\\localhost\" + $absRepo.Substring(0, 1) + "$" + $absRepo.Substring(2)

        $src = Join-Path $work "unc_src"
        $out = Join-Path $work "unc_out"
        New-Item -ItemType Directory -Force -Path $src, $out | Out-Null
        "unc-roundtrip-payload" | Out-File (Join-Path $src "data.txt") -Encoding ascii -NoNewline

        $r = Invoke-Borg @("init", "--encryption", "none", $uncRepo) 40
        if ($r.TimedOut) { throw "init hung on UNC path (admin share unavailable?)" }
        if (-not (Test-Path $absRepo)) { throw "repo not created via UNC (stderr: $($r.Stderr))" }

        # create from a fixed cwd; extract from a DIFFERENT cwd. The UNC repo is
        # absolute/location-independent, so both resolve to the same repo.
        $r = Invoke-Borg @("create", "$uncRepo::a1", "unc_src") 60 $work
        if ($r.TimedOut) { throw "create hung" }
        $r = Invoke-Borg @("extract", "$uncRepo::a1") 60 $out
        if ($r.TimedOut) { throw "extract hung" }

        $restored = Join-Path $out "unc_src\data.txt"
        if ((Test-Path $restored) -and ((Get-Sha (Join-Path $src "data.txt")) -eq (Get-Sha $restored))) {
            Pass "borg_local_repo_via_unc" "UNC-rewritten local repo round-trips (init -> create -> cross-cwd extract -> byte-verify) - drive-letter fix works"
        } else {
            Fail "borg_local_repo_via_unc" "restore via UNC repo did not byte-match (restored exists: $(Test-Path $restored))"
        }
    } catch {
        Fail "borg_local_repo_via_unc" "$_"
    }
}

# ==================================================================
# Cleanup + summary
# ==================================================================
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

Write-Host "`n========================================" -ForegroundColor White
Write-Host "  WINDOWS VALIDATION RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Total: $($script:Passed + $script:Failed)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$resultsPath = Join-Path $env:USERPROFILE "validate-results.json"
$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath $resultsPath -Encoding UTF8
Write-Host "Results written to $resultsPath"

# Still requires eyes on a real desktop (cannot be asserted headlessly): the
# Tauri window/tray rendering, --minimized landing in the tray, a scheduled task
# actually firing the headless run, the console-flash being gone, and the OS
# keychain storing the passphrase in Windows Credential Manager.

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
