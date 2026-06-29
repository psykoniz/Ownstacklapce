# Capture the screen (or foreground window) to a PNG for UI inspection.
# Usage: powershell -File ui_screenshot.ps1 -Out path.png [-Window]
param(
    [string]$Out = "$env:TEMP\ownstack-shot.png",
    [switch]$Window  # capture only the foreground window instead of full virtual screen
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W]::SetProcessDPIAware()   # capture in physical pixels (match the mouse driver)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
if ($Window) {
    $h = [W]::GetForegroundWindow()
    $r = New-Object W+RECT
    [void][W]::GetWindowRect($h, [ref]$r)
    $bounds = New-Object System.Drawing.Rectangle $r.Left, $r.Top, ($r.Right-$r.Left), ($r.Bottom-$r.Top)
} else {
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
}
$bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Host "saved $Out ($($bounds.Width)x$($bounds.Height))"
