# Shared result helpers for the Windows smoke scripts.
#
# Dot-sourced (NOT invoked with &) by each script:  . "$PSScriptRoot\_common.ps1"
# Dot-sourcing runs this file in the CALLER's scope, so `$script:Passed` below
# binds to the sourcing script exactly as if these lines were inline. Calling it
# with `&` would give the helpers their own scope and every counter would stay 0.
#
# run.sh uploads this file next to each script it runs (see push_ps1), so
# $PSScriptRoot resolves to the same directory on the guest.
#
# ASCII-only. PowerShell 5.1 reads a UTF-8 file without a BOM as ANSI, so one
# non-ASCII byte here breaks parsing for every script that sources it.
#
# The output text is a contract, not cosmetics: run.sh greps the summary for
# `Skipped:\s*\d+` (report_skips, the #126 loud-skip warning) and callers grep
# for `Failed: 0`. Changing the "  PASS: " / "        " shapes or the counter
# names breaks that silently.

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

# Every Skip MUST increment $script:Skipped. validate-vss-spike.ps1 previously
# defined a Skip that only appended to $Results, so its summary always reported
# `Skipped: 0` and run.sh's loud-skip warning could never fire for it -- the
# precise "a permanently-skipping check looks identical to a passing one"
# failure that #126 exists to prevent.
function Skip($name, $detail) {
    $script:Skipped++; $script:Results += @{ Name = $name; Status = "SKIP"; Detail = $detail }
    Write-Host "  SKIP: $name" -ForegroundColor Yellow
    if ($detail) { Write-Host "        $detail" -ForegroundColor DarkGray }
}
