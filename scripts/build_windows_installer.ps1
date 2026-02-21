param(
    [switch]$InstallMissingTools,
    [switch]$SkipPythonBuild,
    [switch]$SkipRustBuild,
    [switch]$SignArtifacts,
    [string]$CodeSignCertPath = "",
    [string]$CodeSignCertPassword = "",
    [string]$SignToolPath = "",
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "[STEP] $Message" -ForegroundColor Cyan
}

function Write-Ok {
    param([string]$Message)
    Write-Host "[OK] $Message" -ForegroundColor Green
}

function Fail {
    param([string]$Message)
    throw $Message
}

function Resolve-Executable {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string]$FallbackPath
    )

    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        return $cmd.Source
    }

    if ($FallbackPath -and (Test-Path $FallbackPath)) {
        return $FallbackPath
    }

    return $null
}

function Resolve-SignToolPath {
    param([string]$CandidatePath)

    if ($CandidatePath -and (Test-Path $CandidatePath)) {
        return $CandidatePath
    }

    $cmd = Get-Command signtool -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        return $cmd.Source
    }

    $kitRoots = @(
        "C:\Program Files (x86)\Windows Kits\10\bin",
        "C:\Program Files\Windows Kits\10\bin"
    )
    foreach ($root in $kitRoots) {
        if (-not (Test-Path $root)) {
            continue
        }
        $candidate = Get-ChildItem -Path $root -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($null -ne $candidate) {
            return $candidate.FullName
        }
    }

    return $null
}

function Invoke-SignFile {
    param(
        [Parameter(Mandatory = $true)][string]$SignToolExe,
        [Parameter(Mandatory = $true)][string]$CertPath,
        [Parameter(Mandatory = $true)][string]$CertPassword,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$TimestampServer
    )

    if (-not (Test-Path $FilePath)) {
        Fail "Cannot sign missing file: $FilePath"
    }

    Invoke-Checked -Exe $SignToolExe -Arguments @(
        "sign",
        "/fd", "SHA256",
        "/td", "SHA256",
        "/tr", $TimestampServer,
        "/f", $CertPath,
        "/p", $CertPassword,
        $FilePath
    )

    $signature = Get-AuthenticodeSignature -FilePath $FilePath
    if ($signature.Status -ne "Valid") {
        Fail "Signature verification failed for $FilePath : $($signature.Status)"
    }
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [string]$WorkingDirectory
    )

    if ($WorkingDirectory) {
        Push-Location $WorkingDirectory
    }

    try {
        & $Exe @Arguments
        if ($LASTEXITCODE -ne 0) {
            Fail "Command failed: $Exe $($Arguments -join ' ') (exit code: $LASTEXITCODE)"
        }
    }
    finally {
        if ($WorkingDirectory) {
            Pop-Location
        }
    }
}

function Test-WixToolset {
    $candle = Get-Command candle -ErrorAction SilentlyContinue
    $light = Get-Command light -ErrorAction SilentlyContinue
    if ($null -ne $candle -and $null -ne $light) {
        return $true
    }

    if ($env:WIX) {
        $candlePath = Join-Path $env:WIX "candle.exe"
        $lightPath = Join-Path $env:WIX "light.exe"
        if ((Test-Path $candlePath) -and (Test-Path $lightPath)) {
            return $true
        }
    }

    return $false
}

$scriptDir = $PSScriptRoot
$repoRoot = Split-Path -Parent $scriptDir
$pythonProjectDir = Join-Path $repoRoot "ownstack-python"
$pythonSpec = Join-Path $pythonProjectDir "ownstack_backend.spec"
$pythonDistExe = Join-Path $pythonProjectDir "dist\ownstack_backend.exe"
$releaseDir = Join-Path $repoRoot "target\release"
$releaseBackendExe = Join-Path $releaseDir "ownstack_backend.exe"
$releaseIdeExe = Join-Path $releaseDir "ownstack-ide.exe"
$releaseAgentExe = Join-Path $releaseDir "ownstack-agent.exe"
$wixTargetDir = Join-Path $repoRoot "target\wix"

