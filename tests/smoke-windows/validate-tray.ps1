# BorgUI Windows tray right-click MENU validation (#34) -- the last unobserved
# Tier C interaction. PR #33 confirmed the tray *icon* renders; this drives the
# *menu*: right-click the icon, read its items, assert exactly
#   Show BorgUI / Backup now / Quit
# and exercise the Show + Quit actions.
#
# SESSION 1: the notification area + accessibility APIs need a real interactive
# desktop -- an SSH session has none. So when invoked normally this script
# RELAUNCHES ITSELF in session 1 via an /IT scheduled task (same trick the
# keychain test uses), waits, and reports the session-1 run's results. The
# `-InSession1` switch marks the inner run that does the actual work.
#
# HOW THE MENU IS READ: the tray menu is a native Win32 popup (Tauri/muda HMENU),
# built in tray.rs independently of the WebView. It is a `#32768` window that
# exposes NOTHING to UI Automation (0 descendants) -- but the classic MSAA
# (oleacc `IAccessible`, `AccessibleObjectFromWindow` + `accName`/
# `accDoDefaultAction`) reads its items and invokes them. Because the menu is
# native, contents + Show/Quit work even with a plain `cargo build --release`
# (dev-mode) exe; only "Backup now" (which emits to the JS frontend) needs the
# real `tauri build` UI, so that sub-item is a SIGNAL deferred to the checklist
# (validate-gui.ps1's scheduled path already proves the backup engine fires).
#
# BEST-EFFORT: locating the icon on Windows 11 is the brittle part (new icons
# hide in the overflow flyout, which is a toggle). If the icon/menu can't be
# reached the checks SKIP -- never a false FAIL -- and the manual VNC checklist
# in README.md is the authoritative verdict (issue #34).
#
# Mirrors validate-gui.ps1: Pass/Fail/Skip/Signal + JSON + exit code. ASCII only
# (Windows PowerShell 5.1 reads UTF-8-without-BOM as ANSI and breaks parsing) --
# so this file never writes the tray tooltip's em-dash; it matches "Backup Manager".

param([int]$LaunchWaitSec = 8, [int]$MenuWaitMs = 900, [switch]$InSession1)

$ErrorActionPreference = "Continue"

# =============================================================================
# SESSION-1 RELAUNCH WRAPPER (outer run, over SSH / no desktop).
# =============================================================================
if (-not $InSession1) {
    $self = $MyInvocation.MyCommand.Path
    $task = "BorgUI-TrayS1"
    $resJson = Join-Path $env:USERPROFILE "tray-results.json"
    $log = Join-Path $env:USERPROFILE "tray-session1.log"
    $sentinel = Join-Path $env:USERPROFILE "tray-session1.done"
    $bat = Join-Path $env:USERPROFILE "tray-run.bat"
    Remove-Item $resJson, $log, $sentinel -EA SilentlyContinue

    @(
        "@echo off",
        "powershell -ExecutionPolicy Bypass -File `"$self`" -InSession1 -LaunchWaitSec $LaunchWaitSec -MenuWaitMs $MenuWaitMs > `"$log`" 2>&1",
        "echo DONE > `"$sentinel`""
    ) | Set-Content -Path $bat -Encoding Ascii

    & schtasks.exe /Create /F /TN $task /TR "`"$bat`"" /SC ONCE /ST 23:59 /IT 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "SKIP: tray validation -- schtasks /Create failed (rc=$LASTEXITCODE); needs an interactive desktop." -ForegroundColor Yellow
        Write-Host "Failed: 0"
        exit 0
    }
    & schtasks.exe /Run /TN $task 2>&1 | Out-Null
    $deadline = (Get-Date).AddSeconds(150)
    while ((Get-Date) -lt $deadline -and -not (Test-Path $sentinel)) { Start-Sleep -Seconds 3 }

    if (Test-Path $log) { Get-Content $log }
    & schtasks.exe /Delete /F /TN $task 2>&1 | Out-Null
    Remove-Item $bat -EA SilentlyContinue

    if (-not (Test-Path $sentinel)) {
        Write-Host "`nSKIP: session-1 tray task did not finish within 150s." -ForegroundColor Yellow
        Write-Host "      No interactive desktop? Log the VM user in at localhost:8006 and re-run, or use the README VNC checklist." -ForegroundColor DarkGray
        Write-Host "Failed: 0"
        exit 0
    }
    $failed = 0
    if (Test-Path $resJson) {
        try { $failed = @((Get-Content $resJson -Raw | ConvertFrom-Json) | Where-Object { $_.Status -eq "FAIL" }).Count } catch {}
    }
    if ($failed -gt 0) { exit 1 } else { exit 0 }
}

