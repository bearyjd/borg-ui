# BorgUI VSS feasibility spike (de-risks .claude/PRPs/plans/fix-vss-paths-in-archive.plan.md).
#
# VSS is disabled in the app because borg stores the shadow-copy device path
# (`\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN\...`) verbatim in the archive,
# and the `?`/`:` make it un-restorable on Windows. The plan's Approach B is to
# expose the snapshot as a DOS-namespace NTFS *junction*, run borg with that
# junction as its working directory, and pass volume-relative source paths -- so
# borg stores clean paths like `Users/me/docs/file.txt` that restore cleanly.
#
# The whole approach hinges on ONE unknown that can only be answered on real
# Windows: does `mklink /J` accept an NT-device (`\\?\GLOBALROOT\...`) target?
# Junctions traditionally point at local mount points, not arbitrary NT devices.
# This spike settles that -- and the downstream clean-path + restore behaviour --
# WITHOUT writing any implementation code. If it goes green, implement Approach B.
# If the junction step fails, pivot to Approach C (COM IVssBackupComponents::
# ExposeSnapshot). Either way, no guesswork in the implementation PR.
#
# REQUIRES admin (VSS snapshot creation). The
# smoke VM's default user is admin. Every borg call is hard-timeout-wrapped so a
# hang can never block the run. Cleans up its own snapshot, junction, and files.

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Results = @()

# Cleanup handles -- tracked in script scope so the finally block always releases
# the snapshot and junction even if a test throws midway.
$script:ShadowId = $null
$script:MountDir = $null

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

function Skip($name, $detail) {
    $script:Results += @{ Name = $name; Status = "SKIP"; Detail = $detail }
    Write-Host "  SKIP: $name" -ForegroundColor Yellow
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}

function Write-TestHeader($name) {
    Write-Host "`n--- VSS SPIKE: $name ---" -ForegroundColor Cyan
}

function Get-Sha($path) { (Get-FileHash -Algorithm SHA256 -Path $path).Hash }

# A top-level C: directory (not under %TEMP%) so the volume-relative source path
# borg stores is short and readable: `borgui-vss-spike/src/...`. The seed files
# live here BEFORE the snapshot; repo/out/mount are created after (on live C:).
$root = "C:\borgui-vss-spike"
$srcDir = Join-Path $root "src"
$mount = Join-Path $root "mount"
$outDir = Join-Path $root "out"
$absRepo = Join-Path $root "repo"
# The path of the seed file relative to the C:\ volume root -- what borg stores
# and what we assert is "clean". e.g. borgui-vss-spike\src\locked.txt
$relSeed = "borgui-vss-spike\src\locked.txt"

Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $srcDir, $outDir | Out-Null
$seedContent = "vss-spike-payload-" + (Get-Date).Ticks
$seedFile = Join-Path $srcDir "locked.txt"
$seedContent | Out-File $seedFile -Encoding ascii -NoNewline
$seedSha = Get-Sha $seedFile

# ==================================================================
# 0. Preconditions: admin + borg.exe.
# ==================================================================
Write-TestHeader "preconditions"
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Fail "preconditions" "not running as administrator -- VSS snapshot creation requires elevation. Run the spike as the VM's admin user."
}

$borgDir = "C:\borg"
$script:BorgExe = $null
$existing = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if ($existing) {
    $script:BorgExe = $existing.FullName
} else {
    try {
        $zip = "$env:TEMP\borg-windows.zip"
        $url = "https://github.com/marcpope/borg-windows/releases/download/v1.4.4-win7/borg-windows.zip"
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        New-Item -ItemType Directory -Force -Path $borgDir | Out-Null
        Expand-Archive -Path $zip -DestinationPath $borgDir -Force
        $found = Get-ChildItem $borgDir -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $script:BorgExe = $found.FullName }
    } catch {}
}
if ($isAdmin -and $script:BorgExe) {
    $ver = (& $script:BorgExe --version 2>&1 | Out-String).Trim()
    Pass "preconditions" "admin + borg.exe at $($script:BorgExe) ($ver)"
} elseif ($isAdmin) {
    Fail "preconditions" "borg.exe not found / download failed"
}

# Non-interactive borg environment, mirroring borg.rs::base_command_with.
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

# Run a borg subcommand with a hard timeout so a hang can never block the run.
function Invoke-Borg {
    param([string[]]$BorgArgs, [int]$TimeoutSec = 60, [string]$Cwd)
    $o = Join-Path $env:TEMP "vss-borg-o.txt"
    $e = Join-Path $env:TEMP "vss-borg-e.txt"
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
        return @{ TimedOut = $true; ExitCode = $null; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
    }
    $p.WaitForExit()
    return @{ TimedOut = $false; ExitCode = $p.ExitCode; Stdout = (Get-Content $o -Raw -EA SilentlyContinue); Stderr = (Get-Content $e -Raw -EA SilentlyContinue) }
}

