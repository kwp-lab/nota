[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $workspace "src-tauri\Cargo.toml"
$outputFullPath = [System.IO.Path]::GetFullPath($OutputPath)
$temporaryDirectory = Join-Path `
    ([System.IO.Path]::GetTempPath()) `
    "Nota-mpl-sources-$([System.Guid]::NewGuid().ToString('N'))"

try {
    $metadataJson = & cargo metadata `
        --format-version 1 `
        --locked `
        --filter-platform x86_64-pc-windows-msvc `
        --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo metadata failed while collecting MPL sources."
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $resolvedIds = @($metadata.resolve.nodes | ForEach-Object { [string]$_.id })
    $packages = @($metadata.packages | Where-Object {
        ([string]$_.id -in $resolvedIds) -and
        $_.source -and
        ([string]$_.license).Contains("MPL-2.0")
    } | Sort-Object name, version)

    if ($packages.Count -eq 0) {
        throw "No MPL-2.0 Cargo packages were found; review the license policy before removing the source archive."
    }

    New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null
    $sourceRoot = Join-Path $temporaryDirectory "sources"
    New-Item -ItemType Directory -Path $sourceRoot | Out-Null
    Copy-Item `
        -LiteralPath (Join-Path $PSScriptRoot "licenses\MPL-SOURCE-README.txt") `
        -Destination (Join-Path $temporaryDirectory "README.txt")
    Copy-Item `
        -LiteralPath (Join-Path $workspace "THIRD_PARTY_SOURCES.md") `
        -Destination $temporaryDirectory

    foreach ($package in $packages) {
        $sourceDirectory = Split-Path -Parent ([string]$package.manifest_path)
        $destination = Join-Path $sourceRoot "$($package.name)-$($package.version)"
        Copy-Item -LiteralPath $sourceDirectory -Destination $destination -Recurse
    }

    # Cargo registry archives intentionally preserve very old normalized
    # timestamps, while the ZIP format cannot represent dates before 1980.
    # Normalize only the temporary archive copy; file contents remain exact.
    $zipEpoch = [DateTime]::new(1980, 1, 1, 0, 0, 0, [DateTimeKind]::Utc)
    Get-ChildItem -LiteralPath $temporaryDirectory -Recurse -Force | ForEach-Object {
        if ($_.LastWriteTimeUtc -lt $zipEpoch) {
            $_.LastWriteTimeUtc = $zipEpoch
        }
    }

    $parent = Split-Path -Parent $outputFullPath
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    if (Test-Path -LiteralPath $outputFullPath) {
        Remove-Item -LiteralPath $outputFullPath -Force
    }
    Compress-Archive `
        -Path (Join-Path $temporaryDirectory "*") `
        -DestinationPath $outputFullPath `
        -CompressionLevel Optimal
}
finally {
    if (Test-Path -LiteralPath $temporaryDirectory) {
        $temporaryFullPath = [System.IO.Path]::GetFullPath($temporaryDirectory)
        $temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if ($temporaryFullPath.StartsWith($temporaryRoot) -and
            (Split-Path -Leaf $temporaryFullPath).StartsWith("Nota-mpl-sources-")) {
            Remove-Item -LiteralPath $temporaryFullPath -Recurse -Force
        }
    }
}

Write-Host "MPL source archive created at $outputFullPath"
