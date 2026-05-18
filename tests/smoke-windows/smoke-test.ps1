param(
    [string]$SourceDir = "C:\borgui-test"
)

$ErrorActionPreference = "Continue"
$script:Passed = 0
$script:Failed = 0
$script:Results = @()

function Write-TestHeader($name) {
    Write-Host "`n--- TEST: $name ---" -ForegroundColor Cyan
}

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

# ------------------------------------------------------------------
# Test 1: Rust toolchain available
# ------------------------------------------------------------------
Write-TestHeader "rust_toolchain"

$env:PATH = "C:\mingw64\bin;$env:USERPROFILE\.cargo\bin;$env:PATH"
$rustc = & rustc --version 2>&1
if ($rustc -match "rustc") {
    Pass "rust_toolchain" "$rustc"
} else {
    Fail "rust_toolchain" "rustc not found"
}

# ------------------------------------------------------------------
# Test 2: Source code present
# ------------------------------------------------------------------
Write-TestHeader "source_present"

if (Test-Path "$SourceDir\Cargo.toml") {
    Pass "source_present" "Workspace Cargo.toml found at $SourceDir"
} else {
    Fail "source_present" "No Cargo.toml at $SourceDir"
    Write-Host "`nCannot continue without source." -ForegroundColor Red
    exit 1
}

# ------------------------------------------------------------------
# Test 3: borg-core tests pass on Windows
# ------------------------------------------------------------------
Write-TestHeader "borg_core_tests"

$output = & cargo test -p borg-core --manifest-path "$SourceDir\Cargo.toml" 2>&1 | Out-String
if ($output -match "test result: ok\. (\d+) passed") {
    $count = $Matches[1]
    Pass "borg_core_tests" "$count tests passed"
} else {
    Fail "borg_core_tests" "Tests failed or did not run"
    Write-Host $output
}

# ------------------------------------------------------------------
# Test 4: borg-platform-win tests pass on Windows
# ------------------------------------------------------------------
Write-TestHeader "borg_platform_win_tests"

$output = & cargo test -p borg-platform-win --manifest-path "$SourceDir\Cargo.toml" 2>&1 | Out-String
if ($output -match "test result: ok\. (\d+) passed") {
    $count = $Matches[1]
    Pass "borg_platform_win_tests" "$count tests passed"
} else {
    Fail "borg_platform_win_tests" "Tests failed or did not run"
    Write-Host $output
}

# ------------------------------------------------------------------
# Test 5: borg-core builds in release mode
# ------------------------------------------------------------------
Write-TestHeader "release_build"

$output = & cargo build --release -p borg-core -p borg-platform-win --manifest-path "$SourceDir\Cargo.toml" 2>&1 | Out-String
if ($output -match "Finished|Compiling borg") {
    Pass "release_build" "Release build succeeded"
} else {
    Fail "release_build" "Release build failed"
    Write-Host $output
}

# ------------------------------------------------------------------
# Test 6: WebView2 runtime available
# ------------------------------------------------------------------
Write-TestHeader "webview2_available"

$wv2Paths = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BEF-A3BE4B6AF2AC}",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BEF-A3BE4B6AF2AC}",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BEF-A3BE4B6AF2AC}"
)
$found = $false
foreach ($p in $wv2Paths) {
    $val = Get-ItemProperty $p -ErrorAction SilentlyContinue
    if ($val) { Pass "webview2_available" "WebView2 found at $p"; $found = $true; break }
}
$wv2Bin = Get-ChildItem "C:\Program Files*\Microsoft\EdgeWebView" -Recurse -Filter "msedgewebview2.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$found -and $wv2Bin) {
    Pass "webview2_available" "WebView2 binary at $($wv2Bin.FullName)"
} elseif (!$found) {
    Fail "webview2_available" "WebView2 not found"
}

# ------------------------------------------------------------------
# Test 7: Windows environment sanity
# ------------------------------------------------------------------
Write-TestHeader "windows_env"

$os = [System.Environment]::OSVersion.VersionString
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "AMD64") {
    Pass "windows_env" "$os ($arch)"
} else {
    Fail "windows_env" "Unexpected architecture: $arch"
}

# ------------------------------------------------------------------
# Summary
# ------------------------------------------------------------------
Write-Host "`n========================================" -ForegroundColor White
Write-Host "  SMOKE TEST RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Passed: $script:Passed" -ForegroundColor Green
Write-Host "  Failed: $script:Failed" -ForegroundColor $(if ($script:Failed -gt 0) { "Red" } else { "Green" })
Write-Host "  Total:  $($script:Passed + $script:Failed)" -ForegroundColor White
Write-Host "========================================`n" -ForegroundColor White

$jsonResults = $script:Results | ConvertTo-Json -Depth 3
$resultsPath = "$SourceDir\smoke-results.json"
$jsonResults | Out-File -FilePath $resultsPath -Encoding UTF8
Write-Host "Results written to $resultsPath"

if ($script:Failed -gt 0) {
    exit 1
} else {
    exit 0
}
