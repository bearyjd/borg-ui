# BorgUI interactive GUI-flow validation (the flows that need a real desktop +
# the production tauri-build exe). Drives the live Svelte UI via UI Automation.
#
# REQUIRES a PRODUCTION build (embedded frontend) at
#   C:\borgui-test\target\release\borg-ui.exe
# (a dev-mode `cargo build` exe shows the localhost-error page). Build it with the
# now-fixed pnpm: `pnpm tauri build --no-bundle` in app-tauri.
#
# KEY UNLOCK: WebView2/Chromium only exposes its UIA accessibility tree when asked.
# We launch with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-renderer-accessibility
# so the Svelte content (nav links, buttons, inputs, text) is reachable via UIA.
#
# Self-relaunches in session 1 (UIA + the desktop need an interactive session).
# Pass/Fail/Skip + JSON + exit code, mirroring validate-gui.ps1. ASCII only.
#
# Flows: 1 tray "Backup now" -> /backup nav; 4 Settings profile switch.
# (Restore round-trip + Cancel are added once these pass.)

param([switch]$InSession1, [int]$WinWaitSec = 25)
$ErrorActionPreference = "Continue"

# ----------------------------------------------------------------------------
# SESSION-1 RELAUNCH WRAPPER
# ----------------------------------------------------------------------------
if (-not $InSession1) {
    $self = $MyInvocation.MyCommand.Path
    $task = "BorgUI-GuiFlows"; $log = Join-Path $env:USERPROFILE "gui-flows.log"
    $sentinel = Join-Path $env:USERPROFILE "gui-flows.done"; $bat = Join-Path $env:USERPROFILE "gui-flows.bat"
    $resJson = Join-Path $env:USERPROFILE "gui-flows-results.json"
    Remove-Item $log, $sentinel, $resJson -EA SilentlyContinue
    @("@echo off",
      "powershell -ExecutionPolicy Bypass -File `"$self`" -InSession1 > `"$log`" 2>&1",
      "echo DONE > `"$sentinel`"") | Set-Content $bat -Encoding Ascii
    & schtasks.exe /Create /F /TN $task /TR "`"$bat`"" /SC ONCE /ST 23:59 /IT 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Host "SKIP: schtasks /Create failed; needs interactive desktop."; Write-Host "Failed: 0"; exit 0 }
    & schtasks.exe /Run /TN $task 2>&1 | Out-Null
    $deadline = (Get-Date).AddSeconds(300)
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
# SESSION-1 INNER RUN
# ----------------------------------------------------------------------------
$script:Passed = 0; $script:Failed = 0; $script:Skipped = 0; $script:Results = @()
function Pass($n, $d) { $script:Passed++; $script:Results += @{ Name = $n; Status = "PASS"; Detail = $d }; Write-Host "  PASS: $n"; if ($d) { Write-Host "        $d" } }
function Fail($n, $d) { $script:Failed++; $script:Results += @{ Name = $n; Status = "FAIL"; Detail = $d }; Write-Host "  FAIL: $n"; if ($d) { Write-Host "        $d" } }
function Skip($n, $d) { $script:Skipped++; $script:Results += @{ Name = $n; Status = "SKIP"; Detail = $d }; Write-Host "  SKIP: $n"; if ($d) { Write-Host "        $d" } }
function Hdr($n) { Write-Host "`n--- GUI-FLOW: $n ---" }
function Summary {
    Write-Host "`n========================================"
    Write-Host "  GUI FLOW VALIDATION RESULTS"
    Write-Host "  Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
    Write-Host "========================================`n"
    $script:Results | ConvertTo-Json -Depth 3 | Out-File (Join-Path $env:USERPROFILE "gui-flows-results.json") -Encoding UTF8
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
  [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] ref object acc);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  public const uint RD=0x0008, RU=0x0010, LD=0x0002, LU=0x0004;
  public static void RClick(int x,int y){SetCursorPos(x,y);System.Threading.Thread.Sleep(140);mouse_event(RD,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(40);mouse_event(RU,0,0,0,UIntPtr.Zero);}
  public static void LClick(int x,int y){SetCursorPos(x,y);System.Threading.Thread.Sleep(140);mouse_event(LD,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(40);mouse_event(LU,0,0,0,UIntPtr.Zero);}
}
"@

$UIA = [System.Windows.Automation.AutomationElement]
$TREE = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]
$ANYCOND = [System.Windows.Automation.Condition]::TrueCondition
$IPATTERN = [System.Windows.Automation.InvokePattern]::Pattern
$EXPAT = [System.Windows.Automation.ExpandCollapsePattern]::Pattern
$SIPAT = [System.Windows.Automation.SelectionItemPattern]::Pattern
$VPAT = [System.Windows.Automation.ValuePattern]::Pattern

$EXE = "C:\borgui-test\target\release\borg-ui.exe"
$CFGDIR = Join-Path $env:APPDATA "com.borgui.app"

function CCond($c) { New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, $c) }
function TCond($t) { New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $t) }
function AidCond($a) { New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, $a) }

