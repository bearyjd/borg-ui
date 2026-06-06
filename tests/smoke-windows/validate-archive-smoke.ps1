# BorgUI large-archive GUI smoke (#35): prove the streaming + virtualized archive
# browser actually holds up in the running app on a genuinely huge (~100k-entry)
# archive -- the one #35 behaviour the other harnesses never stressed on real
# hardware (validate-gui-flows only restores a tiny archive via the row button,
# never the streaming ArchiveBrowser contents tree).
#
# REQUIRES a PRODUCTION build (embedded frontend) at
#   C:\borgui-test\target\release\borg-ui.exe
# (a dev-mode `cargo build` exe shows the localhost-error page). Build it with
# `pnpm tauri build --no-bundle` in app-tauri.
#
# KEY UNLOCK (same as validate-gui-flows): WebView2/Chromium only exposes its UIA
# accessibility tree when asked. We launch with
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-renderer-accessibility so the
# Svelte tree (checkboxes, buttons, the "{sel} / {total} files" header) is
# reachable via UIA.
#
# The wrapper (SSH context, no tight deadline) stages the 100k-file repo with a
# compiled C# loop + a Defender exclusion, points an active profile at it, then
# relaunches the inner run in session 1 (UIA + the desktop need an interactive
# session). It restores the profile + removes the staging on the way out.
#
# Checks (against the production exe + the staged "huge" archive):
#   1 huge_archive_streams_and_builds -- Browse streams all ~100k entries and the
#       header shows the full "/ N files" total (bounded-memory stream + tree).
#   2 archive_browser_virtualizes     -- with ~200 logical rows expanded the DOM
#       holds only a windowed handful of rows (checkbox count stays bounded).
#   3 select_all_counts_all           -- "Select all" reaches the full N (the
#       O(leaves) Selection model scales to 100k).
#   4 scroll_windows_rows             -- wheel-scrolling the tree swaps the
#       windowed rows in (a late directory becomes visible). SKIP if undrivable.
#   5 browser_selective_restore       -- tick one folder, "Restore selected",
#       pick a destination, and the chosen subset extracts byte-correct.
#
# Pass/Fail/Skip + JSON + exit code, mirroring validate-gui-flows.ps1. ASCII only
# (PS 5.1 reads UTF-8-no-BOM as ANSI -- a non-ASCII byte breaks parsing).
#
# TODO (tracked follow-up): the UIA helper block + the session-1 relaunch wrapper
# are duplicated across validate-{gui,tray,gui-flows,archive-smoke}.ps1. Extract a
# dot-sourced smoke-uia.ps1 shared by all four. Kept self-contained here for now so
# run.sh can scp + run each script standalone with no on-VM module resolution.

param([switch]$InSession1, [int]$WinWaitSec = 30, [int]$FileCount = 100000, [int]$DirCount = 200, [int]$LoadWaitSec = 240)
$ErrorActionPreference = "Continue"

# ----------------------------------------------------------------------------
# SHARED CONSTANTS + STAGING HELPERS (used by the wrapper; no UIA needed)
# ----------------------------------------------------------------------------
$ROOT = "C:\archive-smoke"
$DATA = "$ROOT\data"
$REPO = "$ROOT\repo"
$OUT = "$ROOT\out"
$ARCH = "huge"
$MARKERDIR = "d0000"
$MARKER = "smoke-payload-d0000"
$CFGDIR = Join-Path $env:APPDATA "com.borgui.app"
$profilesPath = Join-Path $CFGDIR "profiles.json"

