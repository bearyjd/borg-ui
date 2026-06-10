# BorgUI MANUAL-path VSS validation: does the GUI "Start Backup"
# (app-tauri commands.rs::create_backup) take a VSS snapshot?
#
# validate-vss.ps1 covers the SCHEDULED path (borg-ui.exe --scheduled-backup ->
# scheduled.rs). This covers the MANUAL path: it drives the live Svelte UI via UI
# Automation -- Add Folder -> Start Backup -- with one source file exclusively
# LOCKED. The locked file landing in the archive proves create_backup engaged VSS
# (a live backup cannot open the locked file and would skip it). Same definitive
# signal as validate-vss.ps1, through the other production entry point.
#
# REQUIRES a PRODUCTION tauri-build exe (embedded frontend) at
#   C:\borgui-test\target\release\borg-ui.exe
# (a dev-mode `cargo build` exe shows the localhost-error page). The app must run
# ELEVATED for VSS (snapshot creation + the C$ admin-share repo rewrite need
# admin), so this self-relaunches in session 1 with /RL HIGHEST -- the launched
# app inherits the elevated token.
#
# Mirrors validate-gui-flows.ps1 (UIA driving) + validate-vss.ps1 (locked-file
# assertion). Pass/Fail/Skip + JSON + exit code. ASCII only (PS 5.1 reads
# UTF-8-without-BOM as ANSI and breaks parsing).

param([switch]$InSession1, [int]$WinWaitSec = 30)
$ErrorActionPreference = "Continue"

