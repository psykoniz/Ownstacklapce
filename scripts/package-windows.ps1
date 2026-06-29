<#
.SYNOPSIS
  Build a complete OwnStack IDE Windows distribution (portable zip + MSI).

  Unlike the bare CI portable (ide exe only), this bundles BOTH ownstack-ide.exe
  and ownstack-agent.exe. The IDE spawns the agent from its own directory, so
  the AI bridge is dead without it.

.PARAMETER BuildProfile
  Cargo profile to build/package. Default "release-lto" (max-opt, official).
  Use "release" for a faster build that is still optimized.

.PARAMETER SkipBuild
  Package from already-built binaries in target/<BuildProfile>/.

.EXAMPLE
  powershell -File scripts/package-windows.ps1 -BuildProfile release
#>
param(
    [string]$BuildProfile = "release-lto",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

if (-not $SkipBuild) {
    Write-Host "==> cargo build --profile $BuildProfile (ide + agent)"
    cargo build --profile $BuildProfile --bin ownstack-ide --bin ownstack-agent
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$binDir = Join-Path "target" $BuildProfile
$ide   = Join-Path $binDir "ownstack-ide.exe"
$agent = Join-Path $binDir "ownstack-agent.exe"
foreach ($f in @($ide, $agent)) {
    if (-not (Test-Path $f)) { throw "missing binary: $f (build first)" }
}

New-Item -ItemType Directory -Force -Path "dist" | Out-Null

# --- Complete portable package (ide + agent + icon) ---
$stage = "dist/OwnStack"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $ide   $stage
Copy-Item $agent $stage
Copy-Item "extra/windows/lapce.ico" $stage -ErrorAction SilentlyContinue
$zip = "dist/OwnStack-windows-portable.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage/*" -DestinationPath $zip
Write-Host ("==> Portable: {0} ({1:N1} MB)" -f $zip, ((Get-Item $zip).Length/1MB))

# --- MSI via WiX (the wxs reads target/release-lto/) ---
if ($BuildProfile -ne "release-lto") {
    New-Item -ItemType Directory -Force -Path "target/release-lto" | Out-Null
    Copy-Item $ide   "target/release-lto/" -Force
    Copy-Item $agent "target/release-lto/" -Force
}
$candle = Get-ChildItem "C:\Program Files (x86)\WiX Toolset*\bin\candle.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $candle) {
    Write-Warning "WiX (candle.exe) not found - skipping MSI. Portable zip is ready."
    return
}
$wixBin = $candle.DirectoryName
& "$wixBin\candle.exe" -arch x64 -ext WixUIExtension -ext WixUtilExtension -out "dist/ownstack-ide.wixobj" "extra/windows/wix/lapce.wxs"
if ($LASTEXITCODE -ne 0) { throw "candle failed" }
& "$wixBin\light.exe" -ext WixUIExtension -ext WixUtilExtension -out "dist/OwnStack-windows.msi" -sice:ICE61 -sice:ICE91 "dist/ownstack-ide.wixobj"
if ($LASTEXITCODE -ne 0) { throw "light failed" }
Write-Host ("==> MSI: dist/OwnStack-windows.msi ({0:N1} MB)" -f ((Get-Item 'dist/OwnStack-windows.msi').Length/1MB))
Write-Host "Done."
