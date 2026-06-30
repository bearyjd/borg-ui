$ErrorActionPreference = 'Stop'
$validator = Join-Path $PSScriptRoot 'validate-signing-config.ps1'

function Expect-Success([string]$Name, [scriptblock]$Action) {
    try {
        & $Action
        Write-Host "PASS: $Name"
    } catch {
        throw "Expected success for '$Name': $_"
    }
}

function Expect-Failure([string]$Name, [string]$Pattern, [scriptblock]$Action) {
    try {
        & $Action
    } catch {
        if ("$_" -notmatch $Pattern) {
            throw "Failure for '$Name' did not match '$Pattern': $_"
        }
        Write-Host "PASS: $Name"
        return
    }
    throw "Expected failure for '$Name'."
}

$valid = @{
    SigningEnabled = $true
    ClientId = '11111111-1111-1111-1111-111111111111'
    TenantId = '22222222-2222-2222-2222-222222222222'
    SubscriptionId = '33333333-3333-3333-3333-333333333333'
    Endpoint = 'https://eus.codesigning.azure.net/'
    Account = 'borgui-signing'
    Profile = 'public-trust'
}

Expect-Success 'disabled configuration needs no Azure values' {
    & $validator -SigningEnabled $false
}
Expect-Success 'valid configuration' {
    & $validator @valid
}
Expect-Failure 'missing value' 'missing:.*SIGNING_PROFILE' {
    $case = $valid.Clone()
    $case.Profile = ''
    & $validator @case
}
Expect-Failure 'invalid client id' 'AZURE_CLIENT_ID must be a GUID' {
    $case = $valid.Clone()
    $case.ClientId = 'not-a-guid'
    & $validator @case
}
Expect-Failure 'non-HTTPS endpoint' 'absolute HTTPS URL' {
    $case = $valid.Clone()
    $case.Endpoint = 'http://example.test/'
    & $validator @case
}
Expect-Failure 'invalid account name' 'SIGNING_ACCOUNT contains invalid' {
    $case = $valid.Clone()
    $case.Account = 'bad account'
    & $validator @case
}