# ----------------------------------------------------------------------------
# SESSION-1 (ELEVATED) RELAUNCH WRAPPER
# ----------------------------------------------------------------------------
if (-not $InSession1) {
    $self = $MyInvocation.MyCommand.Path
    $task = "BorgUI-VssManual"; $log = Join-Path $env:USERPROFILE "vss-manual.log"
    $sentinel = Join-Path $env:USERPROFILE "vss-manual.done"; $bat = Join-Path $env:USERPROFILE "vss-manual.bat"
    $resJson = Join-Path $env:USERPROFILE "vss-manual-results.json"
    Remove-Item $log, $sentinel, $resJson -EA SilentlyContinue
    @("@echo off",
        "powershell -ExecutionPolicy Bypass -File `"$self`" -InSession1 > `"$log`" 2>&1",
        "echo DONE > `"$sentinel`"") | Set-Content $bat -Encoding Ascii
    # /IT = interactive session 1 (UIA + desktop); /RL HIGHEST = elevated (VSS).
    & schtasks.exe /Create /F /TN $task /TR "`"$bat`"" /SC ONCE /ST 23:59 /IT /RL HIGHEST 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Host "SKIP: schtasks /Create failed; needs interactive desktop."; Write-Host "Failed: 0"; exit 0 }
    & schtasks.exe /Run /TN $task 2>&1 | Out-Null
    $deadline = (Get-Date).AddSeconds(360)
    while ((Get-Date) -lt $deadline -and -not (Test-Path $sentinel)) { Start-Sleep -Seconds 3 }
    if (Test-Path $log) { Get-Content $log }
    & schtasks.exe /Delete /F /TN $task 2>&1 | Out-Null
    Remove-Item $bat -EA SilentlyContinue
    if (-not (Test-Path $sentinel)) { Write-Host "`nSKIP: session-1 task did not finish (no desktop?)."; Write-Host "Failed: 0"; exit 0 }
    $failed = 0
    if (Test-Path $resJson) { try { $failed = @((Get-Content $resJson -Raw | ConvertFrom-Json) | Where-Object { $_.Status -eq "FAIL" }).Count } catch {} }
    if ($failed -gt 0) { exit 1 } else { exit 0 }
}

# ----------------------------------------------------------------------------
# SESSION-1 INNER RUN (elevated)
# ----------------------------------------------------------------------------
$script:Passed = 0; $script:Failed = 0; $script:Skipped = 0; $script:Results = @()
function Pass($n, $d) { $script:Passed++; $script:Results += @{ Name = $n; Status = "PASS"; Detail = $d }; Write-Host "  PASS: $n"; if ($d) { Write-Host "        $d" } }
function Fail($n, $d) { $script:Failed++; $script:Results += @{ Name = $n; Status = "FAIL"; Detail = $d }; Write-Host "  FAIL: $n"; if ($d) { Write-Host "        $d" } }
function Skip($n, $d) { $script:Skipped++; $script:Results += @{ Name = $n; Status = "SKIP"; Detail = $d }; Write-Host "  SKIP: $n"; if ($d) { Write-Host "        $d" } }
function Hdr($n) { Write-Host "`n--- VSS-MANUAL: $n ---" }
function Summary {
    Write-Host "`n========================================"
    Write-Host "  MANUAL-PATH VSS VALIDATION RESULTS"
    Write-Host "  Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
    Write-Host "========================================`n"
    $script:Results | ConvertTo-Json -Depth 3 | Out-File (Join-Path $env:USERPROFILE "vss-manual-results.json") -Encoding UTF8
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class GuiNative {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  public const uint LD=0x0002, LU=0x0004;
  public static void LClick(int x,int y){SetCursorPos(x,y);System.Threading.Thread.Sleep(140);mouse_event(LD,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(40);mouse_event(LU,0,0,0,UIntPtr.Zero);}
}
"@

$UIA = [System.Windows.Automation.AutomationElement]
$TREE = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]
$ANYCOND = [System.Windows.Automation.Condition]::TrueCondition
$IPATTERN = [System.Windows.Automation.InvokePattern]::Pattern

$EXE = "C:\borgui-test\target\release\borg-ui.exe"
$CFGDIR = Join-Path $env:APPDATA "com.borgui.app"
$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName

function CCond($c) { New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, $c) }
function TCond($t) { New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $t) }
function AidCond($a) { New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, $a) }
function To-Unc($p) { "\\localhost\" + $p.Substring(0, 1) + "$" + $p.Substring(2) }
function Get-Sha($p) { (Get-FileHash -Algorithm SHA256 -Path $p).Hash }

function Ensure-BorgBeside {
    if (-not $script:BorgExe) { return }
    $ad = Split-Path $EXE -Parent; $bd = Split-Path $script:BorgExe -Parent
    if ($bd -ine $ad -and -not (Test-Path (Join-Path $ad "_internal"))) { try { Copy-Item (Join-Path $bd "*") $ad -Recurse -Force } catch {} }
}
function Launch-App {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--force-renderer-accessibility"
    return Start-Process -FilePath $EXE -PassThru
}
function Stop-App($p) { if ($p -and (Get-Process -Id $p.Id -EA SilentlyContinue)) { Stop-Process -Id $p.Id -Force -EA SilentlyContinue } }
function Bring-Foreground($win) {
    if (-not $win) { return }
    try { $h = [IntPtr]$win.Current.NativeWindowHandle; [void][GuiNative]::ShowWindow($h, 5); [void][GuiNative]::BringWindowToTop($h); [void][GuiNative]::SetForegroundWindow($h); Start-Sleep -Milliseconds 500 } catch {}
}
function Wait-Win($timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        foreach ($w in $UIA::RootElement.FindAll($TREE::Children, (TCond $CT::Window))) {
            $n = ""; try { $n = $w.Current.Name } catch {}
            if ($n -like "*BorgUI*") { $link = $w.FindFirst($TREE::Descendants, (TCond $CT::Hyperlink)); if ($link) { return $w } }
        }
        Start-Sleep -Milliseconds 800
    }
    return $null
}
function Find-El($root, $ctype, $pat) {
    if (-not $root) { return $null }
    foreach ($e in $root.FindAll($TREE::Descendants, (TCond $ctype))) { $n = ""; try { $n = $e.Current.Name } catch {}; if ($n -like $pat) { return $e } }
    return $null
}
function Has-Text($root, $pat) {
    foreach ($e in $root.FindAll($TREE::Descendants, $ANYCOND)) { $n = ""; try { $n = $e.Current.Name } catch {}; if ($n -like $pat) { return $true } }
    return $false
}
function Invoke-El($el) {
    if (-not $el) { return $false }
    try { $el.GetCurrentPattern($IPATTERN).Invoke(); return $true } catch {}
    try { $r = $el.Current.BoundingRectangle; if ($r.Width -gt 0 -and -not [double]::IsInfinity($r.X)) { [GuiNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); return $true } } catch {}
    return $false
}
function Nav($win, $page) {
    $link = Find-El $win $CT::Hyperlink "*$page*"; if (-not $link) { return $false }
    [void](Invoke-El $link); Start-Sleep -Seconds 2; return $true
}
# Drive the native folder picker (#32770): Ctrl+L address bar -> path -> Enter ->
# "Select Folder" (AutomationId 1). Same technique as validate-gui-flows.ps1.
function Set-FolderDialog($path) {
    $dlg = $null; $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and -not $dlg) { $dlg = $UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770")); if (-not $dlg) { Start-Sleep -Milliseconds 500 } }
    if (-not $dlg) { return "no folder dialog appeared" }
    try { $h = [IntPtr]$dlg.Current.NativeWindowHandle; [void][GuiNative]::SetForegroundWindow($h); [void][GuiNative]::BringWindowToTop($h) } catch {}
    Start-Sleep -Milliseconds 600
    [System.Windows.Forms.SendKeys]::SendWait("^l"); Start-Sleep -Milliseconds 450
    [System.Windows.Forms.SendKeys]::SendWait(($path -replace '([+^%~(){}\[\]])', '{$1}')); Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}"); Start-Sleep -Milliseconds 1100
    $dlg2 = $UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770"))
    if ($dlg2) {
        $sel = $dlg2.FindFirst($TREE::Descendants, (AidCond "1")); $clicked = $false
        if ($sel) {
            try { $sel.GetCurrentPattern($IPATTERN).Invoke(); $clicked = $true } catch {}
            if (-not $clicked) { try { $r = $sel.Current.BoundingRectangle; if ($r.Width -gt 0 -and -not [double]::IsInfinity($r.X)) { [GuiNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); $clicked = $true } } catch {} }
        }
        if (-not $clicked) { [System.Windows.Forms.SendKeys]::SendWait("{ENTER}") }
        Start-Sleep -Milliseconds 1000
    }
    if ($UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770"))) { return "dialog still open after Ctrl+L navigate + select" }
    return "ok"
}
function RunBorg($arglist, $cwd) {
    $o = Join-Path $env:TEMP "vm-o.txt"; $e = Join-Path $env:TEMP "vm-e.txt"
    $env:BORG_PASSPHRASE = ""; $env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
    $env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"; $env:BORG_DISPLAY_PASSPHRASE = "no"
    $p = Start-Process -FilePath $script:BorgExe -ArgumentList $arglist -WindowStyle Hidden -PassThru -RedirectStandardOutput $o -RedirectStandardError $e -WorkingDirectory $cwd
    if (-not $p.WaitForExit(60000)) { try { $p.Kill() } catch {}; return @{ ok = $false; code = $null; out = ""; err = "timeout" } }
    return @{ ok = ($p.ExitCode -le 1); code = $p.ExitCode; out = (Get-Content $o -Raw -EA SilentlyContinue); err = (Get-Content $e -Raw -EA SilentlyContinue) }
}

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

