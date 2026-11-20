# Builds the Windows downloads into dist\:
#   Glint-<version>-windows-x64-setup.exe     the installer (needs Inno Setup 6: `iscc`)
#   Glint-<version>-windows-x64-portable.zip  glint.exe that keeps its data beside itself
#
#   .\scripts\dist-windows.ps1                  both
#   .\scripts\dist-windows.ps1 -Only portable   just the zip (no Inno Setup needed)
#   .\scripts\dist-windows.ps1 -Only setup      just the installer
#   .\scripts\dist-windows.ps1 -NoBuild         package the exe already in target\release
#
# Release builds leave out RUNE's headless tools (`--no-default-features`); a developer build
# (`cargo build --release`) keeps them.
param(
    [ValidateSet("all", "setup", "portable")] [string] $Only = "all",
    [switch] $NoBuild
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$exe = Join-Path $root "target\release\glint.exe"
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null

if (-not $NoBuild) {
    Get-Process glint -ErrorAction SilentlyContinue | Stop-Process -Force
    cargo build --release --locked -p glint --no-default-features
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
if (-not (Test-Path $exe)) { throw "no $exe; build it first" }

if ($Only -in "all", "portable") {
    $stage = Join-Path $dist "portable"
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item $exe $stage
    # The marker that makes glint.exe keep its settings, sign-in and cache in .\data beside it.
    Set-Content -Path (Join-Path $stage "glint.portable") -Encoding ascii -Value "Glint keeps its data in the data folder next to glint.exe while this file is here."
    Copy-Item LICENSE (Join-Path $stage "LICENSE.txt")
    Copy-Item NOTICE.md $stage
    $zip = Join-Path $dist "Glint-$version-windows-x64-portable.zip"
    Remove-Item -Force $zip -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
    Remove-Item -Recurse -Force $stage
    Write-Host "portable: $zip"
}

if ($Only -in "all", "setup") {
    $iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        foreach ($p in @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")) {
            if (Test-Path $p) { $iscc = $p; break }
        }
    }
    if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found: install it, or run with -Only portable" }
    & $iscc /Q "/DAppVersion=$version" "/DSourceExe=$exe" "/DOutDir=$dist" "packaging\windows\glint.iss"
    if ($LASTEXITCODE -ne 0) { throw "iscc failed" }
    Write-Host "setup: $(Join-Path $dist "Glint-$version-windows-x64-setup.exe")"
}