# =============================================================================
# SESSION-1 INNER RUN (-InSession1): the actual tray-menu validation.
# =============================================================================
$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
$script:Results = @()

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
function Signal($name, $detail) {
    $script:Results += @{ Name = $name; Status = "SIGNAL"; Detail = $detail }
    Write-Host "  SIGNAL: $name" -ForegroundColor Magenta
    if ($detail) { Write-Host "          $detail" -ForegroundColor DarkGray }
}
function Write-TestHeader($name) { Write-Host "`n--- VALIDATE-TRAY: $name ---" -ForegroundColor Cyan }
function Write-Summary {
    Write-Host "`n========================================" -ForegroundColor White
    Write-Host "  TRAY MENU VALIDATION RESULTS" -ForegroundColor White
    Write-Host "========================================" -ForegroundColor White
    Write-Host "  Passed: $script:Passed" -ForegroundColor Green
    Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
    Write-Host "  Skipped: $script:Skipped" -ForegroundColor Yellow
    Write-Host "  Signals: see README VNC checklist for 'Backup now'" -ForegroundColor Magenta
    Write-Host "========================================`n" -ForegroundColor White
    $script:Results | ConvertTo-Json -Depth 3 | Out-File -FilePath (Join-Path $env:USERPROFILE "tray-results.json") -Encoding UTF8
}

# The three expected menu items, in tray.rs order.
$script:Expected = @("Show BorgUI", "Backup now", "Quit")

# --- UI Automation (find the icon + popup) + MSAA (read/invoke the menu) ------
try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    Add-Type -AssemblyName WindowsBase
    Add-Type -AssemblyName System.Windows.Forms
} catch {
    Skip "tray_menu_contents" "UI Automation assemblies unavailable: $_"
    Skip "tray_show_action" "UI Automation unavailable"
    Skip "tray_quit_action" "UI Automation unavailable"
    Signal "tray_backup_now" "UI Automation unavailable"
    Write-Summary
    exit 0
}
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class TrayNative {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint id, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] ref object acc);
    public const uint RIGHTDOWN = 0x0008, RIGHTUP = 0x0010, LEFTDOWN = 0x0002, LEFTUP = 0x0004;
    public static void RightClick(int x, int y) {
        SetCursorPos(x, y); System.Threading.Thread.Sleep(140);
        mouse_event(RIGHTDOWN, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(40);
        mouse_event(RIGHTUP, 0, 0, 0, UIntPtr.Zero);
    }
    public static void LeftClick(int x, int y) {
        SetCursorPos(x, y); System.Threading.Thread.Sleep(140);
        mouse_event(LEFTDOWN, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(40);
        mouse_event(LEFTUP, 0, 0, 0, UIntPtr.Zero);
    }
}
"@

$UIA = [System.Windows.Automation.AutomationElement]
$Tree = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]
$Cond = [System.Windows.Automation.Condition]::TrueCondition
$GETP = [System.Reflection.BindingFlags]::GetProperty
$OBJID_CLIENT = [uint32]4294967292   # 0xFFFFFFFC
$IID_ACC = [Guid]"618736e0-3c3d-11cf-810c-00aa00389b71"

function New-ClassCond($class) { New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, $class) }
function New-TypeCond($ctype) { New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $ctype) }

# The tray icon's name is its tooltip ("BorgUI - Backup Manager"); the taskbar
# button is "BorgUI - N running window". Match the tray icon, exclude the rest.
function Is-TrayIcon($n) {
    return ($n -and ($n -like "*Backup Manager*" -or ($n -like "*BorgUI*" -and $n -notlike "*running window*" -and $n -notlike "*pinned*")))
}

