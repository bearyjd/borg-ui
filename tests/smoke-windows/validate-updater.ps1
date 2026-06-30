# BorgUI installed-app updater smoke.
#
# Installs an updater-capable baseline NSIS package, launches the installed app
# in the interactive desktop, confirms the published update prompt, clicks
# "Download and install", and verifies the installed executable version changes
# to ExpectedVersion after the updater relaunch.
#
# The original public v0.1.0 installer predates updater support. Build the
# baseline from the updater-capable commit immediately before the v0.2.0 version
# bump (or any later lower-version commit).

param(
    [Parameter(Mandatory = $true)][string]$BaselineInstaller,
    [Parameter(Mandatory = $true)][string]$ExpectedVersion,
    [switch]$InSession1
)

$ErrorActionPreference = "Continue"

if (-not $InSession1) {
    $self = $MyInvocation.MyCommand.Path
    $task = "BorgUI-UpdaterSmoke"
    $log = Join-Path $env:USERPROFILE "updater-smoke.log"
    $done = Join-Path $env:USERPROFILE "updater-smoke.done"
    $result = Join-Path $env:USERPROFILE "updater-smoke-result.json"
    $bat = Join-Path $env:USERPROFILE "updater-smoke.bat"
    Remove-Item $log, $done, $result, $bat -ErrorAction SilentlyContinue
    @(
        "@echo off",
        "powershell -ExecutionPolicy Bypass -File `"$self`" -BaselineInstaller `"$BaselineInstaller`" -ExpectedVersion `"$ExpectedVersion`" -InSession1 > `"$log`" 2>&1",
        "echo DONE > `"$done`""
    ) | Set-Content $bat -Encoding Ascii
    & schtasks.exe /Create /F /TN $task /TR "`"$bat`"" /SC ONCE /ST 23:59 /IT 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "SKIP: could not create interactive updater task"
        Write-Host "Passed: 0  Failed: 0  Skipped: 1"
        exit 0
    }
    & schtasks.exe /Run /TN $task 2>&1 | Out-Null
    $deadline = (Get-Date).AddMinutes(12)
    while ((Get-Date) -lt $deadline -and -not (Test-Path $done)) { Start-Sleep -Seconds 3 }
    if (Test-Path $log) { Get-Content $log }
    & schtasks.exe /Delete /F /TN $task 2>&1 | Out-Null
    Remove-Item $bat -ErrorAction SilentlyContinue
    if (-not (Test-Path $done)) {
        Write-Host "FAIL: interactive updater task timed out"
        Write-Host "Passed: 0  Failed: 1  Skipped: 0"
        exit 1
    }
    if (-not (Test-Path $result)) { exit 1 }
    $data = Get-Content $result -Raw | ConvertFrom-Json
    if ($data.Failed -gt 0) { exit 1 }
    exit 0
}

$script:Passed = 0
$script:Failed = 0
$script:Skipped = 0
function Pass($name, $detail) { $script:Passed++; Write-Host "PASS: $name"; if ($detail) { Write-Host "      $detail" } }
function Fail($name, $detail) { $script:Failed++; Write-Host "FAIL: $name"; if ($detail) { Write-Host "      $detail" } }
function Finish {
    $summary = @{ Passed = $script:Passed; Failed = $script:Failed; Skipped = $script:Skipped }
    $summary | ConvertTo-Json | Set-Content (Join-Path $env:USERPROFILE "updater-smoke-result.json") -Encoding UTF8
    Write-Host "Passed: $script:Passed  Failed: $script:Failed  Skipped: $script:Skipped"
}

function Find-InstallDir {
    foreach ($dir in @(
        (Join-Path $env:LOCALAPPDATA "BorgUI"),
        (Join-Path $env:LOCALAPPDATA "Programs\BorgUI"),
        (Join-Path $env:ProgramFiles "BorgUI")
    )) {
        if ($dir -and (Test-Path (Join-Path $dir "borg-ui.exe"))) { return $dir }
    }
    return $null
}

function File-Version($path) {
    $raw = (Get-Item $path).VersionInfo.ProductVersion
    if (-not $raw) { $raw = (Get-Item $path).VersionInfo.FileVersion }
    $clean = ([string]$raw).Split("+")[0]
    return ([version]$clean).ToString(3)
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$UIA = [System.Windows.Automation.AutomationElement]
$Tree = [System.Windows.Automation.TreeScope]
$Control = [System.Windows.Automation.ControlType]
$Invoke = [System.Windows.Automation.InvokePattern]::Pattern
function Type-Condition($type) {
    return New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $type)
}
function Find-ByName($root, $type, $pattern) {
    foreach ($element in $root.FindAll($Tree::Descendants, (Type-Condition $type))) {
        $name = ""; try { $name = $element.Current.Name } catch {}
        if ($name -like $pattern) { return $element }
    }
    return $null
}
function Find-Window($pattern, $seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($window in $UIA::RootElement.FindAll($Tree::Children, (Type-Condition $Control::Window))) {
            $name = ""; try { $name = $window.Current.Name } catch {}
            if ($name -like $pattern) { return $window }
        }
        Start-Sleep -Milliseconds 750
    }
    return $null
}

Get-Process borg-ui -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$oldDir = Find-InstallDir
if ($oldDir) {
    $uninstall = Join-Path $oldDir "uninstall.exe"
    if (Test-Path $uninstall) { Start-Process $uninstall -ArgumentList "/S" -Wait }
}

$install = Start-Process $BaselineInstaller -ArgumentList "/S" -PassThru -Wait
Start-Sleep -Seconds 3
$dir = Find-InstallDir
if (-not $dir) {
    Fail "baseline_install" "installer exit=$($install.ExitCode); borg-ui.exe not found"
    Finish
    exit 1
}
$exe = Join-Path $dir "borg-ui.exe"
$before = File-Version $exe
if ([version]$before -ge [version]$ExpectedVersion) {
    Fail "baseline_version" "baseline $before must be older than $ExpectedVersion"
    Finish
    exit 1
}
Pass "baseline_install" "installed version $before at $dir"

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--force-renderer-accessibility"
$process = Start-Process $exe -PassThru
$window = Find-Window "*BorgUI*" 45
if (-not $window) {
    Fail "update_prompt" "BorgUI window did not become accessible"
    Finish
    exit 1
}

$deadline = (Get-Date).AddSeconds(60)
$button = $null
while ((Get-Date) -lt $deadline -and -not $button) {
    $button = Find-ByName $window $Control::Button "Download and install"
    if (-not $button) { Start-Sleep -Seconds 1 }
}
if (-not $button) {
    Fail "update_prompt" "no consent prompt for $ExpectedVersion"
    Finish
    exit 1
}
Pass "update_prompt" "published update offered and requires confirmation"

try {
    $button.GetCurrentPattern($Invoke).Invoke()
} catch {
    Fail "update_install" "could not invoke Download and install: $_"
    Finish
    exit 1
}

$deadline = (Get-Date).AddMinutes(8)
$updated = $false
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 3
    if ((Test-Path $exe) -and (File-Version $exe) -eq $ExpectedVersion) {
        $updated = $true
        break
    }
}
if ($updated) {
    Pass "update_install" "installed executable is version $ExpectedVersion"
} else {
    Fail "update_install" "installed executable did not reach version $ExpectedVersion"
}

Get-Process borg-ui -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Finish
if ($script:Failed -gt 0) { exit 1 }
