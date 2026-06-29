param(
    [Parameter(Mandatory = $true)]
    [string[]]$Paths,
    [Parameter(Mandatory = $true)]
    [bool]$ExpectSigned
)

$ErrorActionPreference = 'Stop'

foreach ($path in $Paths) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "signature verification target does not exist: $path"
    }

    $signature = Get-AuthenticodeSignature -LiteralPath $path
    if ($ExpectSigned) {
        if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
            throw "invalid or missing Authenticode signature on $path`: $($signature.StatusMessage)"
        }
        if (-not $signature.SignerCertificate) {
            throw "valid signature on $path did not include a signer certificate"
        }
        Write-Host "Valid Authenticode signature: $path"
    } else {
        if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::NotSigned) {
            throw "unsigned build unexpectedly has signature status $($signature.Status) on $path"
        }
        Write-Host "Unsigned artifact (expected): $path"
    }
}
