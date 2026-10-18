# Run alongside editor-cycles.lua. Assert final physical client sizes, including the first
# opening. Start this just after launching Reaper; Seconds must cover startup and all cycles.
# With -PhaseDirectory/-Phases instead of -Count/-Output, acknowledge each capture request
# from new-instance.lua and retain its visible host/canvas sizes in windows.json.
[CmdletBinding(DefaultParameterSetName='Cycles')]
param(
    [Parameter(Mandatory=$true)][string]$Title,
    [Parameter(Mandatory=$true)][int]$Width,
    [Parameter(Mandatory=$true)][int]$Height,
    [Parameter(Mandatory=$true,ParameterSetName='Cycles')][int]$Count,
    [Parameter(Mandatory=$true,ParameterSetName='Cycles')][string]$Output,
    [Parameter(Mandatory=$true,ParameterSetName='Phases')][string]$PhaseDirectory,
    [Parameter(Mandatory=$true,ParameterSetName='Phases')][int]$Phases,
    [int]$Dpi = 0,
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
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
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
 $script:samples[$h.ToInt64()] = [pscustomobject]@{handle=$h.ToInt64();class=[Windows]::Class($h);title=[Windows]::Title($h);width=$r.right;height=$r.bottom;left=$w.left;top=$w.top;right=$w.right;bottom=$w.bottom;dpi=[Windows]::GetDpiForWindow($h)}
}
function Scan-Windows([bool]$VisibleOnly) {
 [void][Windows]::EnumWindows({param($h,$l)
  if ([Windows]::Title($h) -like $Title -and (-not $VisibleOnly -or [Windows]::IsWindowVisible($h))) {
   Capture $h
   [void][Windows]::EnumChildWindows($h,{param($c,$l)
    if (-not $VisibleOnly -or [Windows]::IsWindowVisible($c)) { Capture $c }
    return $true
   },[IntPtr]::Zero)
  }
  return $true
 },[IntPtr]::Zero)
}
function Window-Result([int]$ExpectedCount) {
 $windows = @($script:samples.Values | Sort-Object handle)
 $hosts = @($windows | Where-Object { $_.class -eq 'reaperPluginHostWrapProc' })
 $canvases = @($windows | Where-Object { $_.class -like 'Baseview-*' })
 $bad = @(@($hosts) + @($canvases) | Where-Object {
  $_.width -ne $Width -or $_.height -ne $Height -or ($Dpi -gt 0 -and $_.dpi -ne $Dpi)
 })
 [pscustomobject]@{
  passed=($hosts.Count -eq $ExpectedCount -and $canvases.Count -eq $ExpectedCount -and $bad.Count -eq 0)
  hosts=$hosts; canvases=$canvases; windows=$windows
 }
}
if ($PSCmdlet.ParameterSetName -eq 'Phases') {
 $records = New-Object System.Collections.Generic.List[object]
 $last = ''
 $deadline = (Get-Date).AddSeconds($Seconds)
 while ((Get-Date) -lt $deadline) {
  $request = Join-Path $PhaseDirectory 'capture-request.txt'
  if (Test-Path $request) {
   $phase = (Get-Content -Raw $request).Trim()
   if ($phase -and $phase -ne $last) {
    $script:samples = @{}
    Scan-Windows $true
    $measurement = Window-Result 1
    $records.Add([pscustomobject]@{
     phase=$phase; passed=$measurement.passed; hosts=$measurement.hosts
     canvases=$measurement.canvases; windows=$measurement.windows
    })
    ConvertTo-Json -InputObject @($records.ToArray()) -Depth 8 |
     Set-Content -Encoding UTF8 (Join-Path $PhaseDirectory 'windows.json')
    Write-Output "$phase passed=$($measurement.passed) hosts=$($measurement.hosts.Count) canvases=$($measurement.canvases.Count)"
    [System.IO.File]::WriteAllText((Join-Path $PhaseDirectory 'capture-done.txt'),$phase)
    $last = $phase
   }
  }
  if (Test-Path (Join-Path $PhaseDirectory 'finished.txt')) { break }
  Start-Sleep -Milliseconds 100
 }
 $finished = Join-Path $PhaseDirectory 'finished.txt'
 if (-not (Test-Path $finished) -or (Get-Content -Raw $finished) -notlike 'COMPLETE:*') {
  throw "Scenario failed or timed out; inspect result.txt and windows.json in $PhaseDirectory."
 }
 if ($records.Count -ne $Phases) { throw "Expected $Phases phases, measured $($records.Count)." }
 if (@($records | Where-Object { -not $_.passed }).Count) { exit 2 }
 Write-Output "PASS: all $Phases phases have ${Width}x${Height} host and canvas client sizes."
 exit 0
}
for ($step=0; $step -lt ($Seconds * 10); $step++) {
 Scan-Windows $false
 Start-Sleep -Milliseconds 100
}
$script:samples.Values | Sort-Object handle | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 $Output
$measurement = Window-Result $Count
if (-not $measurement.passed) { throw "Wrong editor count, size or DPI; inspect $Output." }
Write-Output "PASS: all $Count host windows and canvases are ${Width}x${Height} physical pixels."

