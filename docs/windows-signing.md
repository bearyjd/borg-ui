# Windows Authenticode signing

The release workflow supports Azure Artifact Signing through GitHub OIDC. It is
disabled by default, so ordinary builds and workflow dry runs continue to
produce artifacts named `borgui-windows-installers-unsigned`.

To enable signing, configure the Azure federated identity for this repository,
grant it the **Artifact Signing Certificate Profile Signer** role, and set:

- GitHub Actions secrets: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`,
  `AZURE_SUBSCRIPTION_ID`
- GitHub Actions variables: `AZURE_SIGNING_ENDPOINT`,
  `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE`
- GitHub Actions variable `ENABLE_CODE_SIGNING=true` for normal releases

For a one-off validation, dispatch the Release workflow with
`enable_signing=true`. Explicitly enabled runs fail before building if any
configuration is missing or malformed. They also fail on Azure login, signing,
timestamping, or post-signature verification errors.

Before enabling the repository variable, validate the intended values locally
without printing secrets:

```powershell
$env:AZURE_CLIENT_ID = '<application/client GUID>'
$env:AZURE_TENANT_ID = '<tenant GUID>'
$env:AZURE_SUBSCRIPTION_ID = '<subscription GUID>'
$env:SIGNING_ENDPOINT = 'https://<region>.codesigning.azure.net/'
$env:SIGNING_ACCOUNT = '<artifact-signing-account>'
$env:SIGNING_PROFILE = '<certificate-profile>'
.\scripts\validate-signing-config.ps1 -SigningEnabled $true
```

The validator checks required values, Azure GUID syntax, an absolute HTTPS
endpoint, and safe account/profile names. It never prints the client, tenant, or
subscription IDs.

The workflow signs the application executable before bundling, signs both MSI
and NSIS installers, verifies every Authenticode signature before upload, and
then regenerates the Tauri updater signatures because Authenticode changes the
installer bytes. Tauri updater signing and Authenticode use separate keys and
trust systems.
