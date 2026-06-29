# Real keyboard/mouse input driver for ergonomics testing.
# Mouse uses SendInput with absolute coordinates (reliable hit-testing on winit/floem).
# One action per invocation. Re-focuses the OwnStack window unless -NoFocus.
#
#   -Action click  -X 894 -Y 401           : move + left click at X,Y (physical px)
#   -Action dclick -X .. -Y ..              : double click
#   -Action move   -X .. -Y ..              : move only
#   -Action rclick -X .. -Y ..              : right click
#   -Action type   -Text "hello world"      : type literal text
#   -Action keys   -Keys "^o"               : raw SendKeys (^o = Ctrl+O, {ENTER}, ...)
param(
    [Parameter(Mandatory)][string]$Action,
    [int]$X = -1,
    [int]$Y = -1,
    [string]$Text = "",
    [string]$Keys = "",
    [int]$PostDelay = 500,
    [switch]$NoFocus
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class UIn {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] p, int cb);

  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
    public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
  }
  const uint MOVE=0x0001, LDOWN=0x0002, LUP=0x0004, RDOWN=0x0008, RUP=0x0010, ABS=0x8000, VDESK=0x4000;

  static INPUT Mk(int nx, int ny, uint flags){
    INPUT i = new INPUT(); i.type = 0; // INPUT_MOUSE
    i.mi.dx = nx; i.mi.dy = ny; i.mi.dwFlags = flags; return i;
  }
  static void Norm(int x, int y, out int nx, out int ny){
    int w = GetSystemMetrics(0), h = GetSystemMetrics(1);
    nx = (int)((x * 65535.0) / (w - 1));
    ny = (int)((y * 65535.0) / (h - 1));
  }
  public static void Move(int x, int y){
    int nx, ny; Norm(x,y,out nx,out ny);
    SendInput(1, new INPUT[]{ Mk(nx,ny, MOVE|ABS|VDESK) }, Marshal.SizeOf(typeof(INPUT)));
  }
  public static void Click(int x, int y){
    int nx, ny; Norm(x,y,out nx,out ny);
    INPUT[] seq = new INPUT[]{
      Mk(nx,ny, MOVE|ABS|VDESK),
      Mk(nx,ny, LDOWN|ABS|VDESK),
      Mk(nx,ny, LUP|ABS|VDESK)
    };
    SendInput(1,new INPUT[]{seq[0]},Marshal.SizeOf(typeof(INPUT))); System.Threading.Thread.Sleep(60);
    SendInput(1,new INPUT[]{seq[1]},Marshal.SizeOf(typeof(INPUT))); System.Threading.Thread.Sleep(50);
    SendInput(1,new INPUT[]{seq[2]},Marshal.SizeOf(typeof(INPUT)));
  }
  public static void RClick(int x, int y){
    int nx, ny; Norm(x,y,out nx,out ny);
    SendInput(1,new INPUT[]{ Mk(nx,ny, MOVE|ABS|VDESK) },Marshal.SizeOf(typeof(INPUT))); System.Threading.Thread.Sleep(60);
    SendInput(1,new INPUT[]{ Mk(nx,ny, RDOWN|ABS|VDESK) },Marshal.SizeOf(typeof(INPUT))); System.Threading.Thread.Sleep(50);
    SendInput(1,new INPUT[]{ Mk(nx,ny, RUP|ABS|VDESK) },Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[void][UIn]::SetProcessDPIAware()   # MUST run before WinForms loads, else coords stay logical
Add-Type -AssemblyName System.Windows.Forms

if (-not $NoFocus) {
    $ide = Get-Process -Name ownstack-ide -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($ide) {
        [void][UIn]::ShowWindow($ide.MainWindowHandle, 9)
        [void][UIn]::SetForegroundWindow($ide.MainWindowHandle)
        Start-Sleep -Milliseconds 300
    }
}
switch ($Action) {
    "move"   { [UIn]::Move($X, $Y) }
    "click"  { [UIn]::Click($X, $Y) }
    "dclick" { [UIn]::Click($X, $Y); Start-Sleep -Milliseconds 90; [UIn]::Click($X, $Y) }
    "rclick" { [UIn]::RClick($X, $Y) }
    "type"   { [System.Windows.Forms.SendKeys]::SendWait($Text) }
    "keys"   { [System.Windows.Forms.SendKeys]::SendWait($Keys) }
}
Start-Sleep -Milliseconds $PostDelay
Write-Host "ok: $Action X=$X Y=$Y"