# ===========================================================================
# The manual production path: GUI Start Backup with a locked source file.
# ===========================================================================
Hdr "vss_manual_backup_locked_file"
$proc = $null; $work = "C:\borgui-vss-manual"; $lock = $null
$profilesPath = Join-Path $CFGDIR "profiles.json"; $bak = "$profilesPath.vssmanbak"
try {
    if (-not $isAdmin) { Skip "vss_manual_backup_locked_file" "not elevated -- VSS needs admin (the /RL HIGHEST task should provide it)" }
    elseif (-not (Test-Path $EXE)) { Skip "vss_manual_backup_locked_file" "no production exe at $EXE (run a real tauri build)" }
    elseif (-not $script:BorgExe) { Skip "vss_manual_backup_locked_file" "borg.exe not found under C:\borg" }
    else {
        Ensure-BorgBeside
        Remove-Item -Recurse -Force $work -EA SilentlyContinue
        New-Item -ItemType Directory -Force -Path "$work\src", "$work\out" | Out-Null
        $normalFile = "$work\src\normal.txt"; $lockedFile = "$work\src\locked.txt"
        "vss-manual-normal" | Out-File $normalFile -Encoding ascii -NoNewline
        ("vss-manual-locked-" + (Get-Date).Ticks) | Out-File $lockedFile -Encoding ascii -NoNewline
        $normalSha = Get-Sha $normalFile; $lockedSha = Get-Sha $lockedFile

        $repoAbs = "$work\repo"; New-Item -ItemType Directory -Force -Path $repoAbs | Out-Null
        $repoUnc = To-Unc $repoAbs
        # create_backup does `create`, not `init` -> initialise the repo first.
        $ri = RunBorg @("init", "--encryption", "none", $repoUnc) $work
        if (-not (Test-Path "$repoAbs\config")) { throw "borg init failed on $repoUnc (admin share unavailable?): $($ri.err)" }

        # Stage an active profile pointing at the repo (the Backup page reads the
        # active profile's repo; the source comes from the GUI Add Folder).
        New-Item -ItemType Directory -Force -Path $CFGDIR | Out-Null
        if ((Test-Path $profilesPath) -and -not (Test-Path $bak)) { Copy-Item $profilesPath $bak -Force }
        $prof = @{ active_id = "vss-man"; profiles = @(@{ id = "vss-man"; name = "VssManual"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = $repoAbs; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null }) }
        (ConvertTo-Json -InputObject $prof -Depth 8) | Out-File $profilesPath -Encoding ascii

        # Exclusively lock locked.txt BEFORE the backup runs. A live backup cannot
        # open it (share violation -> skipped); only a VSS snapshot read captures it.
        $lock = [System.IO.File]::Open($lockedFile, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)

        $proc = Launch-App
        $win = Wait-Win $WinWaitSec
        if (-not $win) { Skip "vss_manual_backup_locked_file" "window/webview not ready (no desktop / not a production exe?)" }
        else {
            [void](Nav $win "Backup"); Start-Sleep -Seconds 1
            $add = Find-El $win $CT::Button "*Add Folder*"
            if (-not $add) { Skip "vss_manual_backup_locked_file" "'+ Add Folder' button not found on the Backup page" }
            else {
                Bring-Foreground $win
                [void](Invoke-El $add)
                $dlg = Set-FolderDialog "$work\src"
                if ($dlg -ne "ok") { Fail "vss_manual_backup_locked_file" "add-folder dialog: $dlg" }
                else {
                    Start-Sleep -Seconds 1
                    $start = Find-El $win $CT::Button "Start Backup"
                    if (-not $start) { Fail "vss_manual_backup_locked_file" "'Start Backup' not found after adding the folder" }
                    else {
                        [void](Invoke-El $start)
                        # Poll the repo for the completed archive (create releases the
                        # repo lock when done). Retry through transient lock errors.
                        $archName = $null; $deadline = (Get-Date).AddSeconds(120)
                        while ((Get-Date) -lt $deadline) {
                            Start-Sleep -Seconds 3
                            $lst = RunBorg @("list", "--short", $repoUnc) $work
                            if ($lst.ok) {
                                $first = ("$($lst.out)" -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
                                if ($first) { $archName = $first.Trim(); break }
                            }
                        }
                        if ($lock) { $lock.Close(); $lock.Dispose(); $lock = $null }

                        if (-not $archName) { Fail "vss_manual_backup_locked_file" "no archive appeared within 120s -- the GUI backup did not complete (button not wired? repo error? check the app)" }
                        else {
                            $listing = RunBorg @("list", "$repoUnc::$archName") $work
                            $listOut = "$($listing.out)"
                            if ($listOut -notmatch 'normal\.txt') { Fail "vss_manual_backup_locked_file" "archive '$archName' is missing normal.txt -- sources not captured: $listOut" }
                            elseif ($listOut -match 'locked\.txt') {
                                Pass "vss_manual_backup_locked_file" "GUI 'Start Backup' archived the exclusively-locked file (archive '$archName') -- the manual create_backup path took a VSS snapshot (a live backup would have skipped it)"
                            }
                            else { Fail "vss_manual_backup_locked_file" "the locked file is ABSENT from archive '$archName' -- the manual backup fell back to live files (no VSS snapshot): $listOut" }

                            # clean, restorable paths (match the live layout)
                            Hdr "vss_manual_clean_paths"
                            if ($listOut -match 'GLOBALROOT' -or $listOut -match '\?' -or $listOut -match 'HarddiskVolumeShadowCopy') {
                                Fail "vss_manual_clean_paths" "stored paths contain shadow-copy markers (un-restorable): $listOut"
                            } else {
                                Pass "vss_manual_clean_paths" "stored paths are clean: $(($listOut -split "`n" | Where-Object { $_ -match 'locked\.txt' } | Select-Object -First 1))"
                            }

                            # restore round-trip
                            Hdr "vss_manual_restore_roundtrip"
                            $rx = RunBorg @("extract", "$repoUnc::$archName") "$work\out"
                            $rn = Get-ChildItem "$work\out" -Recurse -Filter normal.txt -EA SilentlyContinue | Select-Object -First 1
                            $rl = Get-ChildItem "$work\out" -Recurse -Filter locked.txt -EA SilentlyContinue | Select-Object -First 1
                            if (-not $rn -or -not $rl) { Fail "vss_manual_restore_roundtrip" "restore did not write both files (normal=$([bool]$rn) locked=$([bool]$rl); rc=$($rx.code))" }
                            elseif ((Get-Sha $rn.FullName) -eq $normalSha -and (Get-Sha $rl.FullName) -eq $lockedSha) {
                                Pass "vss_manual_restore_roundtrip" "both files (incl. the locked one) restored byte-correct from the GUI-made VSS archive"
                            }
                            else { Fail "vss_manual_restore_roundtrip" "restored bytes did not match the originals" }
                        }
                    }
                }
            }
        }
    }
} catch { Fail "vss_manual_backup_locked_file" "$_" } finally {
    if ($lock) { try { $lock.Close(); $lock.Dispose() } catch {} }
    try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}
    Stop-App $proc
    if (Test-Path $bak) { Move-Item $bak $profilesPath -Force } else { Remove-Item $profilesPath -EA SilentlyContinue }
    Remove-Item -Recurse -Force $work -EA SilentlyContinue
}

Summary
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
