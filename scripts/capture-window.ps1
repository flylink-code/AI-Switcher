param(
  [Parameter(Mandatory=$true)][string]$OutPath,
  [int]$ClickX = -1,
  [int]$ClickY = -1,
  [int]$SettleMs = 900
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern void SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(int flags, int dx, int dy, int data, int extra);
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@

$proc = Get-Process | Where-Object { $_.MainWindowTitle -like "*AI-Switcher*" } | Select-Object -First 1
if (-not $proc) { Write-Error "AI-Switcher window not found"; exit 1 }

$hwnd = $proc.MainWindowHandle
[Win32]::ShowWindow($hwnd, 9) | Out-Null  # SW_RESTORE
[Win32]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 400

$rect = New-Object Win32+RECT
[Win32]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
Write-Output "Window: ($($rect.Left),$($rect.Top)) ${w}x${h} title='$($proc.MainWindowTitle)'"

if ($ClickX -ge 0 -and $ClickY -ge 0) {
  $absX = $rect.Left + $ClickX
  $absY = $rect.Top + $ClickY
  [Win32]::SetCursorPos($absX, $absY)
  Start-Sleep -Milliseconds 150
  [Win32]::mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
  Start-Sleep -Milliseconds 60
  [Win32]::mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP
  Write-Output "Clicked relative ($ClickX,$ClickY)"
  Start-Sleep -Milliseconds $SettleMs
}

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
$bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "Saved: $OutPath"
