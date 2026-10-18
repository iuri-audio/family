# Windows half of run-wsl.sh: bound the lifetime of only the Reaper instance we launch.
$ErrorActionPreference = 'Stop'
$arguments = '-newinst -nosplash -cfgfile "{0}" "{1}"' -f $env:REAPER_RUN_CONFIG, $env:REAPER_RUN_SCRIPT
$reaper = Start-Process -FilePath $env:REAPER_RUN_EXE -ArgumentList $arguments -PassThru
if (-not $reaper.WaitForExit([int]([double]$env:REAPER_RUN_TIMEOUT * 1000))) {
    Stop-Process -Id $reaper.Id -Force -ErrorAction SilentlyContinue
    exit 124
}
exit $reaper.ExitCode
