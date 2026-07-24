$ErrorActionPreference = "Stop"

$workspace = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $workspace "src-tauri"
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
$cmakeBin = "C:\Program Files\CMake\bin"

if (-not (Test-Path -LiteralPath $vcvars)) {
    throw "未找到 Visual Studio 2022 C++ Build Tools。"
}
if (-not (Test-Path -LiteralPath (Join-Path $cmakeBin "cmake.exe"))) {
    throw "未找到 CMake。"
}

$env:Path = "$cmakeBin;$env:Path"
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

Push-Location $workspace
try {
    & cmd.exe /d /s /c "npm ci"
    if ($LASTEXITCODE -ne 0) { throw "npm ci 失败" }
    & cmd.exe /d /s /c "npm run build"
    if ($LASTEXITCODE -ne 0) { throw "前端构建失败" }
    & cmd.exe /d /s /c "npm test"
    if ($LASTEXITCODE -ne 0) { throw "前端测试失败" }
    & node ".\scripts\generate-license-report.mjs"
    if ($LASTEXITCODE -ne 0) { throw "许可证清单生成失败" }

    $cargoCommand = "call `"$vcvars`" && cargo test --locked && cd /d `"$workspace`" && npx tauri build"
    & cmd.exe /d /s /c $cargoCommand
    if ($LASTEXITCODE -ne 0) { throw "Windows 发布构建失败" }

    $releaseDir = Join-Path $workspace "release"
    $portableDir = Join-Path $releaseDir "Nota-0.1.0-windows-x64-portable"
    if (Test-Path -LiteralPath $portableDir) {
        Remove-Item -LiteralPath $portableDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $portableDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $tauriRoot "target\release\nota.exe") -Destination $portableDir -Force
    Copy-Item -LiteralPath (Join-Path $workspace "README.md") -Destination $portableDir -Force
    Copy-Item -LiteralPath (Join-Path $workspace "THIRD_PARTY_LICENSES.md") -Destination $portableDir -Force
    $portableZip = Join-Path $releaseDir "Nota-0.1.0-windows-x64-portable.zip"
    if (Test-Path -LiteralPath $portableZip) {
        Remove-Item -LiteralPath $portableZip
    }
    Compress-Archive -Path (Join-Path $portableDir "*") -DestinationPath $portableZip -CompressionLevel Optimal

    $nsis = Get-ChildItem -LiteralPath (Join-Path $tauriRoot "target\release\bundle\nsis") -Filter "Nota_*.exe" |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $nsis) { throw "未找到 NSIS 安装包" }
    Copy-Item -LiteralPath $nsis.FullName -Destination $releaseDir -Force
    Write-Host "发布产物位于 $releaseDir"
}
finally {
    Pop-Location
}
