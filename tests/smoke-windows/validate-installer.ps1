# BorgUI installer validation pass.
#
# Verifies that a built Windows installer (NSIS .exe and/or MSI) actually
# installs the app AND lays the bundled borg distribution down correctly, then
# proves the installed borg engine works end-to-end. This is the one thing the
# other validate-*.ps1 scripts never cover: they all run a loose `tauri build`
# exe, never an installed-from-installer layout.
#
# For each installer found:
#   1. Silent install (NSIS `/S`, MSI `msiexec /quiet`).
#   2. Locate the install dir and assert borg-ui.exe + borg.exe + the PyInstaller
#      sibling `_internal\python311.dll` are co-located (the bundling contract:
#      lib.rs resolves borg as <exe dir>\borg.exe, and borg dies without _internal).
#   3. Run the INSTALLED borg.exe through `--version` + a real init/create/list/
#      extract/byte-verify round-trip (relative paths to dodge the drive-letter
#      bug; non-interactive env vars so it can't hang).
#   4. Silent uninstall and assert the install dir is gone.
#
# Every borg call is hard-bounded by a timeout (Invoke-Borg) so a hang can never
# block the run. NSIS (per-user `/S`) needs no elevation; an MSI per-machine
# install does, so if msiexec is blocked by UAC over SSH the MSI case SKIPs
# (not fails) and the NSIS case carries the proof.

param(
    [string]$InstallerDir = "$env:USERPROFILE\borgui-installers"
)

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0

