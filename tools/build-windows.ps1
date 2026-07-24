param(
    [switch]$Release,
    [string]$FrontendBundle = ""
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$Target = "x86_64-pc-windows-msvc"

if ([string]::IsNullOrWhiteSpace($FrontendBundle)) {
    $FrontendBundle = Join-Path $RootDir "target/frontend-bundle/frontend.bundle.tar"
}
if (-not (Test-Path $FrontendBundle)) {
    throw "Frontend bundle not found: $FrontendBundle"
}

$env:DEVICE_HUB_FRONTEND_BUNDLE = (Resolve-Path $FrontendBundle).Path
$Arguments = @(
    "build",
    "-p", "device-hub-server",
    "--no-default-features",
    "--features", "serial",
    "--target", $Target
)
if ($Release) {
    $Arguments += "--release"
}

& cargo @Arguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if ($Release) {
    $PackageDir = Join-Path $RootDir "target/packages/device-hub-windows-serial"
    $PackageZip = Join-Path $RootDir "target/packages/device-hub-windows-serial.zip"
    if (Test-Path $PackageDir) {
        Remove-Item $PackageDir -Recurse -Force
    }
    if (Test-Path $PackageZip) {
        Remove-Item $PackageZip -Force
    }
    New-Item -ItemType Directory -Path $PackageDir | Out-Null
    Copy-Item (Join-Path $RootDir "target/$Target/release/device-hub-server.exe") $PackageDir
    Copy-Item (Join-Path $RootDir "devices.example.yaml") $PackageDir
    Copy-Item (Join-Path $RootDir "devices.windows.yaml") $PackageDir
    Copy-Item (Join-Path $RootDir "README.md") $PackageDir
    Copy-Item (Join-Path $RootDir "docs/windows-linux.md") $PackageDir
    Copy-Item (Join-Path $RootDir "tools/windows-serial-smoke.ps1") $PackageDir
    Copy-Item (Join-Path $RootDir "tools/windows-runtime-smoke.ps1") $PackageDir
    Copy-Item (Join-Path $RootDir "tools/run-windows-from-wsl.sh") $PackageDir
    Copy-Item (Join-Path $RootDir "tools/windows-server-control.ps1") $PackageDir
    Compress-Archive -Path (Join-Path $PackageDir "*") -DestinationPath $PackageZip
    Write-Output $PackageZip
}