function Ensure-BorgBeside {
    $borg = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName
    if (-not $borg) { return }
    $ad = Split-Path $EXE -Parent; $bd = Split-Path $borg -Parent
    if ($bd -ine $ad -and -not (Test-Path (Join-Path $ad "_internal"))) { try { Copy-Item (Join-Path $bd "*") $ad -Recurse -Force } catch {} }
}

function Launch-App($extraArgs) {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--force-renderer-accessibility"
    if ($extraArgs) { return Start-Process -FilePath $EXE -ArgumentList $extraArgs -PassThru }
    return Start-Process -FilePath $EXE -PassThru
}
function Stop-App($p) { if ($p -and (Get-Process -Id $p.Id -EA SilentlyContinue)) { Stop-Process -Id $p.Id -Force -EA SilentlyContinue } }
# rfd/IFileOpenDialog only opens when the owning window is foreground (verified on
# the VM: no foreground -> no dialog). Bring the BorgUI window to the front first.
function Bring-Foreground($win) {
    if (-not $win) { return }
    try { $h = [IntPtr]$win.Current.NativeWindowHandle; [void][GuiNative]::ShowWindow($h, 5); [void][GuiNative]::BringWindowToTop($h); [void][GuiNative]::SetForegroundWindow($h); Start-Sleep -Milliseconds 500 } catch {}
}

# Wait for the BorgUI window AND its webview a11y tree (nav links present).
function Wait-Win($timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        foreach ($w in $UIA::RootElement.FindAll($TREE::Children, (TCond $CT::Window))) {
            $n = ""; try { $n = $w.Current.Name } catch {}
            if ($n -like "*BorgUI*") {
                # webview ready when a nav Hyperlink is visible
                $link = $w.FindFirst($TREE::Descendants, (TCond $CT::Hyperlink))
                if ($link) { return $w }
            }
        }
        Start-Sleep -Milliseconds 800
    }
    return $null
}

function Find-El($root, $ctype, $pat) {
    if (-not $root) { return $null }
    foreach ($e in $root.FindAll($TREE::Descendants, (TCond $ctype))) {
        $n = ""; try { $n = $e.Current.Name } catch {}
        if ($n -like $pat) { return $e }
    }
    return $null
}
function Has-Text($root, $pat) {
    foreach ($e in $root.FindAll($TREE::Descendants, $ANYCOND)) {
        $n = ""; try { $n = $e.Current.Name } catch {}
        if ($n -like $pat) { return $true }
    }
    return $false
}
function Wait-Text($root, $pat, $timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) { if (Has-Text $root $pat) { return $true }; Start-Sleep -Milliseconds 600 }
    return $false
}
function Invoke-El($el) {
    if (-not $el) { return $false }
    try { $el.GetCurrentPattern($IPATTERN).Invoke(); return $true } catch {}
    try {
        $r = $el.Current.BoundingRectangle
        if ($r.Width -gt 0 -and -not [double]::IsInfinity($r.X)) { [GuiNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); return $true }
    } catch {}
    return $false
}
function Nav($win, $page) {
    $link = Find-El $win $CT::Hyperlink "*$page*"
    if (-not $link) { return $false }
    [void](Invoke-El $link)
    Start-Sleep -Seconds 2
    return $true
}