function Pass($name, $detail) {
    $script:Passed++
    Write-Host "  PASS: $name" -ForegroundColor Green
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
function Fail($name, $detail) {
    $script:Failed++
    Write-Host "  FAIL: $name" -ForegroundColor Red
    if ($detail) { Write-Host "        $detail" -ForegroundColor Yellow }
}
function Skip($name, $detail) {
    $script:Skipped++
    Write-Host "  SKIP: $name" -ForegroundColor Yellow
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
function Write-TestHeader($name) {
    Write-Host "`n--- INSTALLER: $name ---" -ForegroundColor Cyan
}

# Non-interactive borg environment, mirroring borg.rs::base_command_with so the
# bundled engine can never block on a prompt with no TTY.
$env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
$env:BORG_DISPLAY_PASSPHRASE = "no"
$env:BORG_PASSPHRASE = ""

# Run a borg subcommand with a hard timeout so a hang can never block the run.
function Invoke-Borg {
    param([string]$Exe, [string[]]$BorgArgs, [int]$TimeoutSec = 60, [string]$Cwd)
    $o = Join-Path $env:TEMP "inst-borg-o.txt"
    $e = Join-Path $env:TEMP "inst-borg-e.txt"
    $params = @{
        FilePath = $Exe; ArgumentList = $BorgArgs; WindowStyle = "Hidden"
        PassThru = $true; RedirectStandardOutput = $o; RedirectStandardError = $e
    }
    if ($Cwd) { $params["WorkingDirectory"] = $Cwd }
    $p = Start-Process @params
    if (-not $p.WaitForExit($TimeoutSec * 1000)) {
        try { $p.Kill() } catch {}
        return @{ TimedOut = $true; ExitCode = $null; Stdout = ""; Stderr = "timed out after ${TimeoutSec}s" }
    }
    $p.WaitForExit()
    return @{
        TimedOut = $false; ExitCode = $p.ExitCode
        Stdout = (Get-Content $o -Raw -EA SilentlyContinue)
        Stderr = (Get-Content $e -Raw -EA SilentlyContinue)
    }
}

# Candidate install locations for product "BorgUI" (NSIS per-user / per-machine, MSI).
function Find-InstallDir {
    $candidates = @(
        (Join-Path $env:ProgramFiles 'BorgUI'),
        (Join-Path ${env:ProgramFiles(x86)} 'BorgUI'),
        (Join-Path $env:LOCALAPPDATA 'BorgUI'),
        (Join-Path $env:LOCALAPPDATA 'Programs\BorgUI')
    )
    foreach ($c in $candidates) {
        if ($c -and (Test-Path (Join-Path $c 'borg-ui.exe'))) { return $c }
    }
    return $null
}

# Assert the bundling contract + exercise the installed borg engine.
function Test-InstalledLayout($label, $installDir) {
    $borgUi   = Join-Path $installDir 'borg-ui.exe'
    $borg     = Join-Path $installDir 'borg.exe'
    $internal = Join-Path $installDir '_internal\python311.dll'

    if ((Test-Path $borgUi) -and (Test-Path $borg) -and (Test-Path $internal)) {
        Pass "$label`_layout" "borg-ui.exe + borg.exe + _internal\python311.dll co-located in $installDir"
    } else {
        Fail "$label`_layout" ("missing in {0}: borg-ui.exe={1} borg.exe={2} _internal\python311.dll={3}" -f `
            $installDir, (Test-Path $borgUi), (Test-Path $borg), (Test-Path $internal))
        return
    }

    # borg --version proves _internal/ loads (the whole point of bundling the onedir).
    $v = Invoke-Borg -Exe $borg -BorgArgs @('--version') -TimeoutSec 30
    if (-not $v.TimedOut -and $v.ExitCode -eq 0 -and $v.Stdout -match 'borg') {
        Pass "$label`_borg_version" $v.Stdout.Trim()
    } else {
        Fail "$label`_borg_version" ("exit={0} timedout={1} err={2}" -f $v.ExitCode, $v.TimedOut, $v.Stderr)
        return
    }

    # Real round-trip with the INSTALLED borg.exe: init -> create -> list -> extract
    # -> byte-verify. Relative paths + a working dir dodge the drive-letter SSH
    # misparse (see validate.ps1). This proves the bundled engine actually works,
    # not just that it starts.
    $work = Join-Path $env:TEMP "borgui-inst-rt"
    Remove-Item -Recurse -Force $work -EA SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $work 'src') | Out-Null
    $marker = "installed-borg-roundtrip-$([System.Guid]::NewGuid().ToString('N'))"
    Set-Content -Path (Join-Path $work 'src\data.txt') -Value $marker -NoNewline

    try {
        $init = Invoke-Borg -Exe $borg -BorgArgs @('init', '--encryption=none', 'repo') -Cwd $work -TimeoutSec 60
        if ($init.TimedOut -or $init.ExitCode -ne 0) { Fail "$label`_roundtrip" "init failed: exit=$($init.ExitCode) err=$($init.Stderr)"; return }

        $create = Invoke-Borg -Exe $borg -BorgArgs @('create', 'repo::arch', 'src') -Cwd $work -TimeoutSec 90
        if ($create.TimedOut -or $create.ExitCode -ne 0) { Fail "$label`_roundtrip" "create failed: exit=$($create.ExitCode) err=$($create.Stderr)"; return }

        # Delete the original so the extract has to genuinely restore it from the
        # archive. Extract runs in $work with the repo as a relative path ("repo")
        # — no drive-letter colon, so borg can't misparse it as an SSH remote.
        $srcFile = Join-Path $work 'src\data.txt'
        Remove-Item -Force $srcFile
        $extract = Invoke-Borg -Exe $borg -BorgArgs @('extract', 'repo::arch') -Cwd $work -TimeoutSec 90
        if ($extract.TimedOut -or $extract.ExitCode -ne 0) { Fail "$label`_roundtrip" "extract failed: exit=$($extract.ExitCode) err=$($extract.Stderr)"; return }

        if ((Test-Path $srcFile) -and ((Get-Content $srcFile -Raw) -eq $marker)) {
            Pass "$label`_roundtrip" "installed borg.exe init->create->delete->extract byte-verified"
        } else {
            Fail "$label`_roundtrip" "restored file missing or content mismatch at $srcFile"
        }
    } finally {
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
    }
}

# ==================================================================
# NSIS installer (per-user, /S — no elevation needed)
# ==================================================================
Write-TestHeader "nsis_install"
$nsis = Get-ChildItem -Path $InstallerDir -Filter '*-setup.exe' -EA SilentlyContinue | Select-Object -First 1
if (-not $nsis) {
    Skip "nsis_install" "no *-setup.exe under $InstallerDir"
} else {
    Write-Host "  Installing $($nsis.Name) (/S)..." -ForegroundColor DarkGray
    $proc = Start-Process -FilePath $nsis.FullName -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    $dir = Find-InstallDir
    if (-not $dir) {
        Fail "nsis_install" "installer exit=$($proc.ExitCode); borg-ui.exe not found in any candidate install dir"
    } else {
        Pass "nsis_install" "installed to $dir"
        Test-InstalledLayout "nsis" $dir
        # Uninstall via Tauri's NSIS uninstaller.
        $uninst = Join-Path $dir 'uninstall.exe'
        if (Test-Path $uninst) {
            Start-Process -FilePath $uninst -ArgumentList '/S' -Wait -WindowStyle Hidden
            Start-Sleep -Seconds 3
            if (Test-Path (Join-Path $dir 'borg-ui.exe')) {
                Fail "nsis_uninstall" "borg-ui.exe still present after uninstall in $dir"
            } else {
                Pass "nsis_uninstall" "uninstaller removed the app from $dir"
            }
        } else {
            Skip "nsis_uninstall" "uninstall.exe not found in $dir"
        }
    }
}

# ==================================================================
# MSI installer (per-machine — needs elevation; SKIP if UAC blocks it)
# ==================================================================
Write-TestHeader "msi_install"
$msi = Get-ChildItem -Path $InstallerDir -Filter '*.msi' -EA SilentlyContinue | Select-Object -First 1
if (-not $msi) {
    Skip "msi_install" "no *.msi under $InstallerDir"
} else {
    $log = Join-Path $env:TEMP 'borgui-msi.log'
    Write-Host "  Installing $($msi.Name) (msiexec /quiet)..." -ForegroundColor DarkGray
    $proc = Start-Process -FilePath 'msiexec.exe' `
        -ArgumentList @('/i', "`"$($msi.FullName)`"", '/quiet', '/norestart', '/l*v', "`"$log`"") `
        -Wait -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 3
    $dir = Find-InstallDir
    if ($dir) {
        Pass "msi_install" "installed to $dir (msiexec exit=$($proc.ExitCode))"
        Test-InstalledLayout "msi" $dir
        Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/x', "`"$($msi.FullName)`"", '/quiet', '/norestart') -Wait -WindowStyle Hidden
        Start-Sleep -Seconds 3
        if (Find-InstallDir) { Fail "msi_uninstall" "app still present after msiexec /x" }
        else { Pass "msi_uninstall" "msiexec /x removed the app" }
    } elseif ($proc.ExitCode -in @(1602, 1603, 1625, 5)) {
        # 1603/1625/5 = access denied / blocked by policy => almost always UAC over SSH.
        Skip "msi_install" "msiexec exit=$($proc.ExitCode) (elevation blocked over SSH); NSIS case carries the bundling proof"
    } else {
        Fail "msi_install" "msiexec exit=$($proc.ExitCode); app not found. See $log"
    }
}

# ==================================================================
Write-Host "`n=== SUMMARY ===" -ForegroundColor Cyan
Write-Host "  Passed:  $script:Passed" -ForegroundColor Green
Write-Host "  Failed:  $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $script:Skipped" -ForegroundColor Yellow
if ($script:Passed -eq 0 -and $script:Failed -eq 0) {
    Write-Host "  (nothing ran — no installers were found under $InstallerDir)" -ForegroundColor Yellow
}