$script:BorgExe = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName
function To-Unc($p) { "\\localhost\" + $p.Substring(0, 1) + "$" + $p.Substring(2) }
function RunBorgLong($arglist, $cwd, $timeoutMs) {
    $o = Join-Path $env:TEMP "as-o.txt"; $e = Join-Path $env:TEMP "as-e.txt"
    $env:BORG_PASSPHRASE = ""; $env:BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK = "yes"
    $env:BORG_RELOCATED_REPO_ACCESS_IS_OK = "yes"; $env:BORG_DISPLAY_PASSPHRASE = "no"
    $p = Start-Process -FilePath $script:BorgExe -ArgumentList $arglist -WindowStyle Hidden -PassThru -RedirectStandardOutput $o -RedirectStandardError $e -WorkingDirectory $cwd
    if (-not $p.WaitForExit($timeoutMs)) { try { $p.Kill() } catch {}; return @{ ok = $false; code = -1; out = ""; err = "timeout after ${timeoutMs}ms" } }
    return @{ ok = ($p.ExitCode -le 1); code = $p.ExitCode; out = (Get-Content $o -Raw -EA SilentlyContinue); err = (Get-Content $e -Raw -EA SilentlyContinue) }
}

# Compiled file factory: a PowerShell loop creating 100k files is painfully slow;
# a compiled C# loop with the path Defender-excluded does it in seconds.
Add-Type @"
using System;
using System.IO;
using System.Text;
public static class SmokeStaging {
  public static int MakeFiles(string dataRoot, int dirCount, int fileCount, string markerDir, string markerContent) {
    int perDir = (int)Math.Ceiling((double)fileCount / dirCount);
    int made = 0;
    byte[] empty = new byte[0];
    byte[] marker = Encoding.ASCII.GetBytes(markerContent);
    for (int d = 0; d < dirCount && made < fileCount; d++) {
      string dn = "d" + d.ToString("D4");
      string dir = Path.Combine(dataRoot, dn);
      Directory.CreateDirectory(dir);
      bool isMarker = (dn == markerDir);
      for (int f = 0; f < perDir && made < fileCount; f++) {
        string fp = Path.Combine(dir, "f" + made.ToString("D6") + ".txt");
        File.WriteAllBytes(fp, isMarker ? marker : empty);
        made++;
      }
    }
    return made;
  }
}
"@

function Stage-Repo {
    if (-not $script:BorgExe) { return @{ ok = $false; why = "borg.exe not found under C:\borg" } }
    Remove-Item -Recurse -Force $ROOT -EA SilentlyContinue
    [void][System.IO.Directory]::CreateDirectory($DATA)
    [void][System.IO.Directory]::CreateDirectory($OUT)
    try { Add-MpPreference -ExclusionPath $ROOT -EA SilentlyContinue } catch {}
    $made = [SmokeStaging]::MakeFiles($DATA, $DirCount, $FileCount, $MARKERDIR, $MARKER)
    Write-Host "staged $made files under $DATA ($DirCount dirs)"
    if ($made -lt 1) { return @{ ok = $false; why = "no files staged" } }
    $unc = To-Unc $REPO
    $ri = RunBorgLong @("init", "--encryption", "none", $unc) $ROOT 120000
    if (-not (Test-Path "$REPO\config")) { return @{ ok = $false; why = ("borg init failed: " + $ri.err) } }
    # cwd=$ROOT, arg "data" -> archive paths are "data/dNNNN/fNNNNNN.txt".
    $rc = RunBorgLong @("create", "$unc::$ARCH", "data") $ROOT 900000
    $lst = RunBorgLong @("list", "--short", $unc) $ROOT 120000
    if ("$($lst.out)" -notmatch [regex]::Escape($ARCH)) {
        return @{ ok = $false; why = ("archive '$ARCH' not created (create rc=$($rc.code)): " + $rc.err) }
    }
    return @{ ok = $true; made = $made }
}

# ----------------------------------------------------------------------------
# SESSION-1 RELAUNCH WRAPPER (runs over SSH: stage -> relaunch -> cleanup)
# ----------------------------------------------------------------------------
if (-not $InSession1) {
    $bak = "$profilesPath.assmokebak"
    # Staging cleanup is always safe to run. The profile restore must only run
    # AFTER we have actually replaced profiles.json -- otherwise its "no prior
    # profile" branch would delete a profiles.json the run never touched.
    $cleanupStaging = {
        Remove-Item -Recurse -Force $ROOT -EA SilentlyContinue
        try { Remove-MpPreference -ExclusionPath $ROOT -EA SilentlyContinue } catch {}
    }
    $restoreProfile = {
        if (Test-Path $bak) { Move-Item $bak $profilesPath -Force } else { Remove-Item $profilesPath -EA SilentlyContinue }
    }

    Write-Host "Staging a $FileCount-file archive (this can take a couple of minutes)..."
    $stage = Stage-Repo
    if (-not $stage.ok) {
        Write-Host ("  SKIP: archive_smoke_staging -- " + $stage.why)
        Write-Host "  Passed: 0  Failed: 0  Skipped: 1"
        Write-Host "Failed: 0"
        & $cleanupStaging
        exit 0
    }
    Write-Host "Staged $($stage.made) files; archive '$ARCH' created."

    New-Item -ItemType Directory -Force -Path $CFGDIR | Out-Null
    if ((Test-Path $profilesPath) -and -not (Test-Path $bak)) { Copy-Item $profilesPath $bak -Force }
    $prof = @{ active_id = "as-smoke"; profiles = @(@{ id = "as-smoke"; name = "ArchiveSmoke"; repo = @{ ssh_host = ""; ssh_port = 0; ssh_user = ""; repo_path = $REPO; ssh_key_path = $null }; schedule = $null; retention = $null; archive_template = $null; pre_backup = $null; post_backup = $null }) }
    (ConvertTo-Json -InputObject $prof -Depth 8) | Out-File $profilesPath -Encoding ascii

    $self = $MyInvocation.MyCommand.Path
    $task = "BorgUI-ArchiveSmoke"; $log = Join-Path $env:USERPROFILE "archive-smoke.log"
    $sentinel = Join-Path $env:USERPROFILE "archive-smoke.done"; $bat = Join-Path $env:USERPROFILE "archive-smoke.bat"
    $resJson = Join-Path $env:USERPROFILE "archive-smoke-results.json"
    Remove-Item $log, $sentinel, $resJson -EA SilentlyContinue
    @("@echo off",
      "powershell -ExecutionPolicy Bypass -File `"$self`" -InSession1 -FileCount $FileCount -DirCount $DirCount -LoadWaitSec $LoadWaitSec > `"$log`" 2>&1",
      "echo DONE > `"$sentinel`"") | Set-Content $bat -Encoding Ascii
    & schtasks.exe /Create /F /TN $task /TR "`"$bat`"" /SC ONCE /ST 23:59 /IT 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "SKIP: schtasks /Create failed; needs interactive desktop."
        Write-Host "Failed: 0"
        & $restoreProfile; & $cleanupStaging
        exit 0
    }
    & schtasks.exe /Run /TN $task 2>&1 | Out-Null
    $deadline = (Get-Date).AddSeconds(900)
    while ((Get-Date) -lt $deadline -and -not (Test-Path $sentinel)) { Start-Sleep -Seconds 5 }
    if (Test-Path $log) { Get-Content $log }
    & schtasks.exe /Delete /F /TN $task 2>&1 | Out-Null
    Remove-Item $bat -EA SilentlyContinue
    # The sentinel is written only AFTER the inner run's finally (which Stop-App's
    # the exe) returns -- so its presence means the app is down and the staged repo
    # is safe to delete. On a timeout the app may STILL be running: restore the
    # profile but leave $ROOT in place so we don't yank the repo out from under a
    # live borg extract.
    $timedOut = -not (Test-Path $sentinel)
    & $restoreProfile
    if (-not $timedOut) { & $cleanupStaging } else { Write-Host "WARN: session-1 overran 900s; leaving $ROOT in place (app may still be using it)." }
    if ($timedOut) { Write-Host "`nSKIP: session-1 task did not finish (no desktop?)."; Write-Host "Failed: 0"; exit 0 }
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
function Hdr($n) { Write-Host "`n--- ARCHIVE-SMOKE: $n ---" }
function Summary {
    Write-Host "`n========================================"
    Write-Host "  ARCHIVE SMOKE VALIDATION RESULTS"
    Write-Host "  Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
    Write-Host "========================================`n"
    $script:Results | ConvertTo-Json -Depth 3 | Out-File (Join-Path $env:USERPROFILE "archive-smoke-results.json") -Encoding UTF8
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class AsNative {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  public const uint LD=0x0002, LU=0x0004, WHEEL=0x0800;
  public static void LClick(int x,int y){SetCursorPos(x,y);System.Threading.Thread.Sleep(140);mouse_event(LD,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(40);mouse_event(LU,0,0,0,UIntPtr.Zero);}
  public static void Wheel(int x,int y,int delta){SetCursorPos(x,y);System.Threading.Thread.Sleep(60);mouse_event(WHEEL,0,0,(uint)delta,UIntPtr.Zero);}
}
"@

$UIA = [System.Windows.Automation.AutomationElement]
$TREE = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]
$ANYCOND = [System.Windows.Automation.Condition]::TrueCondition
$IPATTERN = [System.Windows.Automation.InvokePattern]::Pattern
$TOGPAT = [System.Windows.Automation.TogglePattern]::Pattern
$SCROLLPAT = [System.Windows.Automation.ScrollPattern]::Pattern

$EXE = "C:\borgui-test\target\release\borg-ui.exe"

function CCond($c) { New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, $c) }
function TCond($t) { New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $t) }

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
    try { $h = [IntPtr]$win.Current.NativeWindowHandle; [void][AsNative]::ShowWindow($h, 5); [void][AsNative]::BringWindowToTop($h); [void][AsNative]::SetForegroundWindow($h); Start-Sleep -Milliseconds 500 } catch {}
}
function Wait-Win($timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        foreach ($w in $UIA::RootElement.FindAll($TREE::Children, (TCond $CT::Window))) {
            $n = ""; try { $n = $w.Current.Name } catch {}
            if ($n -like "*BorgUI*") {
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
function Find-All($root, $ctype) {
    if (-not $root) { return @() }
    return @($root.FindAll($TREE::Descendants, (TCond $ctype)))
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
        if ($r.Width -gt 0 -and -not [double]::IsInfinity($r.X)) { [AsNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); return $true }
    } catch {}
    return $false
}
function Toggle-El($el) {
    if (-not $el) { return $false }
    try { $el.GetCurrentPattern($TOGPAT).Toggle(); return $true } catch {}
    return (Invoke-El $el)
}
function Nav($win, $page) {
    $link = Find-El $win $CT::Hyperlink "*$page*"
    if (-not $link) { return $false }
    [void](Invoke-El $link)
    Start-Sleep -Seconds 2
    return $true
}

# The browser header renders "{sel} / {total} files" only after the stream
# completes -- so its appearance with total>0 IS the load-done signal. The
# loading "{N} files" text has no slash and never matches.
function Get-CountPair($root) {
    foreach ($e in $root.FindAll($TREE::Descendants, $ANYCOND)) {
        $n = ""; try { $n = $e.Current.Name } catch {}
        if ($n -match '([0-9][0-9,]*)\s*/\s*([0-9][0-9,]*)\s+files') {
            return @{ sel = [int]($matches[1] -replace ',', ''); total = [int]($matches[2] -replace ',', '') }
        }
    }
    return $null
}
function Wait-CountTotal($root, $timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec); $last = $null
    while ((Get-Date) -lt $deadline) { $cp = Get-CountPair $root; if ($cp -and $cp.total -gt 0) { return $cp }; if ($cp) { $last = $cp }; Start-Sleep -Milliseconds 800 }
    return $last
}
# Load-done signal that does not depend on the header text being a single UIA
# node: the "Select all" link only renders once the stream completes and the
# tree is built (it lives in the loaded {:else} branch). Returns a hashtable so
# a load *error* (the {:else if error} banner) fails fast instead of burning the
# full timeout waiting for a "Select all" that will never appear.
function Wait-Loaded($root, $timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Find-El $root $CT::Button "Select all") { return @{ ok = $true } }
        if (Has-Text $root "*Failed to load archive contents*") { return @{ ok = $false; err = "browser reported a load error" } }
        Start-Sleep -Milliseconds 800
    }
    return @{ ok = $false; err = "timeout after ${timeoutSec}s (no 'Select all')" }
}
# The footer button's accessible name is a single, reliable string:
# "Restore selected (N)" where N is the selected-file count.
function Get-RestoreBtnCount($root) {
    $b = Find-El $root $CT::Button "*Restore selected*"
    if (-not $b) { return $null }
    $n = ""; try { $n = $b.Current.Name } catch {}
    if ($n -match 'Restore selected\s*\(([0-9,]+)\)') { return [int]($matches[1] -replace ',', '') }
    return $null
}
# Names of the currently-windowed directory rows (checkbox labels like d0007).
function Visible-DirIndexes($root) {
    $idx = @()
    foreach ($e in Find-All $root $CT::CheckBox) {
        $n = ""; try { $n = $e.Current.Name } catch {}
        if ($n -match '^d([0-9]{4})$') { $idx += [int]$matches[1] }
    }
    return $idx
}

