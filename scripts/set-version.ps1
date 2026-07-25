[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$')]
    [string]$Version
)

$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$packagePath = Join-Path $workspace "package.json"
$packageLockPath = Join-Path $workspace "package-lock.json"
$tauriConfigPath = Join-Path $workspace "src-tauri\tauri.conf.json"
$cargoPath = Join-Path $workspace "src-tauri\Cargo.toml"
$readmePaths = @(
    (Join-Path $workspace "README.md"),
    (Join-Path $workspace "README.zh-CN.md")
)
$versionJsonScript = Join-Path $PSScriptRoot "version-json.mjs"

Push-Location $workspace
try {
    & node $versionJsonScript "set" $packagePath $Version
    if ($LASTEXITCODE -ne 0) {
        throw "Updating package.json failed."
    }

    & node $versionJsonScript "set" $tauriConfigPath $Version
    if ($LASTEXITCODE -ne 0) {
        throw "Updating src-tauri/tauri.conf.json failed."
    }

    & node $versionJsonScript "set-lock" $packageLockPath $Version
    if ($LASTEXITCODE -ne 0) {
        throw "Updating package-lock.json failed."
    }

    foreach ($readmePath in $readmePaths) {
        & node $versionJsonScript "set-readme" $readmePath $Version
        if ($LASTEXITCODE -ne 0) {
            throw "Updating the version badge in $readmePath failed."
        }
    }

    $cargoText = [System.IO.File]::ReadAllText($cargoPath)
    $cargoVersionPattern = '(?ms)(^\[package\]\s*(?:(?!^\[).)*?^version\s*=\s*")[^"]+(")'
    $cargoVersionMatch = [regex]::Match($cargoText, $cargoVersionPattern)
    if (-not $cargoVersionMatch.Success) {
        throw "Could not update the package version in src-tauri/Cargo.toml."
    }
    $cargoReplacement = '${1}' + $Version + '${2}'
    $updatedCargoText = [regex]::Replace(
        $cargoText,
        $cargoVersionPattern,
        $cargoReplacement,
        1
    )
    [System.IO.File]::WriteAllText(
        $cargoPath,
        $updatedCargoText,
        [System.Text.UTF8Encoding]::new($false)
    )

    $verifiedVersion = & (Join-Path $PSScriptRoot "verify-version.ps1")
    if ($verifiedVersion -ne $Version) {
        throw "Version verification returned $verifiedVersion, expected $Version."
    }

    Write-Host "Nota version is now $Version."
}
finally {
    Pop-Location
}
