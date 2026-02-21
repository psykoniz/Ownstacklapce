# Script to manually patch OwnStack IDE installation
# MUST BE RUN AS ADMINISTRATOR

$source = "c:\Users\leoon\Downloads\LapceOwnstack\Ownstacklapce\target\release\ownstack-ide.exe"
$dest = "C:\Program Files\OwnStack IDE\bin\ownstack-ide.exe"

Write-Host "--- OWNSTACK PATCHER ---"
Write-Host "Source: $source"
Write-Host "Dest:   $dest"

if (-not (Test-Path $source)) {
    Write-Error "Source file not found! Please build the project first."
    exit 1
}

try {
    Write-Host "Copying file (Force)..."
    Copy-Item -Path $source -Destination $dest -Force -ErrorAction Stop
    Write-Host "[SUCCESS] Installation patched!"
}
catch {
    Write-Error "Failed to copy: $_"
    Write-Host "HINT: Did you run this script as Administrator?"
}

Write-Host "Press Enter to close..."
Read-Host
