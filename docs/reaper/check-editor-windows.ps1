# Run alongside editor-cycles.lua. Assert final physical client sizes, including the first
# opening. Start this just after launching Reaper; Seconds must cover startup and all cycles.
param(
    [Parameter(Mandatory=$true)][string]$Title,
    [Parameter(Mandatory=$true)][int]$Width,
    [Parameter(Mandatory=$true)][int]$Height,
    [Parameter(Mandatory=$true)][int]$Count,
    [Parameter(Mandatory=$true)][string]$Output,
    [int]$Seconds = 12
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Windows {
 public delegate bool EnumProc(IntPtr h, IntPtr l);
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left, top, right, bottom; }
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
 [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 public static string Title(IntPtr h) { var s=new StringBuilder(1024); GetWindowText(h,s,1024); return s.ToString(); }
 public static string Class(IntPtr h) { var s=new StringBuilder(1024); GetClassName(h,s,1024); return s.ToString(); }
}
'@
[void][Windows]::SetThreadDpiAwarenessContext([IntPtr](-4))
$script:samples = @{}
function Capture($h) {
 $r = New-Object Windows+Rect
 [void][Windows]::GetClientRect($h,[ref]$r)
 $w = New-Object Windows+Rect
 [void][Windows]::GetWindowRect($h,[ref]$w)
 $script:samples[$h.ToInt64()] = [pscustomobject]@{handle=$h.ToInt64();class=[Windows]::Class($h);width=$r.right;height=$r.bottom;left=$w.left;top=$w.top;right=$w.right;bottom=$w.bottom;dpi=[Windows]::GetDpiForWindow($h)}
}
for ($step=0; $step -lt ($Seconds * 10); $step++) {
 [void][Windows]::EnumWindows({param($h,$l)
  if ([Windows]::Title($h) -like $Title) {
   Capture $h
   [void][Windows]::EnumChildWindows($h,{param($c,$l) Capture $c; return $true},[IntPtr]::Zero)
  }
  return $true
 },[IntPtr]::Zero)
 Start-Sleep -Milliseconds 100
}
$script:samples.Values | Sort-Object handle | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 $Output
$hosts = @($script:samples.Values | Where-Object { $_.class -eq 'reaperPluginHostWrapProc' })
$canvases = @($script:samples.Values | Where-Object { $_.class -like 'Baseview-*' })
if ($hosts.Count -ne $Count -or $canvases.Count -ne $Count) {
    throw "Expected $Count host and canvas windows, found $($hosts.Count) and $($canvases.Count)."
}
foreach ($window in @($hosts) + @($canvases)) {
    if ($window.width -ne $Width -or $window.height -ne $Height) {
        throw "Clipped or wrongly sized editor: $($window.class) is $($window.width)x$($window.height), expected ${Width}x${Height}."
    }
}
Write-Output "PASS: all $Count host windows and canvases are ${Width}x${Height} physical pixels."

