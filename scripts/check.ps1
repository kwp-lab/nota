$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $workspace "src-tauri\Cargo.toml"
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vcvars = $null

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,
        [Parameter(Mandatory = $true)]
        [string[]]$ArgumentList,
        [Parameter(Mandatory = $true)]
        [string]$FailureMessage
    )

    & $Command @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

if (Test-Path -LiteralPath $vswhere) {
    $vcvars = & $vswhere `
        -latest `
        -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -find "VC\Auxiliary\Build\vcvars64.bat" |
        Select-Object -First 1
}
if (-not $vcvars) {
    $fallbackVcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    if (Test-Path -LiteralPath $fallbackVcvars) {
        $vcvars = $fallbackVcvars
    }
}

if (-not $vcvars -or -not (Test-Path -LiteralPath $vcvars)) {
    throw "Visual Studio 2022 C++ Build Tools were not found."
}
if (-not (Get-Command cmake.exe -ErrorAction SilentlyContinue)) {
    throw "CMake was not found."
}

$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

Push-Location $workspace
try {
    & ".\scripts\verify-version.ps1"
    Invoke-NativeCommand "npm.cmd" @("run", "build:web") "The frontend build failed."
    Invoke-NativeCommand "npm.cmd" @("test") "The frontend tests failed."
    Invoke-NativeCommand "cargo.exe" @(
        "fmt",
        "--manifest-path", $manifest,
        "--all",
        "--",
        "--check"
    ) "Rust formatting failed."

    $cargoChecks = "call `"$vcvars`" && cargo test --locked --manifest-path `"$manifest`" && cargo clippy --locked --manifest-path `"$manifest`" --all-targets -- -D warnings"
    & cmd.exe /d /s /c $cargoChecks
    if ($LASTEXITCODE -ne 0) {
        throw "Rust tests or Clippy failed."
    }
}
finally {
    Pop-Location
}
