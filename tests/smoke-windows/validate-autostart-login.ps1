# BorgUI autostart login-cycle validation: prove the HKCU \Run value the app
# registers actually LAUNCHES the app (minimized, in the interactive session)
# after a real reboot + auto-login. The reg add/query/delete round-trip and the
# --minimized -> hidden-in-tray behaviour are validated separately
# (validate.ps1::autostart_registry_roundtrip + PR #33 item 2); the one piece
# never exercised on real hardware is the login actually FIRING the Run key.
#
# Three phases, driven by run.sh across a guest reboot (a single script can't
# survive the reboot):
#   -Phase set    : register the Run value EXACTLY as borg-platform-win::
#                   autostart::enable writes it (value "BorgUI" =
#                   "<exe>" --minimized), after killing any running instance so a
#                   post-reboot instance is unambiguously login-fired.
#   -Phase verify : after the reboot + auto-login, confirm borg-ui.exe is running
#                   in an INTERACTIVE session (>=1), launched with --minimized.
#                   Gated on the interactive desktop (explorer.exe) actually being
#                   up -- "no desktop" is an environment SKIP, not an autostart
#                   FAIL. Always cleans up (restore any prior value, kill the app).
#   -Phase clean  : remove our Run value / restore any prior one / kill the app.
#                   run.sh invokes this on a mid-run abort so a failed run never
#                   leaves the test exe auto-launching on the warm VM.
#
# Pass/Fail/Skip + JSON + a VERIFY-COMPLETE marker + exit code. run.sh treats a
# confirmed PASS (>=1 pass, 0 fail) as success, a SKIP as "not a confirmed pass",
# and anything else as failure. ASCII only (PS 5.1 reads UTF-8-no-BOM as ANSI --
# a non-ASCII byte breaks parsing).

param([ValidateSet("set", "verify", "clean")][string]$Phase = "set", [int]$WaitSec = 180, [int]$ExplorerWaitSec = 120)
$ErrorActionPreference = "Continue"

$EXE = "C:\borgui-test\target\release\borg-ui.exe"
$REGPATH = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$VALUE = "BorgUI"   # matches borg-platform-win::autostart::AUTOSTART_VALUE
$BAKFILE = Join-Path $env:USERPROFILE "autostart-login.bak"
$RESJSON = Join-Path $env:USERPROFILE "autostart-login-results.json"

function Ensure-BorgBeside {
    # The app starts (window + tray) without borg -- borg is only used lazily on an
    # operation -- so this is best-effort; warn rather than swallow so a staging
    # failure can't be silently misread later.
    $borg = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName
    if (-not $borg) { return }
    $ad = Split-Path $EXE -Parent; $bd = Split-Path $borg -Parent
    if ($bd -ine $ad -and -not (Test-Path (Join-Path $ad "_internal"))) {
        try { Copy-Item (Join-Path $bd "*") $ad -Recurse -Force }
        catch { Write-Host "WARN: Ensure-BorgBeside copy failed: $($_.Exception.Message)" }
    }
}

# Remove our Run value (restoring any prior one, byte-exact) and kill the app.
# Idempotent: safe to call from verify (normal end) or clean (abort path).
function Remove-Autostart {
    Remove-ItemProperty -Path $REGPATH -Name $VALUE -EA SilentlyContinue
    if (Test-Path $BAKFILE) {
        $orig = Get-Content $BAKFILE -Raw -EA SilentlyContinue
        if ($null -ne $orig -and $orig.Length -gt 0) {
            New-ItemProperty -Path $REGPATH -Name $VALUE -Value $orig -PropertyType String -Force | Out-Null
        }
        Remove-Item $BAKFILE -EA SilentlyContinue
    }
    Get-Process borg-ui -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
}

# ----------------------------------------------------------------------------
# PHASE: clean -- abort-path cleanup invoked by run.sh; no verdict.
# ----------------------------------------------------------------------------
if ($Phase -eq "clean") {
    Remove-Autostart
    Write-Host "CLEAN-OK: removed Run value '$VALUE' and any leftover app"
    Write-Host "Failed: 0"
    exit 0
}

# ----------------------------------------------------------------------------
# PHASE: set -- register the Run value the way the app does, then let run.sh
# reboot the guest.
# ----------------------------------------------------------------------------
if ($Phase -eq "set") {
    if (-not (Test-Path $EXE)) { Write-Host "SKIP: no production exe at $EXE (run a real tauri build)"; Write-Host "Failed: 0"; exit 0 }
    Ensure-BorgBeside
    # Kill any running instance so that ANY borg-ui.exe seen after the reboot can
    # only have come from the Run key firing at login.
    Get-Process borg-ui -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue

    # Back up an existing value (test VM should have none) so cleanup can restore it
    # byte-exact (-NoNewline so the bak file carries no trailing newline).
    $existing = $null
    try { $existing = (Get-ItemProperty -Path $REGPATH -Name $VALUE -EA Stop).$VALUE } catch {}
    if ($null -ne $existing) { Set-Content -Path $BAKFILE -Value $existing -NoNewline -Encoding Ascii } else { Remove-Item $BAKFILE -EA SilentlyContinue }

    # The exact REG_SZ data autostart::enable stores: format!("\"{exe}\" --minimized").
    # Written via the registry provider so the embedded quotes can't be mangled by
    # native-command argument passing; the resulting value is byte-identical to the
    # app's `reg add ... /D "\"<exe>\" --minimized"`.
    $data = '"' + $EXE + '" --minimized'
    New-ItemProperty -Path $REGPATH -Name $VALUE -Value $data -PropertyType String -Force | Out-Null
    $readback = (Get-ItemProperty -Path $REGPATH -Name $VALUE -EA SilentlyContinue).$VALUE
    if ($readback -ne $data) { Write-Host "SKIP: Run value did not persist as expected: '$readback'"; Write-Host "Failed: 0"; exit 0 }
    Write-Host "SET-OK: $VALUE = $readback"
    Write-Host "Failed: 0"
    exit 0
}