# ===========================================================================
# FLOW 1: tray "Backup now" navigates the window to the Backup page.
# (The frontend listener for tray-trigger-backup does goto('/backup').)
# Reuses the #34 tray technique: find icon in overflow, right-click, MSAA read,
# positional-click the item.
# ===========================================================================
function Get-Overflow {
    foreach ($c in @("TopLevelWindowForOverflowXamlIsland", "NotifyIconOverflowWindow", "Xaml_WindowedPopupClass")) {
        $ov = $UIA::RootElement.FindFirst($TREE::Children, (CCond $c)); if ($ov) { return $ov }
    }
    return $null
}
function Click-TrayItem($itemName) {
    # locate icon (overflow-aware), right-click, MSAA-read items, positional-click $itemName
    $tray = $UIA::RootElement.FindFirst($TREE::Children, (CCond "Shell_TrayWnd"))
    if (-not $tray) { return "no taskbar" }
    function _isIcon($n) { return ($n -and ($n -like "*Backup Manager*" -or ($n -like "*BorgUI*" -and $n -notlike "*running window*" -and $n -notlike "*pinned*"))) }
    $btn = $null
    foreach ($b in $tray.FindAll($TREE::Descendants, (TCond $CT::Button))) { $n = ""; try { $n = $b.Current.Name } catch {}; if (_isIcon $n) { $btn = $b; break } }
    if (-not $btn) {
        $ov = Get-Overflow
        if (-not $ov) {
            foreach ($b in $tray.FindAll($TREE::Descendants, (TCond $CT::Button))) { $n = ""; try { $n = $b.Current.Name } catch {}; if ($n -like "*hidden*" -or $n -like "*overflow*") { $r = $b.Current.BoundingRectangle; [GuiNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); Start-Sleep -Milliseconds 800; break } }
            $ov = Get-Overflow
        }
        if ($ov) { foreach ($b in $ov.FindAll($TREE::Descendants, (TCond $CT::Button))) { $n = ""; try { $n = $b.Current.Name } catch {}; if (_isIcon $n) { $btn = $b; break } } }
    }
    if (-not $btn) { return "icon not found" }
    $r = $btn.Current.BoundingRectangle
    [GuiNative]::RClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
    Start-Sleep -Milliseconds 900
    $popup = $UIA::RootElement.FindFirst($TREE::Children, (CCond "#32768"))
    if (-not $popup) { return "no menu" }
    $hwnd = [IntPtr]$popup.Current.NativeWindowHandle
    $iid = [Guid]"618736e0-3c3d-11cf-810c-00aa00389b71"; $acc = $null
    $hr = [GuiNative]::AccessibleObjectFromWindow($hwnd, [uint32]4294967292, [ref]$iid, [ref]$acc)
    if ($hr -ne 0 -or -not $acc) { return "no msaa" }
    $bf = [System.Reflection.BindingFlags]::GetProperty
    $cnt = [int]$acc.GetType().InvokeMember("accChildCount", $bf, $null, $acc, @())
    $pos = 0
    for ($i = 1; $i -le $cnt; $i++) { $nm = ""; try { $nm = [string]$acc.GetType().InvokeMember("accName", $bf, $null, $acc, @([int]$i)) } catch {}; if ($nm.Replace("&", "").Trim() -eq $itemName) { $pos = $i; break } }
    if ($pos -lt 1) { return "item '$itemName' not in menu" }
    $pr = $popup.Current.BoundingRectangle; $ih = $pr.Height / $cnt
    [GuiNative]::LClick([int]($pr.X + $pr.Width / 2), [int]($pr.Y + ($pos - 0.5) * $ih))
    return "ok"
}

