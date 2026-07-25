[CmdletBinding()]
param(
    [string]$Tag
)

$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$packagePath = Join-Path $workspace "package.json"
$packageLockPath = Join-Path $workspace "package-lock.json"
$cargoPath = Join-Path $workspace "src-tauri\Cargo.toml"
$tauriConfigPath = Join-Path $workspace "src-tauri\tauri.conf.json"
$readmePath = Join-Path $workspace "README.md"
$chineseReadmePath = Join-Path $workspace "README.zh-CN.md"

$package = [System.IO.File]::ReadAllText($packagePath) | ConvertFrom-Json
$tauriConfig = [System.IO.File]::ReadAllText($tauriConfigPath) | ConvertFrom-Json
$cargoText = [System.IO.File]::ReadAllText($cargoPath)

$versionJsonScript = Join-Path $PSScriptRoot "version-json.mjs"
$lockVersionsJson = & node $versionJsonScript "read-lock" $packageLockPath
if ($LASTEXITCODE -ne 0) {
    throw "Could not read versions from package-lock.json."
}
$lockVersions = $lockVersionsJson | ConvertFrom-Json

$readmeText = [System.IO.File]::ReadAllText($readmePath)
$chineseReadmeText = [System.IO.File]::ReadAllText($chineseReadmePath)
$readmeVersionMatch = [regex]::Match(
    $readmeText,
    'badge/version-(.+?)-56615D\?style'
)
$chineseReadmeVersionMatch = [regex]::Match(
    $chineseReadmeText,
    'badge/version-(.+?)-56615D\?style'
)
if (-not $readmeVersionMatch.Success -or -not $chineseReadmeVersionMatch.Success) {
    throw "Could not read the version badge from both README files."
}

$cargoMatch = [regex]::Match(
    $cargoText,
    '(?ms)^\[package\]\s*(?:(?!^\[).)*?^version\s*=\s*"([^"]+)"'
)
if (-not $cargoMatch.Success) {
    throw "Could not read the package version from src-tauri/Cargo.toml."
}

if (-not $lockVersions.rootPackageVersion) {
    throw "Could not read the root package version from package-lock.json."
}

$versions = [ordered]@{
    "package.json" = [string]$package.version
    "package-lock.json" = [string]$lockVersions.documentVersion
    "package-lock.json root package" = [string]$lockVersions.rootPackageVersion
    "src-tauri/Cargo.toml" = [string]$cargoMatch.Groups[1].Value
    "src-tauri/tauri.conf.json" = [string]$tauriConfig.version
    "README.md badge" = [string]$readmeVersionMatch.Groups[1].Value
    "README.zh-CN.md badge" = [string]$chineseReadmeVersionMatch.Groups[1].Value
}

$expectedVersion = $versions["package.json"]
foreach ($entry in $versions.GetEnumerator()) {
    if ($entry.Value -ne $expectedVersion) {
        throw "Version mismatch: $($entry.Key) is $($entry.Value), expected $expectedVersion."
    }
}

if ($Tag) {
    $expectedTag = "v$expectedVersion"
    if ($Tag -ne $expectedTag) {
        throw "Release tag $Tag does not match application version $expectedVersion. Expected $expectedTag."
    }
}

Write-Output $expectedVersion