try {
    if ($script:Failed -gt 0 -or -not $script:BorgExe) {
        Skip "vss_snapshot_create" "preconditions not met"
        Skip "vss_junction_mount" "preconditions not met"
        Skip "vss_borg_clean_paths" "preconditions not met"
        Skip "vss_borg_restore" "preconditions not met"
        Skip "vss_locked_file_consistency" "preconditions not met"
    } else {
        # ==========================================================
        # 1. Create a VSS snapshot of C: (mirrors vss.rs::create_snapshot --
        #    Win32_ShadowCopy.Create returns a \\?\GLOBALROOT\... device path).
        # ==========================================================
        Write-TestHeader "vss_snapshot_create"
        $device = $null
        try {
            $class = [wmiclass]"root\cimv2:Win32_ShadowCopy"
            $res = $class.Create("C:\", "ClientAccessible")
            if ($res.ReturnValue -ne 0) { throw "Win32_ShadowCopy.Create returned $($res.ReturnValue) (1=access denied, 2=in use, 5=unsupported)" }
            $shadow = Get-WmiObject Win32_ShadowCopy | Where-Object { $_.ID -eq $res.ShadowID }
            if (-not $shadow) { throw "snapshot $($res.ShadowID) not found after Create" }
            $script:ShadowId = $shadow.ID
            $device = $shadow.DeviceObject
            if ($device -notmatch 'GLOBALROOT') { throw "unexpected device path: $device" }
            Pass "vss_snapshot_create" "snapshot $($shadow.ID) -> $device"
        } catch {
            Fail "vss_snapshot_create" "$_"
        }

        # ==========================================================
        # 2. THE CRITICAL UNKNOWN: mount the shadow as an NTFS junction.
        #    Try the device path both with and without a trailing backslash --
        #    mklink /J wants a directory target and the DeviceObject has no
        #    trailing slash. Whichever yields a traversable junction wins.
        # ==========================================================
        Write-TestHeader "vss_junction_mount"
        $mounted = $false
        if (-not $device) {
            Skip "vss_junction_mount" "no snapshot device to mount"
        } else {
            foreach ($target in @("$device\", "$device")) {
                Remove-Item -Force $mount -ErrorAction SilentlyContinue
                cmd /c rmdir "$mount" 2>$null
                $mk = (cmd /c mklink /J "$mount" "$target" 2>&1 | Out-String).Trim()
                $probe = Join-Path $mount $relSeed
                if ((Test-Path $mount) -and (Test-Path $probe)) {
                    $through = Get-Content $probe -Raw -ErrorAction SilentlyContinue
                    if ($through -eq $seedContent) {
                        $script:MountDir = $mount
                        $mounted = $true
                        Pass "vss_junction_mount" "mklink /J accepts the NT-device target (form: '$target'); seed file readable through junction, content matches"
                        break
                    }
                }
            }
            if (-not $mounted) {
                cmd /c rmdir "$mount" 2>$null
                Fail "vss_junction_mount" "mklink /J did not yield a traversable junction to the shadow device. Last mklink output: '$mk'. -> Approach B (junction) is NOT viable on this build; pivot to Approach C (COM ExposeSnapshot)."
            }
        }

        # ==========================================================
        # 3. THE PAYOFF: borg run with cwd=junction + a volume-relative source
        #    must store a CLEAN path -- no `?`, no `:`, no GLOBALROOT. This is
        #    exactly what the VSS path cleanup prevents (paths otherwise contain
#    `?/GLOBALROOT/Device/HarddiskVolumeShadowCopyN/...`).
        #    The repository uses the raw absolute drive-letter path.
        # ==========================================================
        Write-TestHeader "vss_borg_clean_paths"
        $archiveOk = $false
        if (-not $mounted) {
            Skip "vss_borg_clean_paths" "no junction mounted"
        } else {
            try {
                $r = Invoke-Borg @("init", "--encryption", "none", $absRepo) 40
                if ($r.TimedOut) { throw "init hung on drive-letter repo" }
                if (-not (Test-Path $absRepo)) { throw "repo not created (stderr: $($r.Stderr))" }

                # cwd = junction (shadow root); source = volume-relative path.
                $r = Invoke-Borg @("create", "$absRepo::a1", $relSeed) 60 $mount
                if ($r.TimedOut) { throw "create hung" }

                $listOut = Join-Path $env:TEMP "vss-list.txt"
                $p = Start-Process -FilePath $script:BorgExe -ArgumentList @("list", "$absRepo::a1") `
                    -WindowStyle Hidden -PassThru -RedirectStandardOutput $listOut `
                    -RedirectStandardError (Join-Path $env:TEMP "vss-list-e.txt")
                if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "list timed out" }
                $listing = (Get-Content $listOut -Raw -ErrorAction SilentlyContinue)

                if ($listing -notmatch 'locked\.txt') {
                    throw "archive listing missing the file: $listing"
                }
                # The whole point: the stored path must NOT contain the shadow markers.
                if ($listing -match 'GLOBALROOT' -or $listing -match '\?' -or $listing -match 'HarddiskVolumeShadowCopy') {
                    throw "stored path STILL contains shadow-copy markers (un-restorable): $listing"
                }
                # And it should look like a clean volume-relative path.
                if ($listing -notmatch 'borgui-vss-spike[\\/]src[\\/]locked\.txt') {
                    throw "stored path is not the expected clean volume-relative form: $listing"
                }
                $archiveOk = $true
                Pass "vss_borg_clean_paths" "borg stored a CLEAN path through the junction: $($listing.Trim())"
            } catch {
                Fail "vss_borg_clean_paths" "$_"
            }
        }

        # ==========================================================
        # 4. Restore must actually write files (this is what fails today --
        #    extract aborts on the `?` and 0 files land). Byte-verify.
        # ==========================================================
        Write-TestHeader "vss_borg_restore"
        if (-not $archiveOk) {
            Skip "vss_borg_restore" "no clean archive to restore"
        } else {
            try {
                $r = Invoke-Borg @("extract", "$absRepo::a1") 60 $outDir
                if ($r.TimedOut) { throw "extract hung" }
                $restored = Join-Path $outDir $relSeed
                if (-not (Test-Path $restored)) { throw "restore wrote no file at $restored (stderr: $($r.Stderr))" }
                if ((Get-Sha $restored) -ne $seedSha) { throw "restored file did not byte-match the original" }
                Pass "vss_borg_restore" "extract wrote $relSeed byte-correct -- the archive is restorable (today it is not)"
            } catch {
                Fail "vss_borg_restore" "$_"
            }
        }

        # ==========================================================
        # 5. BONUS -- the actual reason VSS exists: a file under an exclusive
        #    lock on the LIVE volume is still backable THROUGH the snapshot.
        #    Lock the original, then create a second archive from the junction.
        # ==========================================================
        Write-TestHeader "vss_locked_file_consistency"
        if (-not $mounted -or -not $script:BorgExe) {
            Skip "vss_locked_file_consistency" "no junction mounted"
        } else {
            $fs = $null
            try {
                # Exclusively lock the live original (FileShare::None) -- a live
                # backup could not open this; the snapshot copy is unaffected.
                $fs = [System.IO.File]::Open($seedFile, [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)

                $r = Invoke-Borg @("create", "$absRepo::locked", $relSeed) 60 $mount
                if ($r.TimedOut) { throw "create-through-snapshot hung while original was locked" }

                $listOut = Join-Path $env:TEMP "vss-list2.txt"
                $p = Start-Process -FilePath $script:BorgExe -ArgumentList @("list", "$absRepo::locked") `
                    -WindowStyle Hidden -PassThru -RedirectStandardOutput $listOut `
                    -RedirectStandardError (Join-Path $env:TEMP "vss-list2-e.txt")
                if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "list timed out" }
                $listing = (Get-Content $listOut -Raw -ErrorAction SilentlyContinue)
                if ($listing -match 'locked\.txt') {
                    Pass "vss_locked_file_consistency" "backed up an exclusively-locked file via the snapshot -- VSS's whole purpose works"
                } else {
                    throw "archive missing the locked file: $listing"
                }
            } catch {
                Fail "vss_locked_file_consistency" "$_"
            } finally {
                if ($fs) { $fs.Close(); $fs.Dispose() }
            }
        }
    }
}
finally {
    # ==============================================================
    # Cleanup -- ALWAYS release the junction then the snapshot, never leak.
    # rmdir (not Remove-Item -Recurse) removes only the junction link, never
    # the shadow contents behind it.
    # ==============================================================
    if ($script:MountDir) { cmd /c rmdir "$script:MountDir" 2>$null }
    if ($script:ShadowId) {
        try {
            $s = Get-WmiObject Win32_ShadowCopy | Where-Object { $_.ID -eq $script:ShadowId }
            if ($s) { $s.Delete() }
        } catch {}
    }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}

# ==================================================================
# Summary
# ==================================================================
Write-Host "`n========================================" -ForegroundColor White
Write-Host "  VSS FEASIBILITY SPIKE RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Total: $($script:Passed + $script:Failed)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$verdict = if ($script:Failed -eq 0 -and $script:Passed -ge 5) {
    "VERDICT: Approach B (NTFS junction) is VIABLE -- implement it."
} elseif ($script:Results | Where-Object { $_.Name -eq "vss_junction_mount" -and $_.Status -eq "FAIL" }) {
    "VERDICT: junction mount FAILED -- pivot to Approach C (COM IVssBackupComponents::ExposeSnapshot)."
} else {
    "VERDICT: inconclusive -- inspect failures above."
}
Write-Host $verdict -ForegroundColor Magenta

$resultsPath = Join-Path $env:USERPROFILE "validate-vss-spike-results.json"
$script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath $resultsPath -Encoding UTF8
Write-Host "Results written to $resultsPath"

if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
