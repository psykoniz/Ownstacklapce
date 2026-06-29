<#
.SYNOPSIS
  Smoke-test the portable distribution: extract it to a clean temp dir, launch
  the IDE from there, and confirm the BUNDLED ownstack-agent.exe actually spawns.

  This guards the exact packaging bug we fixed: a portable zip / MSI that ships
  only ownstack-ide.exe leaves the AI bridge dead, because the IDE spawns the
  agent from its own directory.

.PARAMETER Zip
  Path to the portable zip. Default dist/OwnStack-windows-portable.zip
#>
param(
    [string]$Zip = "dist/OwnStack-windows-portable.zip"
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

if (-not (Test-Path $Zip)) { throw "portable zip not found: $Zip (run package-windows.ps1 first)" }

$work = Join-Path $env:TEMP ("ownstack-smoke-" + [guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Force -Path $work | Out-Null
Write-Host "==> extracting $Zip -> $work"
Expand-Archive -Path $Zip -DestinationPath $work -Force

$ide   = Join-Path $work "ownstack-ide.exe"
$agent = Join-Path $work "ownstack-agent.exe"
if (-not (Test-Path $ide))   { throw "FAIL: ownstack-ide.exe missing from package" }
if (-not (Test-Path $agent)) { throw "FAIL: ownstack-agent.exe missing from package (the bundled-agent bug is back)" }
Write-Host "==> package contains both exes (ide + agent) OK"

# Count agents before, launch the extracted IDE, confirm an agent spawns.
function Count-Agents { (Get-Process -Name ownstack-agent -ErrorAction SilentlyContinue | Measure-Object).Count }
$before = Count-Agents
Write-Host "==> agents before launch: $before"

$proc = Start-Process -FilePath $ide -PassThru
try {
    $spawned = $false
    foreach ($i in 1..20) {
        Start-Sleep -Milliseconds 750
        # an agent whose path is inside our temp work dir == spawned by THIS ide
        $ours = Get-CimInstance Win32_Process -Filter "Name='ownstack-agent.exe'" -ErrorAction SilentlyContinue |
                Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($work) }
        if ($ours) { $spawned = $true; break }
    }
    if ($spawned) {
        Write-Host "==> PASS: bundled agent spawned from the package dir" -ForegroundColor Green
        $exit = 0
    } else {
        Write-Host "==> FAIL: IDE launched but no agent spawned from package dir" -ForegroundColor Red
        $exit = 1
    }
} finally {
    if ($proc -and -not $proc.HasExited) { $proc.Kill() }
    Start-Sleep -Seconds 2
    # job object should have reaped the agent; clean up any stragglers from our dir
    Get-CimInstance Win32_Process -Filter "Name='ownstack-agent.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($work) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
exit $exit
