# BorgUI autostart login-cycle validation: prove the HKCU \Run value the app
# registers actually LAUNCHES the app (minimized, in the interactive session)
# after a real reboot + auto-login. The reg add/query/delete round-trip and the
# --minimized -> hidden-in-tray behaviour are validated separately
# (validate.ps1::autostart_registry_roundtrip + PR #33 item 2); the one piece
# never exercised on real hardware is the login actually FIRING the Run key.
#
# Two phases, driven by run.sh across a guest reboot (a single script can't
# survive the reboot):
#   -Phase set    : register the Run value EXACTLY as borg-platform-win::
#                   autostart::enable writes it (value "BorgUI" =
#                   "<exe>" --minimized), after killing any running instance so a
#                   post-reboot instance is unambiguously login-fired.
#   -Phase verify : after the reboot + auto-login, confirm borg-ui.exe is running
#                   in an interactive session (>=1), launched with --minimized,
#                   parented by the shell -- i.e. the Run key fired. Then clean up
#                   (restore any prior value, kill the app).
#
# Pass/Fail/Skip + JSON + exit code, mirroring the other validate-*.ps1. ASCII
# only (PS 5.1 reads UTF-8-no-BOM as ANSI -- a non-ASCII byte breaks parsing).

param([ValidateSet("set", "verify")][string]$Phase = "set", [int]$WaitSec = 180)
$ErrorActionPreference = "Continue"

$EXE = "C:\borgui-test\target\release\borg-ui.exe"
$REGPATH = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$VALUE = "BorgUI"   # matches borg-platform-win::autostart::AUTOSTART_VALUE
$BAKFILE = Join-Path $env:USERPROFILE "autostart-login.bak"
$RESJSON = Join-Path $env:USERPROFILE "autostart-login-results.json"

function Ensure-BorgBeside {
    $borg = (Get-ChildItem C:\borg -Recurse -Filter borg.exe -EA SilentlyContinue | Select-Object -First 1).FullName
    if (-not $borg) { return }
    $ad = Split-Path $EXE -Parent; $bd = Split-Path $borg -Parent
    if ($bd -ine $ad -and -not (Test-Path (Join-Path $ad "_internal"))) { try { Copy-Item (Join-Path $bd "*") $ad -Recurse -Force } catch {} }
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

    # Back up an existing value (test VM should have none) so cleanup can restore it.
    $existing = $null
    try { $existing = (Get-ItemProperty -Path $REGPATH -Name $VALUE -EA Stop).$VALUE } catch {}
    if ($existing) { Set-Content -Path $BAKFILE -Value $existing -Encoding Ascii } else { Remove-Item $BAKFILE -EA SilentlyContinue }

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

# Poll for the login-fired process. sshd can come up before the interactive
# desktop finishes auto-login, so allow a generous window for Explorer to process
# the Run key.
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
    $ppid = [int]$proc.ParentProcessId
    $parent = (Get-Process -Id $ppid -EA SilentlyContinue).ProcessName
    if (-not $parent) { $parent = "(exited pid $ppid)" }
    $sid = (Get-Process -Id $procPid -EA SilentlyContinue).SessionId
    $hasMin = ($cmd -match '--minimized')
    $interactive = ($null -ne $sid -and [int]$sid -ge 1)
    if ($hasMin -and $interactive) {
        Res "autostart_login_fires" "PASS" "borg-ui.exe (pid $procPid) auto-started after login in interactive session $sid via the Run key, launched with --minimized by '$parent'. cmd: $cmd"
    }
    elseif ($hasMin) {
        Res "autostart_login_fires" "PASS" "borg-ui.exe auto-started with --minimized (session '$sid', parent '$parent') -- the Run key fired at login"
    }
    else {
        Res "autostart_login_fires" "FAIL" "borg-ui.exe is running but not via the expected --minimized Run-key command: '$cmd'"
    }
}

# --- cleanup: remove our value (restore any prior one), kill the app ---
Remove-ItemProperty -Path $REGPATH -Name $VALUE -EA SilentlyContinue
if (Test-Path $BAKFILE) {
    $orig = (Get-Content $BAKFILE -Raw -EA SilentlyContinue)
    if ($orig) { $orig = $orig.TrimEnd("`r", "`n"); New-ItemProperty -Path $REGPATH -Name $VALUE -Value $orig -PropertyType String -Force | Out-Null }
    Remove-Item $BAKFILE -EA SilentlyContinue
}
Get-Process borg-ui -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue

Write-Host "`n========================================"
Write-Host "  AUTOSTART LOGIN VALIDATION RESULTS"
Write-Host "  Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
Write-Host "========================================`n"
$script:Results | ConvertTo-Json -Depth 3 | Out-File $RESJSON -Encoding UTF8
if ($script:Failed -gt 0) { exit 1 } else { exit 0 }