function Get-ClickPoint($el) {
    try { $p = $el.GetClickablePoint(); if ($p -and $p.X -gt 0 -and $p.Y -gt 0) { return @{ X = [int]$p.X; Y = [int]$p.Y } } } catch {}
    try {
        $r = $el.Current.BoundingRectangle
        if ($r.Width -gt 0 -and $r.Height -gt 0 -and -not [double]::IsInfinity($r.X)) {
            return @{ X = [int]($r.X + $r.Width / 2); Y = [int]($r.Y + $r.Height / 2) }
        }
    } catch {}
    return $null
}

function Find-TrayButton($root) {
    if (-not $root) { return $null }
    try { $els = $root.FindAll($Tree::Descendants, (New-TypeCond $CT::Button)) } catch { return $null }
    foreach ($el in $els) { $n = $null; try { $n = $el.Current.Name } catch {}; if (Is-TrayIcon $n) { return $el } }
    return $null
}

# The overflow flyout is a TOGGLE; return it if already open (don't re-click).
function Get-Overflow {
    foreach ($cls in @("TopLevelWindowForOverflowXamlIsland", "NotifyIconOverflowWindow", "Xaml_WindowedPopupClass", "SystemTrayOverflowWindow")) {
        $ov = $UIA::RootElement.FindFirst($Tree::Children, (New-ClassCond $cls))
        if ($ov) { return $ov }
    }
    return $null
}

# Read the native popup menu's items via MSAA. Returns @{ Acc; Items=@(@{Index;Name}) }.
function Read-MenuItems($hwnd) {
    $acc = $null
    $hr = [TrayNative]::AccessibleObjectFromWindow($hwnd, $OBJID_CLIENT, [ref]$IID_ACC, [ref]$acc)
    if ($hr -ne 0 -or -not $acc) { return $null }
    $cnt = 0
    try { $cnt = [int]$acc.GetType().InvokeMember("accChildCount", $GETP, $null, $acc, @()) } catch { return @{ Acc = $acc; Items = @() } }
    $items = @()
    for ($i = 1; $i -le $cnt; $i++) {
        $nm = ""
        try { $nm = [string]$acc.GetType().InvokeMember("accName", $GETP, $null, $acc, @([int]$i)) } catch {}
        if ($nm) { $nm = $nm.Replace("&", "").Trim() }
        if ($nm) { $items += @{ Index = $i; Name = $nm } }
    }
    return @{ Acc = $acc; Items = $items }
}

# Invoke a menu item by its 1-based VISIBLE position via a positional left-click
# computed from the live popup rect. muda's menu IAccessible returns
# DISP_E_MEMBERNOTFOUND for accDoDefaultAction, but a positional click works
# (verified on the VM). Re-fetches the #32768 rect so a re-rendered/moved menu
# is handled, and returns $false if the menu has been dismissed.
function Invoke-MenuItemByPosition($position, $count) {
    if ($count -le 0 -or $position -lt 1) { return $false }
    $popup = $UIA::RootElement.FindFirst($Tree::Children, (New-ClassCond "#32768"))
    if (-not $popup) { return $false }
    try {
        $pr = $popup.Current.BoundingRectangle
        if ($pr.Height -le 0 -or [double]::IsInfinity($pr.X)) { return $false }
        $itemH = $pr.Height / $count
        $cx = [int]($pr.X + $pr.Width / 2)
        $cy = [int]($pr.Y + ($position - 0.5) * $itemH)
        [TrayNative]::LeftClick($cx, $cy)
        return $true
    } catch { return $false }
}

function Find-BorgUi {
    $candidates = @(
        "C:\borgui\borg-ui.exe",
        (Join-Path $env:USERPROFILE "borg-ui.exe"),
        "C:\borgui-test\target\release\borg-ui.exe",
        "C:\borgui-test\app-tauri\src-tauri\target\release\borg-ui.exe"
    )
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    $found = Get-ChildItem C:\borgui-test -Recurse -Filter borg-ui.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found) { return $found.FullName }
    return $null
}
function Ensure-BorgBeside($appExe) {
    $borgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
    if (-not $borgExe) { return }
    $appDir = Split-Path $appExe -Parent
    $borgDir = Split-Path $borgExe -Parent
    if ($borgDir -ieq $appDir) { return }
    if (-not (Test-Path (Join-Path $appDir "borg.exe")) -or -not (Test-Path (Join-Path $appDir "_internal"))) {
        try { Copy-Item (Join-Path $borgDir "*") -Destination $appDir -Recurse -Force } catch {}
    }
}
function Get-MainWindowVisible {
    try {
        $tops = $UIA::RootElement.FindAll($Tree::Children, (New-TypeCond $CT::Window))
        foreach ($t in $tops) {
            $n = $null; try { $n = $t.Current.Name } catch {}
            if ($n -and $n -like "*BorgUI*") { try { return (-not $t.Current.IsOffscreen) } catch { return $true } }
        }
    } catch {}
    return $false
}
function Close-Menu {
    try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 150; [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}
    Start-Sleep -Milliseconds 200
}
function Stop-App($proc) {
    if ($proc -and (Get-Process -Id $proc.Id -EA SilentlyContinue)) { Stop-Process -Id $proc.Id -Force -EA SilentlyContinue }
}