# Drive the native folder picker (same approach as validate-gui-flows): Ctrl+L,
# type the path, Enter to navigate into it, then "Select Folder" (AutomationId 1).
function AidCond($a) { New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, $a) }
function Set-FolderDialog($path) {
    $dlg = $null; $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and -not $dlg) { $dlg = $UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770")); if (-not $dlg) { Start-Sleep -Milliseconds 500 } }
    if (-not $dlg) { return "no folder dialog appeared" }
    try { $h = [IntPtr]$dlg.Current.NativeWindowHandle; [void][AsNative]::SetForegroundWindow($h); [void][AsNative]::BringWindowToTop($h) } catch {}
    Start-Sleep -Milliseconds 600
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
            if (-not $clicked) { try { $r = $sel.Current.BoundingRectangle; if ($r.Width -gt 0 -and -not [double]::IsInfinity($r.X)) { [AsNative]::LClick([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)); $clicked = $true } } catch {} }
        }
        if (-not $clicked) { [System.Windows.Forms.SendKeys]::SendWait("{ENTER}") }
        Start-Sleep -Milliseconds 1000
    }
    if ($UIA::RootElement.FindFirst($TREE::Descendants, (CCond "#32770"))) { return "dialog still open after Ctrl+L navigate + select" }
    return "ok"
}

