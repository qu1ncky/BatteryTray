# Removes BatteryTray: stops it, deletes the logon task, the Start menu shortcut and the Program Files folder.
# Settings, history and the log in %LOCALAPPDATA%\BatteryTray are kept; delete that folder by hand if you want.
param([switch]$Elevated)

$ErrorActionPreference = 'Stop'
$result = Join-Path ([IO.Path]::GetTempPath()) 'BatteryTray-uninstall.txt'

if (-not $Elevated) {
    Remove-Item $result -ErrorAction SilentlyContinue
    Write-Host 'Removing BatteryTray - confirm the administrator prompt...'
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden `
        -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Elevated'
    if (Test-Path $result) { Get-Content $result }
    exit $p.ExitCode
}

$code = 1
try {
    Get-Process BatteryTray -ErrorAction SilentlyContinue | Stop-Process -Force
    if (Get-ScheduledTask -TaskName 'BatteryTray' -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskName 'BatteryTray' -Confirm:$false
    }
    Start-Sleep -Milliseconds 700
    $target = Join-Path $env:ProgramFiles 'BatteryTray'
    if (Test-Path $target) { Remove-Item $target -Recurse -Force }
    $lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\BatteryTray.lnk'
    if (Test-Path $lnk) { Remove-Item $lnk -Force }
    $message = 'BatteryTray removed. Settings and log are kept in ' + (Join-Path $env:LOCALAPPDATA 'BatteryTray')
    $code = 0
}
catch {
    $message = 'ERROR: ' + $_
}
$message | Set-Content -Path $result -Encoding ASCII
exit $code
