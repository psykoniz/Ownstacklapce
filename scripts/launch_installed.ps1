# Debug script to launch OwnStack IDE and capture output
$exePath = "C:\Program Files\OwnStack IDE\bin\ownstack-ide.exe"
$workDir = "C:\Program Files\OwnStack IDE\bin"

Write-Host "--- OWNSTACK IDE LAUNCH DEBUGGER ---"
Write-Host "Target: $exePath"
Write-Host "WorkDir: $workDir"

if (Test-Path $exePath) {
    Write-Host "[OK] Executable found."
    Write-Host "Launching process..."
    
    try {
        # Launch with redirected output streams and wait for exit
        $process = Start-Process -FilePath $exePath -WorkingDirectory $workDir -NoNewWindow -Wait -PassThru
        
        Write-Host "Process exited with code: $($process.ExitCode)"
        
        if ($process.ExitCode -ne 0) {
            Write-Error "Process exited with ERROR code."
        }
    }
    catch {
        Write-Error "FATAL EXCEPTION: $_"
    }
}
else {
    Write-Error "[FAIL] Executable NOT FOUND at: $exePath"
    Write-Host "Please verify installation path."
}

Write-Host "--- END OF LOG ---"
# Read-Host removed for automated debugging
