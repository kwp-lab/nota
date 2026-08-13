[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$ExecutablePath,
    [string]$InstallerPath
)

$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $workspace "src-tauri"
$version = & (Join-Path $PSScriptRoot "verify-version.ps1")

if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $workspace "release"
}
if (-not $ExecutablePath) {
    $ExecutablePath = Join-Path $tauriRoot "target\release\nota.exe"
}

$outputFullPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$outputRoot = [System.IO.Path]::GetPathRoot($outputFullPath)
if ($outputFullPath.TrimEnd('\') -eq $outputRoot.TrimEnd('\')) {
    throw "The release output directory cannot be a drive root."
}

if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "The release executable was not found at $ExecutablePath."
}

if (-not $InstallerPath) {
    $nsisDirectory = Join-Path $tauriRoot "target\release\bundle\nsis"
    $installer = Get-ChildItem -LiteralPath $nsisDirectory -Filter "Nota_*.exe" |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $installer) {
        throw "The NSIS installer was not found in $nsisDirectory."
    }
    $InstallerPath = $installer.FullName
}
if (-not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
    throw "The NSIS installer was not found at $InstallerPath."
}

New-Item -ItemType Directory -Force -Path $outputFullPath | Out-Null

$portableBaseName = "Nota-$version-windows-x64-portable"
$portableZip = Join-Path $outputFullPath "$portableBaseName.zip"
$releaseInstaller = Join-Path $outputFullPath "Nota-$version-windows-x64-setup.exe"
$checksumPath = Join-Path $outputFullPath "Nota-$version-SHA256SUMS.txt"
$mplSourcesZip = Join-Path $outputFullPath "Nota-$version-mpl-sources.zip"
$stagingDirectory = Join-Path `
    ([System.IO.Path]::GetTempPath()) `
    "Nota-package-$([System.Guid]::NewGuid().ToString('N'))"

try {
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null

    Copy-Item -LiteralPath $ExecutablePath -Destination $stagingDirectory -Force
    $packageFiles = @(
        "README.md",
        "README.zh-CN.md",
        "LICENSE",
        "THIRD_PARTY_LICENSES.md",
        "THIRD_PARTY_NOTICES.txt",
        "THIRD_PARTY_SOURCES.md",
        "bom.cyclonedx.json"
    )
    foreach ($packageFile in $packageFiles) {
        $source = Join-Path $workspace $packageFile
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Required release compliance file is missing: $packageFile"
        }
        Copy-Item -LiteralPath $source -Destination $stagingDirectory -Force
    }

    if (Test-Path -LiteralPath $portableZip) {
        Remove-Item -LiteralPath $portableZip -Force
    }
    Compress-Archive `
        -Path (Join-Path $stagingDirectory "*") `
        -DestinationPath $portableZip `
        -CompressionLevel Optimal
}
finally {
    if (Test-Path -LiteralPath $stagingDirectory) {
        $stagingFullPath = [System.IO.Path]::GetFullPath($stagingDirectory)
        $temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if ($stagingFullPath.StartsWith($temporaryRoot) -and
            (Split-Path -Leaf $stagingFullPath).StartsWith("Nota-package-")) {
            Remove-Item -LiteralPath $stagingFullPath -Recurse -Force
        }
    }
}

Copy-Item -LiteralPath $InstallerPath -Destination $releaseInstaller -Force
& (Join-Path $PSScriptRoot "package-mpl-sources.ps1") -OutputPath $mplSourcesZip

$releaseFiles = @(
    $releaseInstaller,
    $portableZip,
    $mplSourcesZip,
    (Join-Path $workspace "bom.cyclonedx.json"),
    (Join-Path $workspace "THIRD_PARTY_NOTICES.txt"),
    (Join-Path $workspace "THIRD_PARTY_SOURCES.md")
)
foreach ($complianceFile in $releaseFiles[3..5]) {
    Copy-Item -LiteralPath $complianceFile -Destination $outputFullPath -Force
}
$releaseFiles = @(
    $releaseInstaller,
    $portableZip,
    $mplSourcesZip,
    (Join-Path $outputFullPath "bom.cyclonedx.json"),
    (Join-Path $outputFullPath "THIRD_PARTY_NOTICES.txt"),
    (Join-Path $outputFullPath "THIRD_PARTY_SOURCES.md")
)
$checksumLines = foreach ($file in $releaseFiles) {
    $hash = Get-FileHash -LiteralPath $file -Algorithm SHA256
    "{0}  {1}" -f $hash.Hash.ToLowerInvariant(), (Split-Path -Leaf $file)
}
[System.IO.File]::WriteAllLines(
    $checksumPath,
    $checksumLines,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Host "Release artifacts are available in $outputFullPath"
Write-Host "  $(Split-Path -Leaf $releaseInstaller)"
Write-Host "  $(Split-Path -Leaf $portableZip)"
Write-Host "  $(Split-Path -Leaf $mplSourcesZip)"
Write-Host "  $(Split-Path -Leaf $checksumPath)"