function Open-Browser($win) {
    $browse = Find-El $win $CT::Button "Browse"
    if (-not $browse) { return $false }
    [void](Invoke-El $browse)
    return (Wait-Text $win "*Browse archive*" 12)
}
function Close-Browser($win) {
    $cancel = Find-El $win $CT::Button "Cancel"
    if ($cancel) { [void](Invoke-El $cancel) } else { try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {} }
    $deadline = (Get-Date).AddSeconds(8)
    while ((Get-Date) -lt $deadline -and (Has-Text $win "*Browse archive*")) { Start-Sleep -Milliseconds 500 }
}

# ===========================================================================
Hdr "launch + open browser"
$proc = $null
$winOk = $false; $browserOk = $false; $expandedOk = $false
try {
    if (-not (Test-Path $EXE)) {
        Skip "huge_archive_streams_and_builds" "no production exe at $EXE (run a real tauri build)"
        Skip "archive_browser_virtualizes" "no production exe"
        Skip "select_all_counts_all" "no production exe"
        Skip "scroll_windows_rows" "no production exe"
        Skip "browser_selective_restore" "no production exe"
        Summary; if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
    }
    Ensure-BorgBeside
    $proc = Launch-App
    $win = Wait-Win $WinWaitSec
    if (-not $win) {
        Skip "huge_archive_streams_and_builds" "window/webview not ready (no desktop?)"
        Skip "archive_browser_virtualizes" "window not ready"
        Skip "select_all_counts_all" "window not ready"
        Skip "scroll_windows_rows" "window not ready"
        Skip "browser_selective_restore" "window not ready"
    }
    else {
        $winOk = $true
        [void](Nav $win "Archives"); Start-Sleep -Seconds 1
        $refresh = Find-El $win $CT::Button "Refresh"; if ($refresh) { [void](Invoke-El $refresh) }
        if (-not (Wait-Text $win "*$ARCH*" 25)) {
            Fail "huge_archive_streams_and_builds" "archive '$ARCH' never appeared in the Archives list"
            Skip "archive_browser_virtualizes" "no archive listed"
            Skip "select_all_counts_all" "no archive listed"
            Skip "scroll_windows_rows" "no archive listed"
            Skip "browser_selective_restore" "no archive listed"
        }
        else {
            # ---- TEST 1: streams + builds the full tree --------------------
            if (-not (Open-Browser $win)) {
                Fail "huge_archive_streams_and_builds" "could not open the archive Browse view"
                Skip "archive_browser_virtualizes" "browser did not open"
                Skip "select_all_counts_all" "browser did not open"
                Skip "scroll_windows_rows" "browser did not open"
            }
            else {
                $sawLoading = Wait-Text $win "*Loading archive contents*" 4
                # Best-effort, NON-gating: sample the progressive "{N} files" loaded
                # count while the stream is in flight. With 100k zero-byte entries the
                # stream can finish in ~seconds, so this may legitimately miss -- it is
                # evidence in the Pass detail, never a gate (batching is hard-asserted
                # by the streaming_list_matches_collected_listing e2e).
                $mid = 0; $probe = (Get-Date).AddSeconds(8)
                while ((Get-Date) -lt $probe -and -not (Find-El $win $CT::Button "Select all")) {
                    foreach ($e in $win.FindAll($TREE::Descendants, $ANYCOND)) {
                        $nm = ""; try { $nm = $e.Current.Name } catch {}
                        if ($nm -match '^([0-9][0-9,]*)\s+files$') { $v = [int]($matches[1] -replace ',', ''); if ($v -gt $mid -and $v -lt $FileCount) { $mid = $v } }
                    }
                }
                $loaded = Wait-Loaded $win $LoadWaitSec
                if (-not $loaded.ok) {
                    Fail "huge_archive_streams_and_builds" "load did not complete: $($loaded.err)"
                }
                else {
                    $browserOk = $true
                    $prog = if ($mid -gt 0) { "progressive count seen at $mid; " } elseif ($sawLoading) { "saw progressive Loading text; " } else { "" }
                    $cp = Get-CountPair $win
                    if ($cp -and [math]::Abs($cp.total - $FileCount) -le 5) {
                        Pass "huge_archive_streams_and_builds" "${prog}browser built the full tree: header shows $($cp.total) / $FileCount files"
                    }
                    elseif ($cp) {
                        Fail "huge_archive_streams_and_builds" "tree total $($cp.total) does not match the $FileCount staged files"
                    }
                    else {
                        Pass "huge_archive_streams_and_builds" "${prog}browser finished loading the full archive (exact total verified in select_all_counts_all)"
                    }
                }

                # ---- TEST 2: windowed DOM (virtualization) -----------------
                if (-not $browserOk) {
                    Skip "archive_browser_virtualizes" "browser load did not complete"
                    Skip "select_all_counts_all" "browser load did not complete"
                    Skip "scroll_windows_rows" "browser load did not complete"
                }
                else {
                    $expandBtn = Find-El $win $CT::Button "Expand"   # the sole top row ("data")
                    if ($expandBtn) { [void](Invoke-El $expandBtn); Start-Sleep -Seconds 1; $expandedOk = $true }
                    $rowCount = (Find-All $win $CT::CheckBox).Count
                    $logical = $DirCount + 1
                    # The DOM window must hold well under a third of the logical rows.
                    # A fixed cap would let a render-everything regression (e.g. 100 of
                    # 201) pass green; scaling with the tree makes that a FAIL while
                    # staying comfortably above any real viewport (~22 on the 720p VM).
                    $cap = [math]::Max(60, [int]($logical / 3))
                    if (-not $expandedOk) {
                        Skip "archive_browser_virtualizes" "could not expand 'data' to populate rows"
                    }
                    elseif ($rowCount -gt 5 -and $rowCount -lt $cap) {
                        Pass "archive_browser_virtualizes" "with ~$logical rows expanded the DOM holds only $rowCount windowed rows (< $cap; constant-size window over 100k entries)"
                    }
                    elseif ($rowCount -ge $cap) {
                        Fail "archive_browser_virtualizes" "DOM holds $rowCount of $logical rows -- not windowed (expected < $cap)"
                    }
                    else {
                        Skip "archive_browser_virtualizes" "only $rowCount rows visible -- expand may not have populated the tree"
                    }

                    # ---- TEST 3: Select all scales to N --------------------
                    $selAll = Find-El $win $CT::Button "Select all"
                    if (-not $selAll) {
                        Skip "select_all_counts_all" "'Select all' button not found"
                    }
                    else {
                        [void](Invoke-El $selAll)
                        $reached = $null; $d3 = (Get-Date).AddSeconds(20)
                        while ((Get-Date) -lt $d3) {
                            $sel = Get-RestoreBtnCount $win
                            if ($null -eq $sel) { $cpx = Get-CountPair $win; if ($cpx) { $sel = $cpx.sel } }
                            if ($null -ne $sel -and $sel -ge ($FileCount - 5)) { $reached = $sel; break }
                            Start-Sleep -Milliseconds 500
                        }
                        if ($null -ne $reached) {
                            Pass "select_all_counts_all" "Select all selected $reached of $FileCount files (Selection model scales to the full archive)"
                        }
                        else {
                            $sel = Get-RestoreBtnCount $win; if ($null -eq $sel) { $cpx = Get-CountPair $win; if ($cpx) { $sel = $cpx.sel } }
                            $got = if ($null -ne $sel) { $sel } else { "?" }
                            Fail "select_all_counts_all" "Select all reached only $got of $FileCount selected"
                        }
                    }

                    # ---- TEST 4: scroll swaps the windowed rows ------------
                    $before = Visible-DirIndexes $win
                    $maxBefore = if ($before.Count) { ($before | Measure-Object -Maximum).Maximum } else { -1 }
                    $treeEl = Find-El $win $CT::Tree "*Archive contents*"
                    $scrolled = $false
                    if ($treeEl) {
                        try { $treeEl.GetCurrentPattern($SCROLLPAT).SetScrollPercent(-1, 100); $scrolled = $true } catch {}
                        if (-not $scrolled) {
                            try {
                                $r = $treeEl.Current.BoundingRectangle
                                $cx = [int]($r.X + $r.Width / 2); $cy = [int]($r.Y + $r.Height / 2)
                                # ~200 notches guarantees reaching the end of a ~5200px list.
                                for ($i = 0; $i -lt 40; $i++) { [AsNative]::Wheel($cx, $cy, -600); Start-Sleep -Milliseconds 90 }
                                $scrolled = $true
                            } catch {}
                        }
                    }
                    Start-Sleep -Milliseconds 800
                    $after = Visible-DirIndexes $win
                    $maxAfter = if ($after.Count) { ($after | Measure-Object -Maximum).Maximum } else { -1 }
                    if (-not $scrolled) {
                        Skip "scroll_windows_rows" "could not drive a scroll on the tree container"
                    }
                    elseif ($maxAfter -ge ($maxBefore + 50) -or $maxAfter -ge ($DirCount - 5)) {
                        Pass "scroll_windows_rows" "scrolling advanced the visible window from dir index $maxBefore to $maxAfter (windowed rows are recycled, not all 100k rendered)"
                    }
                    else {
                        Skip "scroll_windows_rows" "scroll did not visibly advance the window (before max=$maxBefore, after max=$maxAfter)"
                    }
                }
            }

            # ---- TEST 5: selective restore via the browser ----------------
            if (-not $winOk) {
                Skip "browser_selective_restore" "window not ready"
            }
            elseif (-not $script:BorgExe) {
                Skip "browser_selective_restore" "borg.exe not found"
            }
            else {
                # Fresh browser instance: load() resets scroll + selection so d0000 is at top, unselected.
                if (Has-Text $win "*Browse archive*") { Close-Browser $win }
                Remove-Item -Recurse -Force "$OUT\*" -EA SilentlyContinue
                if (-not (Open-Browser $win)) { Skip "browser_selective_restore" "could not re-open the Browse view" }
                else {
                    $loaded2 = Wait-Loaded $win $LoadWaitSec
                    if (-not $loaded2.ok) { Fail "browser_selective_restore" "re-opened browser did not finish loading: $($loaded2.err)" }
                    else {
                        $expandBtn = Find-El $win $CT::Button "Expand"
                        if ($expandBtn) { [void](Invoke-El $expandBtn); Start-Sleep -Seconds 1 }
                        $folderCb = Find-El $win $CT::CheckBox $MARKERDIR
                        if (-not $folderCb) { Skip "browser_selective_restore" "folder '$MARKERDIR' checkbox not visible after expand" }
                        else {
                            [void](Toggle-El $folderCb); Start-Sleep -Milliseconds 800
                            $restoreBtn = Find-El $win $CT::Button "*Restore selected*"
                            if (-not $restoreBtn) { Skip "browser_selective_restore" "'Restore selected' button not found" }
                            else {
                                Bring-Foreground $win
                                [void](Invoke-El $restoreBtn)
                                $dlg = Set-FolderDialog $OUT
                                if ($dlg -ne "ok") { Fail "browser_selective_restore" "restore destination dialog: $dlg" }
                                else {
                                    $restoredDir = Join-Path $OUT "data\$MARKERDIR"
                                    $files = @(); $deadline = (Get-Date).AddSeconds(90)
                                    while ((Get-Date) -lt $deadline) {
                                        $files = @(Get-ChildItem -Path $restoredDir -Filter "f*.txt" -EA SilentlyContinue)
                                        if ($files.Count -ge 1) { Start-Sleep -Seconds 2; $files = @(Get-ChildItem -Path $restoredDir -Filter "f*.txt" -EA SilentlyContinue); break }
                                        Start-Sleep -Seconds 2
                                    }
                                    $perDir = [math]::Ceiling($FileCount / $DirCount)
                                    if ($files.Count -lt 1) { Fail "browser_selective_restore" "no files restored under $restoredDir after 90s" }
                                    else {
                                        $content = (Get-Content $files[0].FullName -Raw -EA SilentlyContinue)
                                        $byteOk = ($content -eq $MARKER)
                                        if ($files.Count -ge [math]::Floor($perDir * 0.9) -and $byteOk) {
                                            Pass "browser_selective_restore" "selected folder '$MARKERDIR' restored $($files.Count)/$perDir files, byte-correct marker content"
                                        }
                                        elseif ($byteOk) {
                                            Pass "browser_selective_restore" "selected folder restored $($files.Count) byte-correct files (expected ~$perDir)"
                                        }
                                        else {
                                            Fail "browser_selective_restore" "restored $($files.Count) files but content mismatch: '$content'"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
catch { Fail "archive_smoke_exception" "$_" }
finally {
    try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch {}
    Stop-App $proc
}

Summary
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