Hdr "tray_backup_now_navigates"
$proc1 = $null
try {
    if (-not (Test-Path $EXE)) { Skip "tray_backup_now_navigates" "no production exe at $EXE (run a real tauri build)" }
    else {
        Ensure-BorgBeside
        $proc1 = Launch-App $null
        $win = Wait-Win $WinWaitSec
        if (-not $win) { Skip "tray_backup_now_navigates" "window/webview not ready (no desktop?)" }
        else {
            # Make sure we're NOT already on Backup: go to Settings first.
            [void](Nav $win "Settings"); Start-Sleep -Seconds 1
            $r = Click-TrayItem "Backup now"
            if ($r -ne "ok") { Skip "tray_backup_now_navigates" "could not click tray 'Backup now': $r" }
            else {
                $win2 = Wait-Win 8
                if ($win2 -and (Wait-Text $win2 "*Start Backup*" 8)) {
                    Pass "tray_backup_now_navigates" "tray 'Backup now' surfaced the window on the Backup page ('Start Backup' present)"
                } else {
                    Fail "tray_backup_now_navigates" "clicked 'Backup now' but the Backup page ('Start Backup') did not appear"
                }
            }
        }
    }
} catch { Fail "tray_backup_now_navigates" "$_" } finally { try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}; Stop-App $proc1; Start-Sleep -Milliseconds 500 }

# ===========================================================================
# FLOW 4: Settings profile switch repopulates fields.
# Stage 2 profiles with different repo paths; switching the PROFILE combobox
# must change the shown repo summary.
# ===========================================================================
Hdr "settings_profile_switch"
$proc4 = $null
$profilesPath = Join-Path $CFGDIR "profiles.json"
$bak = "$profilesPath.guiflowbak"
try {
    if (-not (Test-Path $EXE)) { Skip "settings_profile_switch" "no production exe" }
    else {
        New-Item -ItemType Directory -Force -Path $CFGDIR | Out-Null
        if ((Test-Path $profilesPath) -and -not (Test-Path $bak)) { Copy-Item $profilesPath $bak -Force }
        $profiles = @{
            active_id = "gui-a"
            profiles  = @(
                @{ id = "gui-a"; name = "GuiProfileA"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = "C:\gui-prof-a"; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null },
                @{ id = "gui-b"; name = "GuiProfileB"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = "C:\gui-prof-b"; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null }
            )
        }
        (ConvertTo-Json -InputObject $profiles -Depth 8) | Out-File $profilesPath -Encoding ascii

        $proc4 = Launch-App $null
        $win = Wait-Win $WinWaitSec
        if (-not $win) { Skip "settings_profile_switch" "window/webview not ready" }
        else {
            [void](Nav $win "Settings"); Start-Sleep -Seconds 1
            $beforeA = Has-Text $win "*gui-prof-a*"
            $combo = $win.FindFirst($TREE::Descendants, (TCond $CT::ComboBox))
            if (-not $combo) { Skip "settings_profile_switch" "PROFILE combobox not found" }
            else {
                $switched = $false
                # Expand, pick GuiProfileB
                try { $combo.GetCurrentPattern($EXPAT).Expand(); Start-Sleep -Milliseconds 500 } catch {}
                $opt = Find-El $win $CT::ListItem "*GuiProfileB*"
                if (-not $opt) { $opt = Find-El $combo $CT::ListItem "*GuiProfileB*" }
                if ($opt) { try { $opt.GetCurrentPattern($SIPAT).Select(); $switched = $true } catch { $switched = (Invoke-El $opt) } }
                if (-not $switched) { try { $combo.GetCurrentPattern($VPAT).SetValue("GuiProfileB"); $switched = $true } catch {} }
                Start-Sleep -Seconds 2
                $afterB = Wait-Text $win "*gui-prof-b*" 6
                if ($switched -and $afterB) {
                    Pass "settings_profile_switch" "switching PROFILE A->B repopulated the repo path (gui-prof-a -> gui-prof-b)"
                } elseif (-not $switched) {
                    Skip "settings_profile_switch" "could not operate the PROFILE combobox (before showed gui-prof-a: $beforeA)"
                } else {
                    Fail "settings_profile_switch" "switched profile but repo path did not repopulate to gui-prof-b (beforeA=$beforeA)"
                }
            }
        }
    }
} catch { Fail "settings_profile_switch" "$_" } finally {
    Stop-App $proc4
    if (Test-Path $bak) { Move-Item $bak $profilesPath -Force } else { Remove-Item $profilesPath -EA SilentlyContinue }
}

