# Build + launch the dev binary from this repo.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File .\scripts\launch_dev.ps1
#   powershell -ExecutionPolicy Bypass -File .\scripts\launch_dev.ps1 -Release -Wait

param(
    [switch]$Release,
    [switch]$Wait,
    [string[]]$Args = @('--wait', '--new')
)

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

Write-Host "--- OWNSTACK IDE DEV LAUNCH ---"
Write-Host ("Repo: " + $repoRoot)

$profileDir = if ($Release) { "release" } else { "debug" }

Write-Host ("Building (profile=" + $profileDir + ") ...")
if ($Release) {
    cargo build -p lapce-app --release
} else {
    cargo build -p lapce-app
}

if ($LASTEXITCODE -ne 0) {
    Write-Error ("Build failed with exit code: " + $LASTEXITCODE)
    exit $LASTEXITCODE
}

$exePath = Join-Path $repoRoot ("target\\" + $profileDir + "\\ownstack-ide.exe")
if (-not (Test-Path $exePath)) {
    Write-Error ("Executable not found: " + $exePath)
    exit 1
}

$workDir = Split-Path -Parent $exePath
Write-Host ("Launching: " + $exePath)
Write-Host ("WorkDir:   " + $workDir)

if ($Wait) {
    $p = Start-Process -FilePath $exePath -ArgumentList $Args -WorkingDirectory $workDir -Wait -PassThru
    Write-Host ("Process exited with code: " + $p.ExitCode)
    exit $p.ExitCode
} else {
    Start-Process -FilePath $exePath -ArgumentList $Args -WorkingDirectory $workDir | Out-Null
}

Write-Host "--- END ---"