if (-not (Test-Path $pythonSpec)) {
    Fail "Spec file not found: $pythonSpec"
}

Write-Step "Resolving required executables"
$pythonExe = Resolve-Executable -Name "python" -FallbackPath ""
if (-not $pythonExe) {
    Fail "Python is required but was not found in PATH."
}

$cargoFallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$cargoExe = Resolve-Executable -Name "cargo" -FallbackPath $cargoFallback
if (-not $cargoExe) {
    Fail "Cargo is required but was not found in PATH or at $cargoFallback."
}

Write-Ok "Using python: $pythonExe"
Write-Ok "Using cargo: $cargoExe"

$signToolExe = $null
$effectiveCertPath = $CodeSignCertPath
$effectiveCertPassword = $CodeSignCertPassword
if ($SignArtifacts) {
    if (-not $effectiveCertPath) {
        $effectiveCertPath = $env:WINDOWS_CERT_PATH
    }
    if (-not $effectiveCertPassword) {
        $effectiveCertPassword = $env:WINDOWS_CERT_PASSWORD
    }
    if (-not $effectiveCertPath) {
        Fail "SignArtifacts requested but certificate path is missing. Set -CodeSignCertPath or WINDOWS_CERT_PATH."
    }
    if (-not (Test-Path $effectiveCertPath)) {
        Fail "Code signing certificate not found: $effectiveCertPath"
    }
    if (-not $effectiveCertPassword) {
        Fail "SignArtifacts requested but certificate password is missing. Set -CodeSignCertPassword or WINDOWS_CERT_PASSWORD."
    }

    $signToolExe = Resolve-SignToolPath -CandidatePath $SignToolPath
    if (-not $signToolExe) {
        Fail "SignArtifacts requested but signtool.exe was not found."
    }
    Write-Ok "Using signtool: $signToolExe"
}

Write-Step "Checking cargo-wix availability"
$hasCargoWix = $true
try {
    Invoke-Checked -Exe $cargoExe -Arguments @("wix", "--version") -WorkingDirectory $repoRoot
}
catch {
    $hasCargoWix = $false
}

if (-not $hasCargoWix) {
    if ($InstallMissingTools) {
        Write-Step "Installing cargo-wix"
        Invoke-Checked -Exe $cargoExe -Arguments @("install", "cargo-wix") -WorkingDirectory $repoRoot
    }
    else {
        Fail "cargo-wix is missing. Re-run with -InstallMissingTools or install manually: cargo install cargo-wix"
    }
}
Write-Ok "cargo-wix available"

Write-Step "Checking WiX Toolset (candle/light)"
if (-not (Test-WixToolset)) {
    Fail @"
WiX Toolset is not available (candle/light not found).
Install (admin): winget install --id WiXToolset.WiXToolset --exact --accept-source-agreements --accept-package-agreements
Then reopen your shell and re-run this script.
"@
}
Write-Ok "WiX Toolset detected"

if (-not $SkipPythonBuild) {
    Write-Step "Checking PyInstaller"
    $hasPyInstaller = $true
    try {
        Invoke-Checked -Exe $pythonExe -Arguments @("-m", "PyInstaller", "--version") -WorkingDirectory $repoRoot
    }
    catch {
        $hasPyInstaller = $false
    }

    if (-not $hasPyInstaller) {
        if ($InstallMissingTools) {
            Write-Step "Installing PyInstaller"
            Invoke-Checked -Exe $pythonExe -Arguments @("-m", "pip", "install", "pyinstaller>=6.16.0") -WorkingDirectory $repoRoot
        }
        else {
            Fail "PyInstaller is missing. Re-run with -InstallMissingTools or install manually: python -m pip install pyinstaller>=6.16.0"
        }
    }
    Write-Ok "PyInstaller available"

    Write-Step "Building bundled backend with PyInstaller"
    Invoke-Checked -Exe $pythonExe -Arguments @("-m", "PyInstaller", "ownstack_backend.spec", "--noconfirm") -WorkingDirectory $pythonProjectDir

    if (-not (Test-Path $pythonDistExe)) {
        Fail "Bundled backend was not produced: $pythonDistExe"
    }
    Write-Ok "Bundled backend built: $pythonDistExe"
}
else {
    Write-Step "Skipping Python bundle build"
    if (-not (Test-Path $pythonDistExe)) {
        Fail "SkipPythonBuild is set but backend executable is missing: $pythonDistExe"
    }
}