# ----------------------------------------------------------------------------
# PHASE: verify -- the guest has rebooted + auto-logged in; confirm the Run key
# fired, then clean up.
# ----------------------------------------------------------------------------
$script:Passed = 0; $script:Failed = 0; $script:Skipped = 0; $script:Results = @()
function Res($n, $s, $d) {
    if ($s -eq "PASS") { $script:Passed++ } elseif ($s -eq "FAIL") { $script:Failed++ } else { $script:Skipped++ }
    $script:Results += @{ Name = $n; Status = $s; Detail = $d }
    Write-Host "  ${s}: $n"; if ($d) { Write-Host "        $d" }
}

Write-Host "--- AUTOSTART-LOGIN: verify ---"

# Gate on the interactive desktop being up. The Run key is processed by the user's
# shell at interactive login; sshd can answer before the desktop finishes logging
# in. If Explorer never comes up, that is an environment-not-ready condition (e.g.
# auto-login didn't happen) -- a SKIP, NOT an autostart FAIL.
$explorer = $null; $edl = (Get-Date).AddSeconds($ExplorerWaitSec)
while ((Get-Date) -lt $edl) {
    $explorer = Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" -EA SilentlyContinue | Select-Object -First 1
    if ($explorer) { break }
    Start-Sleep -Seconds 3
}

if (-not $explorer) {
    Res "autostart_login_fires" "SKIP" "no interactive desktop (explorer.exe) came up within ${ExplorerWaitSec}s after the reboot -- environment not ready, cannot judge the Run key"
}
else {
    # Poll for the login-fired process (Explorer may still be processing Run keys).
    $proc = $null; $deadline = (Get-Date).AddSeconds($WaitSec)
    while ((Get-Date) -lt $deadline) {
        $proc = Get-CimInstance Win32_Process -Filter "Name='borg-ui.exe'" -EA SilentlyContinue | Select-Object -First 1
        if ($proc) { break }
        Start-Sleep -Seconds 3
    }
    if (-not $proc) {
        Res "autostart_login_fires" "FAIL" "no borg-ui.exe running ${WaitSec}s after the reboot + auto-login -- the Run key did not launch the app"
    }
    else {
        $cmd = "$($proc.CommandLine)"
        $procPid = [int]$proc.ProcessId
        $sid = $proc.SessionId   # Win32_Process exposes the session directly
        $ppid = [int]$proc.ParentProcessId
        $parent = (Get-Process -Id $ppid -EA SilentlyContinue).ProcessName
        if (-not $parent) { $parent = "(exited pid $ppid)" }
        $hasMin = ($cmd -match '--minimized')
        $interactive = ($null -ne $sid -and [int]$sid -ge 1)
        if ($hasMin -and $interactive) {
            # Explorer-parent is the textbook Run-key signature, but Explorer often
            # re-parents / may have churned by now, so report it without gating on it.
            $pnote = if ($parent -ieq 'explorer') { "by the shell (explorer)" } else { "parent '$parent' (not explorer; attribution indirect)" }
            Res "autostart_login_fires" "PASS" "borg-ui.exe (pid $procPid) auto-started after login in interactive session $sid via the Run key, launched --minimized $pnote. cmd: $cmd"
        }
        elseif (-not $hasMin) {
            Res "autostart_login_fires" "FAIL" "borg-ui.exe is running but not via the expected --minimized Run-key command: '$cmd'"
        }
        else {
            # --minimized but NOT an interactive session -> cannot attribute to an
            # interactive-login Run key (the whole point of this test). Not a pass.
            Res "autostart_login_fires" "FAIL" "borg-ui.exe started with --minimized but in non-interactive session '$sid' (parent '$parent') -- cannot attribute to an interactive-login Run key"
        }
    }
}

# Always clean up, whatever the verdict.
Remove-Autostart

Write-Host "`n========================================"
Write-Host "  AUTOSTART LOGIN VALIDATION RESULTS"
Write-Host "  Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
Write-Host "========================================"
# Positive completion marker: lets run.sh require "the verify script ran to the end
# AND saw a real pass", instead of merely "no Failed: 1 anywhere in the output".
Write-Host "VERIFY-COMPLETE Passed: $script:Passed Failed: $script:Failed Skipped: $script:Skipped"
$script:Results | ConvertTo-Json -Depth 3 | Out-File $RESJSON -Encoding UTF8
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