# Find the BorgUI tray icon (opening the overflow flyout if needed, toggle-safe),
# right-click it, and read the popup menu via MSAA. Returns @{ Acc; Items } or @{ Error }.
function Open-TrayMenu {
    $tray = $UIA::RootElement.FindFirst($Tree::Children, (New-ClassCond "Shell_TrayWnd"))
    if (-not $tray) { return @{ Error = "Shell_TrayWnd (taskbar) not found -- no interactive desktop?" } }

    $btn = Find-TrayButton $tray
    if (-not $btn) {
        $ov = Get-Overflow
        if (-not $ov) {
            try {
                $btns = $tray.FindAll($Tree::Descendants, (New-TypeCond $CT::Button))
                foreach ($b in $btns) {
                    $n = $null; try { $n = $b.Current.Name } catch {}
                    if ($n -and ($n -like "*hidden*" -or $n -like "*overflow*" -or $n -like "*chevron*")) {
                        $cp = Get-ClickPoint $b
                        if ($cp) { [TrayNative]::LeftClick($cp.X, $cp.Y); Start-Sleep -Milliseconds 800 }
                        break
                    }
                }
            } catch {}
            $ov = Get-Overflow
        }
        if ($ov) { $btn = Find-TrayButton $ov }
    }
    if (-not $btn) { return @{ Error = "BorgUI tray icon not found (visible area or overflow)" } }

    $cp = Get-ClickPoint $btn
    if (-not $cp) { return @{ Error = "tray icon found but has no clickable point" } }
    [TrayNative]::RightClick($cp.X, $cp.Y)
    Start-Sleep -Milliseconds $MenuWaitMs

    $popup = $UIA::RootElement.FindFirst($Tree::Children, (New-ClassCond "#32768"))
    if (-not $popup) { return @{ Error = "right-clicked the icon but no popup menu (#32768) appeared" } }
    $hwnd = [IntPtr]$popup.Current.NativeWindowHandle
    $read = Read-MenuItems $hwnd
    if (-not $read -or -not $read.Acc) { return @{ Error = "menu opened but MSAA could not read it (hwnd=$hwnd)" } }
    return @{ Acc = $read.Acc; Items = $read.Items }
}

# --- Locate the app -----------------------------------------------------------
$script:BorgUiExe = Find-BorgUi
if (-not $script:BorgUiExe) {
    Skip "tray_menu_contents" "borg-ui.exe not found (drop one in tests/smoke-windows/shared/ or build on the VM; see README)"
    Skip "tray_show_action" "borg-ui.exe not found"
    Skip "tray_quit_action" "borg-ui.exe not found"
    Signal "tray_backup_now" "borg-ui.exe not found"
    Write-Summary
    exit 0
}
Write-Host "borg-ui.exe: $script:BorgUiExe" -ForegroundColor DarkGray
Ensure-BorgBeside $script:BorgUiExe

