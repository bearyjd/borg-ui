param(
    [bool]$SigningEnabled = ($env:SIGNING_ENABLED -eq 'true'),
    [string]$ClientId = $env:AZURE_CLIENT_ID,
    [string]$TenantId = $env:AZURE_TENANT_ID,
    [string]$SubscriptionId = $env:AZURE_SUBSCRIPTION_ID,
    [string]$Endpoint = $env:SIGNING_ENDPOINT,
    [string]$Account = $env:SIGNING_ACCOUNT,
    [string]$Profile = $env:SIGNING_PROFILE
)

$ErrorActionPreference = 'Stop'

if (-not $SigningEnabled) {
    Write-Host 'Authenticode signing is disabled; Azure configuration is not required.'
    return
}

$values = @{
    AZURE_CLIENT_ID = $ClientId
    AZURE_TENANT_ID = $TenantId
    AZURE_SUBSCRIPTION_ID = $SubscriptionId
    SIGNING_ENDPOINT = $Endpoint
    SIGNING_ACCOUNT = $Account
    SIGNING_PROFILE = $Profile
}
$missing = @($values.Keys | Where-Object { [string]::IsNullOrWhiteSpace($values[$_]) } | Sort-Object)
if ($missing.Count -gt 0) {
    throw "Code signing was explicitly enabled, but configuration is missing: $($missing -join ', ')"
}

$guidValues = @{
    AZURE_CLIENT_ID = $ClientId
    AZURE_TENANT_ID = $TenantId
    AZURE_SUBSCRIPTION_ID = $SubscriptionId
}
foreach ($entry in $guidValues.GetEnumerator()) {
    $parsed = [guid]::Empty
    if (-not [guid]::TryParse($entry.Value, [ref]$parsed)) {
        throw "$($entry.Key) must be a GUID."
    }
}

$endpointUri = $null
if (-not [uri]::TryCreate($Endpoint, [System.UriKind]::Absolute, [ref]$endpointUri) -or
    $endpointUri.Scheme -ne 'https' -or
    [string]::IsNullOrWhiteSpace($endpointUri.Host)) {
    throw 'SIGNING_ENDPOINT must be an absolute HTTPS URL.'
}

$nameValues = @{ SIGNING_ACCOUNT = $Account; SIGNING_PROFILE = $Profile }
foreach ($entry in $nameValues.GetEnumerator()) {
    if ($entry.Value -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{1,126}[A-Za-z0-9]$') {
        throw "$($entry.Key) contains invalid characters or has an invalid length."
    }
}

Write-Host "Azure Artifact Signing configuration is valid for account '$Account' and profile '$Profile'."
