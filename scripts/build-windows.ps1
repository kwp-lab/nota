$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $workspace "src-tauri"
$packageJson = Get-Content -Raw -LiteralPath (Join-Path $workspace "package.json") |
    ConvertFrom-Json
if ([string]$packageJson.packageManager -notmatch '^npm@(.+)$') {
    throw "package.json must declare a pinned npm packageManager version."
}
$npmCommand = "npx --yes npm@$($Matches[1])"
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vcvars = $null

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

$cmake = Get-Command cmake.exe -ErrorAction SilentlyContinue
if (-not $cmake) {
    throw "CMake was not found."
}

$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

Push-Location $workspace
try {
    & ".\scripts\verify-version.ps1"
    & cmd.exe /d /s /c "$npmCommand ci"
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed." }
    & cmd.exe /d /s /c "$npmCommand run build"
    if ($LASTEXITCODE -ne 0) { throw "The frontend build failed." }
    & cmd.exe /d /s /c "$npmCommand test"
    if ($LASTEXITCODE -ne 0) { throw "The frontend tests failed." }
    & node ".\scripts\generate-license-report.mjs"
    if ($LASTEXITCODE -ne 0) { throw "Generating the license report failed." }

    $cargoCommand = "call `"$vcvars`" && cd /d `"$tauriRoot`" && cargo test --locked && cd /d `"$workspace`" && $npmCommand run tauri -- build"
    & cmd.exe /d /s /c $cargoCommand
    if ($LASTEXITCODE -ne 0) { throw "The Windows release build failed." }

    & ".\scripts\package-windows-release.ps1"
}
finally {
    Pop-Location
}
