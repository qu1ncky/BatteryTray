# Installs or updates BatteryTray: copies BatteryTray.exe to Program Files, registers the logon task (starts it
# with admin rights and no UAC prompt) and adds a Start menu shortcut. Run install.cmd, which calls this script.
#   From a release zip: BatteryTray.exe sits next to this script and is installed as is.
#   From the source code: the exe is built first (needs the .NET 8 SDK).
# The install step asks for admin rights once (UAC). Program Files matters: the app runs elevated, so its exe
# must live where only administrators can replace it.
param([switch]$Elevated, [string]$Source)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$target = Join-Path $env:ProgramFiles 'BatteryTray'
$result = Join-Path ([IO.Path]::GetTempPath()) 'BatteryTray-install.txt'

if (-not $Elevated) {
    $Source = Join-Path $root 'BatteryTray.exe'
    if (-not (Test-Path $Source)) {
        $project = Join-Path $root 'BatteryTray.csproj'
        if (-not (Test-Path $project)) { throw 'BatteryTray.exe not found next to install.ps1' }
        Write-Host 'Building BatteryTray...'
        dotnet publish $project -p:PublishProfile=release --nologo -v q
        if ($LASTEXITCODE -ne 0) { throw 'build failed' }
        $Source = Join-Path $root 'publish\BatteryTray.exe'
    }
    Remove-Item $result -ErrorAction SilentlyContinue
    Write-Host 'Installing - confirm the administrator prompt...'
    $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden `
        -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Elevated', '-Source', "`"$Source`""
    if (Test-Path $result) { Get-Content $result }
    exit $p.ExitCode
}

$log = @()
$code = 1
try {
    Get-Process BatteryTray -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 700

    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Get-ChildItem -Path $target | Remove-Item -Recurse -Force # leftovers of an older build
    $exe = Join-Path $target 'BatteryTray.exe'
    Copy-Item -Path $Source -Destination $exe -Force
    # A downloaded exe carries the "from the internet" mark; SmartScreen would stop the unattended steps below
    Unblock-File -Path $exe
    $log += "copied to $target"

    $reg = Start-Process $exe -ArgumentList '--register-task' -Wait -PassThru
    if ($reg.ExitCode -ne 0) { throw "logon task registration failed (exit $($reg.ExitCode))" }
    $log += 'logon task registered: BatteryTray'

    # Start menu entry: launching the exe opens the window of the copy running in the tray
    $lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\BatteryTray.lnk'
    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $shortcut.TargetPath = $exe
    $shortcut.WorkingDirectory = $target
    $shortcut.Description = 'Battery level of wireless devices'
    $shortcut.Save()
    $log += 'Start menu shortcut: BatteryTray'

    Start-ScheduledTask -TaskName 'BatteryTray'
    Start-Sleep -Seconds 2
    if (-not (Get-Process BatteryTray -ErrorAction SilentlyContinue)) { throw 'installed, but the app did not start' }
    $log += 'BatteryTray is running - look for its icon next to the clock (it may be under the ^ arrow)'
    $log += 'OK'
    $code = 0
}
catch {
    $log += 'ERROR: ' + $_
}
$log | Set-Content -Path $result -Encoding ASCII
exit $code