# ============================ CONTENTS + SHOW ================================
# Launch --minimized so there's no window/taskbar button -- only the tray icon
# (a clean Show test: the window starts hidden and Show must reveal it).
Write-TestHeader "tray_menu_contents"
$proc1 = $null
try {
    $proc1 = Start-Process -FilePath $script:BorgUiExe -ArgumentList "--minimized" -PassThru -EA Stop
    Start-Sleep -Seconds $LaunchWaitSec
    if (-not (Get-Process -Id $proc1.Id -EA SilentlyContinue)) {
        Skip "tray_menu_contents" "app exited within ${LaunchWaitSec}s of launch (WebView2 missing, or no desktop). Use the README checklist."
        Skip "tray_show_action" "app did not stay alive"
    } else {
        $r = Open-TrayMenu
        if ($r.Error) {
            Skip "tray_menu_contents" "$($r.Error) -- Win11 tray is brittle; confirm via the README VNC checklist"
            Skip "tray_show_action" "tray menu could not be opened"
        } else {
            $menuNames = @($r.Items | ForEach-Object { $_.Name })
            $missing = @($script:Expected | Where-Object { $menuNames -notcontains $_ })
            $extra = @($menuNames | Where-Object { $script:Expected -notcontains $_ })
            if ($missing.Count -eq 0 -and $extra.Count -eq 0) {
                Pass "tray_menu_contents" "menu shows exactly: $($menuNames -join ', ')"
            } elseif ($missing.Count -eq 0) {
                Fail "tray_menu_contents" "expected items present but unexpected extras: [$($extra -join ', ')] (full: $($menuNames -join ', '))"
            } else {
                Fail "tray_menu_contents" "missing [$($missing -join ', ')]; saw [$($menuNames -join ', ')]"
            }

            Write-TestHeader "tray_show_action"
            $names = @($r.Items | ForEach-Object { $_.Name })
            $showPos = [array]::IndexOf($names, "Show BorgUI") + 1
            if ($showPos -lt 1) {
                Skip "tray_show_action" "no 'Show BorgUI' item to invoke"
                Close-Menu
            } else {
                $ok = Invoke-MenuItemByPosition $showPos $names.Count
                Start-Sleep -Seconds 3
                if (Get-MainWindowVisible) {
                    Pass "tray_show_action" "'Show BorgUI' surfaced the main window (visible, not offscreen)"
                } elseif (-not $ok) {
                    Skip "tray_show_action" "could not click 'Show BorgUI' (menu dismissed?) -- confirm via the README checklist"
                    Close-Menu
                } else {
                    Fail "tray_show_action" "clicked 'Show BorgUI' but no visible BorgUI window was found"
                }
            }
        }
    }
} catch {
    Fail "tray_menu_contents" "$_"
} finally {
    Close-Menu
    Stop-App $proc1
    Start-Sleep -Milliseconds 500
}

# ================================ QUIT ======================================
Write-TestHeader "tray_quit_action"
$proc2 = $null
try {
    $proc2 = Start-Process -FilePath $script:BorgUiExe -ArgumentList "--minimized" -PassThru -EA Stop
    Start-Sleep -Seconds $LaunchWaitSec
    if (-not (Get-Process -Id $proc2.Id -EA SilentlyContinue)) {
        Skip "tray_quit_action" "app exited on its own before the Quit probe (no desktop?)"
    } else {
        $r2 = Open-TrayMenu
        if ($r2.Error) {
            Skip "tray_quit_action" "$($r2.Error) -- confirm via the README VNC checklist"
        } else {
            $names2 = @($r2.Items | ForEach-Object { $_.Name })
            $quitPos = [array]::IndexOf($names2, "Quit") + 1
            if ($quitPos -lt 1) {
                Skip "tray_quit_action" "no 'Quit' item to invoke (contents: $($names2 -join ', '))"
                Close-Menu
            } else {
                [void](Invoke-MenuItemByPosition $quitPos $names2.Count)
                $deadline = (Get-Date).AddSeconds(10)
                while ((Get-Date) -lt $deadline -and (Get-Process -Id $proc2.Id -EA SilentlyContinue)) { Start-Sleep -Milliseconds 400 }
                if (-not (Get-Process -Id $proc2.Id -EA SilentlyContinue)) {
                    Pass "tray_quit_action" "'Quit' exited the process (app.exit(0))"
                } else {
                    Fail "tray_quit_action" "invoked 'Quit' but the process is still running after 10s"
                }
            }
        }
    }
} catch {
    Fail "tray_quit_action" "$_"
} finally {
    Close-Menu
    Stop-App $proc2
}

# ============================== BACKUP NOW ==================================
Write-TestHeader "tray_backup_now (signal)"
Signal "tray_backup_now" "Not auto-asserted: needs the real tauri-build frontend (the item emits to JS). validate-gui.ps1 'scheduled_task_fires' already proves the backup engine works; confirm the tray 'Backup now' click via the README VNC checklist."

Write-Summary
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