if (-not $SkipRustBuild) {
    Write-Step "Building Rust binaries (release)"
    Write-Step "Building ownstack-ide"
    Invoke-Checked -Exe $cargoExe -Arguments @("build", "--release", "-p", "lapce-app", "--bin", "ownstack-ide") -WorkingDirectory $repoRoot
    if (-not (Test-Path $releaseIdeExe)) {
        Fail "Rust IDE release binary missing after build: $releaseIdeExe"
    }
    
    Write-Step "Building ownstack-agent"
    Invoke-Checked -Exe $cargoExe -Arguments @("build", "--release", "-p", "ownstack-agent", "--bin", "ownstack-agent") -WorkingDirectory $repoRoot
    if (-not (Test-Path $releaseAgentExe)) {
        Fail "Rust agent release binary missing after build: $releaseAgentExe"
    }
    
    Write-Ok "Rust release binaries built"
}
else {
    Write-Step "Skipping Rust release build"
    if (-not (Test-Path $releaseIdeExe)) {
        Fail "SkipRustBuild is set but IDE executable is missing: $releaseIdeExe"
    }
    if (-not (Test-Path $releaseAgentExe)) {
        Fail "SkipRustBuild is set but agent executable is missing: $releaseAgentExe"
    }
}

Write-Step "Copying bundled backend next to release binaries"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item -Force $pythonDistExe $releaseBackendExe
Write-Ok "Backend copied to: $releaseBackendExe"

if ($SignArtifacts) {
    Write-Step "Signing release binaries"
    Invoke-SignFile -SignToolExe $signToolExe -CertPath $effectiveCertPath -CertPassword $effectiveCertPassword -FilePath $releaseIdeExe -TimestampServer $TimestampUrl
    Invoke-SignFile -SignToolExe $signToolExe -CertPath $effectiveCertPath -CertPassword $effectiveCertPassword -FilePath $releaseAgentExe -TimestampServer $TimestampUrl
    Invoke-SignFile -SignToolExe $signToolExe -CertPath $effectiveCertPath -CertPassword $effectiveCertPassword -FilePath $releaseBackendExe -TimestampServer $TimestampUrl
    Write-Ok "Release binaries signed"
}

Write-Step "Building MSI with cargo-wix"
Invoke-Checked -Exe $cargoExe -Arguments @("wix", "--package", "lapce-app", "--no-build", "--nocapture") -WorkingDirectory $repoRoot

if (-not (Test-Path $wixTargetDir)) {
    Fail "MSI build finished but target folder missing: $wixTargetDir"
}

$msi = Get-ChildItem -Path $wixTargetDir -Filter *.msi -ErrorAction SilentlyContinue |
Sort-Object LastWriteTime -Descending |
Select-Object -First 1

if ($null -eq $msi) {
    Fail "No MSI found in $wixTargetDir after cargo-wix run."
}

if ($SignArtifacts) {
    Write-Step "Signing MSI"
    Invoke-SignFile -SignToolExe $signToolExe -CertPath $effectiveCertPath -CertPassword $effectiveCertPassword -FilePath $msi.FullName -TimestampServer $TimestampUrl
    Write-Ok "MSI signed"
}

Write-Ok "MSI created: $($msi.FullName)"
Write-Host ""
Write-Host "Build complete." -ForegroundColor Green