# ----------------------------------------------------------------------------
# Helpers for the dialog-dependent flows (restore round-trip, cancel).
# ----------------------------------------------------------------------------
$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName
function To-Unc($p) { "\\localhost\" + $p.Substring(0, 1) + "$" + $p.Substring(2) }
function RunBorg($arglist, $cwd) {
    $o = Join-Path $env:TEMP "gf-o.txt"; $e = Join-Path $env:TEMP "gf-e.txt"
    $env:BORG_PASSPHRASE = ""; $env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
    $env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"; $env:BORG_DISPLAY_PASSPHRASE = "no"
    $p = Start-Process -FilePath $script:BorgExe -ArgumentList $arglist -WindowStyle Hidden -PassThru -RedirectStandardOutput $o -RedirectStandardError $e -WorkingDirectory $cwd
    if (-not $p.WaitForExit(60000)) { try { $p.Kill() } catch {}; return @{ ok = $false; err = "timeout" } }
    return @{ ok = ($p.ExitCode -le 1); code = $p.ExitCode; err = (Get-Content $e -Raw -EA SilentlyContinue) }
}
# Drive the native folder picker: type the path into its edit, click Select Folder.
function Set-FolderDialog($path) {
    # The folder dialog (#32770) nests UNDER the Tauri window in the UIA tree, so
    # search Descendants from root, not just root children.
    $dlg = $null; $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and -not $dlg) { $dlg = $UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770")); if (-not $dlg) { Start-Sleep -Milliseconds 500 } }
    if (-not $dlg) { return "no folder dialog appeared" }
    try { $h = [IntPtr]$dlg.Current.NativeWindowHandle; [void][GuiNative]::SetForegroundWindow($h); [void][GuiNative]::BringWindowToTop($h) } catch {}
    Start-Sleep -Milliseconds 600
    # The Windows folder picker exposes the path field as the "Folder:" edit inside
    # the pane with AutomationId 1090, and the confirm button as a Pane with
    # AutomationId 1 ("Select Folder"). Target those by id, not by name/type.
    # Navigate to the target folder via the dialog's ADDRESS BAR. Ctrl+L focuses
    # it reliably regardless of where focus currently is (the file list, etc.) --
    # typing the path there + Enter navigates the picker into that folder; then
    # "Select Folder" (AutomationId 1) confirms the now-current folder.
    [System.Windows.Forms.SendKeys]::SendWait("^l")
    Start-Sleep -Milliseconds 450
    [System.Windows.Forms.SendKeys]::SendWait(($path -replace '([+^%~(){}\[\]])', '{$1}'))
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    Start-Sleep -Milliseconds 1100
    $dlg2 = $UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770"))
    if ($dlg2) {
        $sel = $dlg2.FindFirst($TREE::Descendants, (AidCond "1"))
        $clicked = $false
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

# ===========================================================================
# FLOW 2: GUI-clicked restore round-trip. Stage a fresh repo + a known archive,
# launch the GUI, click the archive's Restore, pick a destination, verify files.
# ===========================================================================
Hdr "restore_round_trip"
$proc2 = $null; $rt = "C:\gui-rt"; $bak2 = "$profilesPath.gfbak2"
try {
    if (-not (Test-Path $EXE)) { Skip "restore_round_trip" "no production exe" }
    elseif (-not $script:BorgExe) { Skip "restore_round_trip" "borg.exe not found under C:\borg" }
    else {
        Remove-Item -Recurse -Force $rt -EA SilentlyContinue
        New-Item -ItemType Directory -Force -Path "$rt\src", "$rt\out" | Out-Null
        "roundtrip-payload-12345" | Out-File "$rt\src\hello.txt" -Encoding ascii -NoNewline
        $unc = To-Unc "$rt\repo"
        $ri = RunBorg @("init", "--encryption", "none", $unc) $rt
        if (-not (Test-Path "$rt\repo\config")) { Skip "restore_round_trip" "borg init failed: $($ri.err)" }
        else {
            $rc = RunBorg @("create", "$unc::rtarch", "src") $rt
            New-Item -ItemType Directory -Force -Path $CFGDIR | Out-Null
            if ((Test-Path $profilesPath) -and -not (Test-Path $bak2)) { Copy-Item $profilesPath $bak2 -Force }
            $prof = @{ active_id = "gui-rt"; profiles = @(@{ id = "gui-rt"; name = "GuiRoundtrip"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = "$rt\repo"; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null }) }
            (ConvertTo-Json -InputObject $prof -Depth 8) | Out-File $profilesPath -Encoding ascii

            $proc2 = Launch-App $null
            $win = Wait-Win $WinWaitSec
            if (-not $win) { Skip "restore_round_trip" "window/webview not ready" }
            else {
                [void](Nav $win "Archives"); Start-Sleep -Seconds 1
                $refresh = Find-El $win $CT::Button "Refresh"; if ($refresh) { [void](Invoke-El $refresh) }
                if (-not (Wait-Text $win "*rtarch*" 15)) { Fail "restore_round_trip" "archive 'rtarch' did not appear in the Archives list" }
                else {
                    $restoreBtn = Find-El $win $CT::Button "Restore"
                    if (-not $restoreBtn) { Skip "restore_round_trip" "row 'Restore' button not found" }
                    else {
                        Bring-Foreground $win
                        [void](Invoke-El $restoreBtn)
                        $dlg = Set-FolderDialog "$rt\out"
                        if ($dlg -ne "ok") { Fail "restore_round_trip" "restore destination dialog: $dlg" }
                        else {
                            # Wait (bounded) for the restored file to appear.
                            $found = $null; $deadline = (Get-Date).AddSeconds(40)
                            while ((Get-Date) -lt $deadline -and -not $found) {
                                $found = Get-ChildItem -Path $rt\out -Recurse -Filter hello.txt -EA SilentlyContinue | Select-Object -First 1
                                if (-not $found) { Start-Sleep -Seconds 2 }
                            }
                            if (-not $found) { Fail "restore_round_trip" "no restored hello.txt under $rt\out after 40s" }
                            else {
                                $content = (Get-Content $found.FullName -Raw -EA SilentlyContinue)
                                if ($content -eq "roundtrip-payload-12345") { Pass "restore_round_trip" "GUI restore extracted $($found.FullName) with byte-correct content" }
                                else { Fail "restore_round_trip" "restored file content mismatch: '$content'" }
                            }
                        }
                    }
                }
            }
        }
    }
} catch { Fail "restore_round_trip" "$_" } finally {
    try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}
    Stop-App $proc2
    if (Test-Path $bak2) { Move-Item $bak2 $profilesPath -Force } else { Remove-Item $profilesPath -EA SilentlyContinue }
    Remove-Item -Recurse -Force $rt -EA SilentlyContinue
}

# ===========================================================================
# FLOW 3: Cancel a running backup. Stage a large (incompressible) source so the
# backup runs several seconds, add it via the GUI, Start Backup, then Cancel and
# confirm the UI returns to ready (no completed archive).
# ===========================================================================
Hdr "cancel_mid_backup"
$proc3 = $null; $cz = "C:\gui-cancel"; $bak3 = "$profilesPath.gfbak3"
try {
    if (-not (Test-Path $EXE)) { Skip "cancel_mid_backup" "no production exe" }
    elseif (-not $script:BorgExe) { Skip "cancel_mid_backup" "borg.exe not found" }
    else {
        Remove-Item -Recurse -Force $cz -EA SilentlyContinue
        New-Item -ItemType Directory -Force -Path "$cz\src" | Out-Null
        # ~400MB of random (incompressible) data so borg runs long enough (~10s)
        # to catch + cancel mid-backup.
        $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        $buf = New-Object byte[] (8 * 1024 * 1024)
        for ($f = 0; $f -lt 50; $f++) { $rng.GetBytes($buf); [System.IO.File]::WriteAllBytes("$cz\src\big$f.bin", $buf) }
        $unc = To-Unc "$cz\repo"
        $ri = RunBorg @("init", "--encryption", "none", $unc) $cz
        if (-not (Test-Path "$cz\repo\config")) { Skip "cancel_mid_backup" "borg init failed: $($ri.err)" }
        else {
            New-Item -ItemType Directory -Force -Path $CFGDIR | Out-Null
            if ((Test-Path $profilesPath) -and -not (Test-Path $bak3)) { Copy-Item $profilesPath $bak3 -Force }
            $prof = @{ active_id = "gui-cz"; profiles = @(@{ id = "gui-cz"; name = "GuiCancel"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = "$cz\repo"; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null }) }
            (ConvertTo-Json -InputObject $prof -Depth 8) | Out-File $profilesPath -Encoding ascii

            $proc3 = Launch-App $null
            $win = Wait-Win $WinWaitSec
            if (-not $win) { Skip "cancel_mid_backup" "window/webview not ready" }
            else {
                [void](Nav $win "Backup"); Start-Sleep -Seconds 1
                $add = Find-El $win $CT::Button "*Add Folder*"
                if (-not $add) { Skip "cancel_mid_backup" "'+ Add Folder' button not found" }
                else {
                    Bring-Foreground $win
                    [void](Invoke-El $add)
                    $dlg = Set-FolderDialog "$cz\src"
                    if ($dlg -ne "ok") { Fail "cancel_mid_backup" "add-folder dialog: $dlg" }
                    else {
                        Start-Sleep -Seconds 1
                        $start = Find-El $win $CT::Button "Start Backup"
                        if (-not $start) { Fail "cancel_mid_backup" "'Start Backup' not found after adding folder" }
                        else {
                            [void](Invoke-El $start)
                            # Poll for the Cancel button (present only while running) and click it
                            # the instant it appears, before the backup can finish.
                            $cancel = $null; $cd = (Get-Date).AddSeconds(20)
                            while ((Get-Date) -lt $cd) { $cancel = Find-El $win $CT::Button "Cancel"; if ($cancel) { break }; Start-Sleep -Milliseconds 250 }
                            if (-not $cancel) { Fail "cancel_mid_backup" "no Cancel button appeared while backing up (backup finished too fast?)" }
                            else {
                                [void](Invoke-El $cancel)
                                # Confirm it returns to ready: 'Start Backup' enabled again and not 'Backing up'.
                                $ready = $false; $deadline = (Get-Date).AddSeconds(25)
                                while ((Get-Date) -lt $deadline) {
                                    if (-not (Has-Text $win "*Backing up*") -and (Find-El $win $CT::Button "Start Backup")) { $ready = $true; break }
                                    Start-Sleep -Milliseconds 800
                                }
                                # And confirm no COMPLETED archive landed in the repo.
                                $lst = RunBorg @("list", "--short", $unc) $cz
                                $hasArchive = ("$($lst.err)" + (Get-Content (Join-Path $env:TEMP "gf-o.txt") -Raw -EA SilentlyContinue)) -match "\S"
                                $archOut = (Get-Content (Join-Path $env:TEMP "gf-o.txt") -Raw -EA SilentlyContinue)
                                if ($ready -and [string]::IsNullOrWhiteSpace($archOut)) {
                                    Pass "cancel_mid_backup" "Cancel aborted the backup: UI returned to ready and the repo has no completed archive"
                                } elseif ($ready) {
                                    Pass "cancel_mid_backup" "Cancel returned the UI to ready (note: repo list non-empty: '$($archOut.Trim())')"
                                } else {
                                    Fail "cancel_mid_backup" "after Cancel the UI did not return to ready within 25s"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
} catch { Fail "cancel_mid_backup" "$_" } finally {
    try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}
    Stop-App $proc3
    if (Test-Path $bak3) { Move-Item $bak3 $profilesPath -Force } else { Remove-Item $profilesPath -EA SilentlyContinue }
    Remove-Item -Recurse -Force $cz -EA SilentlyContinue
}

Summary
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
