# Takes the app through what happens on a user's PC, on a CI runner (it has a desktop, and no UAC prompts):
#   1. the last C# release, installed from its own zip, updates itself to this build
#   2. this build updates itself to a newer one
#   3. uninstall, then a fresh install of this build; its window opens
#   4. a portable copy registers its logon task and updates itself
#   5. the windows drawn with sample data, and the popups opened as the app opens them
# Nothing in the app is changed for it. The app asks api.github.com for the latest release, so that name is pointed
# at this machine (hosts) where a small server answers with a release made of the exe under test, over HTTPS with a
# certificate this machine is told to trust.
# It uninstalls BatteryTray and edits hosts: only for a throwaway machine, so it refuses to run outside CI.
param(
    [Parameter(Mandatory)][string]$Exe,      # the release exe as built
    [Parameter(Mandatory)][string]$Newer,    # the same build with a higher version
    [Parameter(Mandatory)][string]$Out,      # screenshots and logs end up here
    [string]$Examples,                       # folder with snapshot.exe and popup_probe.exe
    [string]$CSharpTag = 'v1.8.8'
)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true') { throw 'this uninstalls BatteryTray and edits hosts: run it on a CI runner only' }

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Exe = (Resolve-Path $Exe).Path
$Newer = (Resolve-Path $Newer).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
if ($Examples) { $Examples = (Resolve-Path $Examples).Path }
$work = Join-Path ([IO.Path]::GetTempPath()) 'bt-desktop'
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force -Path $work | Out-Null
$feed = Join-Path $work 'feed'
New-Item -ItemType Directory -Force -Path $feed | Out-Null

$installed = Join-Path $env:ProgramFiles 'BatteryTray\BatteryTray.exe'
$data = Join-Path $env:LOCALAPPDATA 'BatteryTray'
$logFile = Join-Path $data 'log.txt'
$hosts = Join-Path $env:windir 'System32\drivers\etc\hosts'
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\BatteryTray.lnk'
$appId = '{5d0f1a3e-6c1b-4a57-9c3e-2f6b8a7d9e10}'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
# The app's windows a user can see are the ones with a title bar. The process always has a "visible" window besides:
# winit's event target, 14 px of a layered window that is never drawn, which is what Process.MainWindowHandle finds.
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class AppWindows {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr lparam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    public static List<string> Titles(uint pid) {
        var titles = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            const long caption = 0x00C00000;
            if (p == pid && IsWindowVisible(h) && (GetWindowLongPtr(h, -16).ToInt64() & caption) == caption) {
                var t = new StringBuilder(256); GetWindowText(h, t, 256);
                titles.Add(t.ToString());
            }
            return true;
        }, IntPtr.Zero);
        return titles;
    }
}
'@

function Write-Step([string]$Text) { Write-Host ''; Write-Host "=== $Text" }

function Assert-That([bool]$Condition, [string]$What) {
    if (-not $Condition) { throw "FAILED: $What" }
    Write-Host "  ok: $What"
}

function Wait-For([scriptblock]$Condition, [int]$Seconds, [string]$What) {
    $until = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $until) {
        if (& $Condition) { Write-Host "  ok: $What"; return }
        Start-Sleep -Milliseconds 500
    }
    throw "FAILED (waited $Seconds s): $What"
}

# "1.99.1" from the exe's FileVersion "1.99.1.0"; empty while the file is missing or being replaced
function Get-ExeVersion([string]$Path) {
    try {
        $v = [version](Get-Item $Path -ErrorAction Stop).VersionInfo.FileVersion
        return "$($v.Major).$($v.Minor).$($v.Build)"
    }
    catch { return '' }
}

function Get-AppLog { if (Test-Path $logFile) { @(Get-Content $logFile) } else { @() } }

# True when a log line written after $Mark contains $Text
function Test-AppLog([int]$Mark, [string]$Text) {
    $lines = Get-AppLog
    for ($i = $Mark; $i -lt $lines.Count; $i++) { if ($lines[$i].Contains($Text)) { return $true } }
    return $false
}

function Get-App { @(Get-Process BatteryTray -ErrorAction SilentlyContinue) }

# The titles of the app's windows that are on the screen
function Get-AppWindow { @(Get-App | ForEach-Object { [AppWindows]::Titles([uint32]$_.Id) }) }

function Save-Screen([string]$Name) {
    try {
        $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
        $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
        $bmp.Save((Join-Path $Out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
        Write-Host "  screenshot: $Name.png"
    }
    catch { Write-Host "  screenshot $Name failed: $_" }
}

# Makes $File the latest release GitHub knows of
function Set-Feed([string]$File) {
    $tag = 'v' + (Get-ExeVersion $File)
    $dir = Join-Path $feed "dl\$tag"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item $File (Join-Path $dir 'BatteryTray.exe') -Force
    $release = @{
        tag_name = $tag
        body     = 'test release'
        html_url = "https://github.com/qu1ncky/BatteryTray/releases/tag/$tag"
        assets   = @(@{
                name                 = 'BatteryTray.exe'
                size                 = (Get-Item $File).Length
                digest               = 'sha256:' + (Get-FileHash $File -Algorithm SHA256).Hash.ToLower()
                browser_download_url = "https://api.github.com/dl/$tag/BatteryTray.exe"
            })
    }
    $tmp = Join-Path $feed 'latest.tmp'
    [IO.File]::WriteAllText($tmp, ($release | ConvertTo-Json -Depth 5))
    Move-Item $tmp (Join-Path $feed 'latest.json') -Force
    Write-Host "  the latest release is now $tag"
}

function Invoke-Script([string]$Path) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Path | ForEach-Object { Write-Host "  | $_" }
    return $LASTEXITCODE
}

# Runs an exe and gives its exit code, or -1 when it is still running after $Seconds
function Invoke-Exe([string]$File, [string]$Arguments, [int]$Seconds = 30) {
    $p = [System.Diagnostics.Process]::Start($File, $Arguments)
    if (-not $p.WaitForExit($Seconds * 1000)) { return -1 }
    return $p.ExitCode
}

function Test-Task([string]$File, [string]$RunLevel) {
    $task = Get-ScheduledTask -TaskName 'BatteryTray'
    return $task.Actions[0].Execute.Trim('"') -eq $File -and $task.Actions[0].Arguments -eq '--background' -and
        $task.Principal.RunLevel -eq $RunLevel
}

function Stop-App {
    Get-App | Stop-Process -Force
    Wait-For { (Get-App).Count -eq 0 } 15 'the app is stopped'
}

$server = $null
try {
    Write-Step 'The machine'
    $os = Get-CimInstance Win32_OperatingSystem
    Write-Host "  $($os.Caption), build $($os.BuildNumber)"
    Write-Host "  session $((Get-Process -Id $PID).SessionId), interactive: $([Environment]::UserInteractive)"
    $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
    Write-Host "  screen $($screen.Width)x$($screen.Height)"
    $fonts = 'SegoeIcons.ttf', 'segmdl2.ttf', 'SegUIVar.ttf', 'segoeui.ttf' |
        ForEach-Object { "$_=" + (Test-Path (Join-Path $env:windir "Fonts\$_")) }
    Write-Host "  fonts: $($fonts -join ', ')"
    $thisVersion = Get-ExeVersion $Exe
    $newerVersion = Get-ExeVersion $Newer
    $csVersion = $CSharpTag.TrimStart('v')
    Write-Host "  C# $csVersion -> this build $thisVersion -> newer $newerVersion"
    Assert-That ([version]$thisVersion -gt [version]$csVersion) 'this build is newer than the C# release'
    Assert-That ([version]$newerVersion -gt [version]$thisVersion) 'the second build is newer than this one'

    Write-Step "The C# release $CSharpTag, as published"
    $zip = Join-Path $work 'cs.zip'
    $url = "https://github.com/qu1ncky/BatteryTray/releases/download/$CSharpTag/BatteryTray-$csVersion-win-x64.zip"
    & curl.exe -sSfL -o $zip $url
    if ($LASTEXITCODE -ne 0) { throw "cannot download $url" }
    $cs = Join-Path $work 'cs'
    Expand-Archive $zip $cs
    Assert-That ((Get-ExeVersion (Join-Path $cs 'BatteryTray.exe')) -eq $csVersion) "the zip has BatteryTray.exe $csVersion"

    Write-Step 'api.github.com answered from this machine'
    $cert = New-SelfSignedCertificate -DnsName 'api.github.com' -CertStoreLocation 'Cert:\LocalMachine\My' `
        -NotAfter (Get-Date).AddDays(2)
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store 'Root', 'LocalMachine'
    $store.Open('ReadWrite'); $store.Add($cert); $store.Close()
    foreach ($address in '0.0.0.0:443', '[::]:443') {
        & netsh http delete sslcert "ipport=$address" | Out-Null
        & netsh http add sslcert "ipport=$address" "certhash=$($cert.Thumbprint)" "appid=$appId" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "cannot bind the certificate to $address" }
    }
    Set-Content -Path (Join-Path $work 'serve.ps1') -Encoding ASCII -Value @'
param([string]$Folder)
$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add('https://+:443/')
$listener.Start()
while ($true) {
    $context = $listener.GetContext()
    try {
        $path = $context.Request.Url.AbsolutePath
        $file = $null
        if ($path -eq '/repos/qu1ncky/BatteryTray/releases/latest') { $file = Join-Path $Folder 'latest.json' }
        elseif ($path -like '/dl/*/BatteryTray.exe' -and $path -notlike '*..*') {
            $file = Join-Path $Folder ($path.TrimStart('/') -replace '/', '\')
        }
        $found = $file -and (Test-Path $file)
        $line = '{0:HH:mm:ss} {1} {2} by {3}' -f (Get-Date), $(if ($found) { 200 } else { 404 }), $path, $context.Request.UserAgent
        Add-Content -Path (Join-Path $Folder 'requests.log') -Value $line
        if ($found) {
            $bytes = [IO.File]::ReadAllBytes($file)
            $context.Response.ContentLength64 = $bytes.Length
            $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        }
        else { $context.Response.StatusCode = 404 }
    }
    catch { }
    finally { try { $context.Response.Close() } catch { } }
}
'@
    Set-Feed (Join-Path $cs 'BatteryTray.exe') # nothing newer yet
    $server = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy',
        'Bypass', '-File', "`"$(Join-Path $work 'serve.ps1')`"", '-Folder', "`"$feed`""
    Add-Content -Path $hosts -Encoding ASCII -Value "`r`n127.0.0.1 api.github.com`r`n::1 api.github.com"
    & ipconfig /flushdns | Out-Null
    Wait-For {
        try { (Invoke-RestMethod 'https://api.github.com/repos/qu1ncky/BatteryTray/releases/latest').tag_name -eq $CSharpTag }
        catch { Write-Host "    not yet: $_"; $false }
    } 30 'the latest release comes from the local server, over HTTPS'

    Write-Step "1. C# $csVersion, installed from its zip, updates itself to $thisVersion"
    # Settings as the C# app writes them, with values that aren't the defaults: they have to survive both updates
    New-Item -ItemType Directory -Force -Path $data | Out-Null
    Get-ChildItem $data | Remove-Item -Recurse -Force
    $seed = '{ "LowPercent": 33, "CriticalPercent": 12, "Language": "ru", "AutoUpdate": true, "OverlayEnabled": false,' +
        ' "Devices": { "razer:barracuda-x": { "Name": "Test headset", "Hidden": false, "Alerts": true } } }'
    [IO.File]::WriteAllText((Join-Path $data 'settings.json'), $seed)
    Set-Feed $Exe
    $code = Invoke-Script (Join-Path $cs 'install.ps1')
    Assert-That ($code -eq 0) 'the C# install.ps1 succeeded'
    Assert-That ((Get-ExeVersion $installed) -eq $csVersion) "Program Files has $csVersion"
    Assert-That ((Get-App).Count -ge 1) 'the C# app is running'
    $mark = 0
    Wait-For { (Get-ExeVersion $installed) -eq $thisVersion } 150 "Program Files has $thisVersion"
    Wait-For { Test-AppLog $mark "started v$thisVersion, elevated=True, updated from v$csVersion" } 60 `
        "the log says: started v$thisVersion, updated from v$csVersion"
    Set-Feed $Newer # before this build's first look for updates, 30 s after its start
    Start-Sleep -Seconds 2
    Save-Screen '1-updated-from-csharp'

    Write-Step "2. $thisVersion updates itself to $newerVersion"
    $mark = (Get-AppLog).Count
    Wait-For { (Get-ExeVersion $installed) -eq $newerVersion } 120 "Program Files has $newerVersion"
    Wait-For { Test-AppLog $mark "started v$newerVersion, elevated=True, updated from v$thisVersion" } 60 `
        "the log says: started v$newerVersion, updated from v$thisVersion"
    Start-Sleep -Seconds 2
    Save-Screen '2-updated-from-rust'
    Wait-For { (Get-App).Count -eq 1 } 30 'one copy of the app is running'
    Wait-For { @(Get-ChildItem (Split-Path $installed)).Count -eq 1 } 90 'the old exes are gone from Program Files'
    $settings = Get-Content (Join-Path $data 'settings.json') -Raw | ConvertFrom-Json
    Assert-That ($settings.LowPercent -eq 33 -and $settings.CriticalPercent -eq 12 -and $settings.Language -eq 'ru') `
        'the settings are as before the updates'
    Assert-That ($settings.Devices.'razer:barracuda-x'.Name -eq 'Test headset') "the device's name is as before"
    Assert-That ($settings.LastRunVersion -eq $newerVersion) "the settings say the last run was $newerVersion"
    Assert-That (Test-Task $installed 'Highest') 'the logon task still starts the installed exe with admin rights'

    Write-Step '3. Uninstall, then a fresh install of this build'
    $code = Invoke-Script (Join-Path $repo 'uninstall.ps1')
    Assert-That ($code -eq 0) 'uninstall.ps1 succeeded'
    Assert-That (-not (Test-Path (Split-Path $installed))) 'the Program Files folder is gone'
    Assert-That (-not (Get-ScheduledTask -TaskName 'BatteryTray' -ErrorAction SilentlyContinue)) 'the logon task is gone'
    Assert-That (-not (Test-Path $shortcut)) 'the Start menu shortcut is gone'
    Assert-That ((Get-App).Count -eq 0) 'the app is not running'

    Set-Feed $Exe # up to date: the app must not restart under the window
    $package = Join-Path $work 'package'
    New-Item -ItemType Directory -Force -Path $package | Out-Null
    Copy-Item $Exe (Join-Path $package 'BatteryTray.exe')
    Copy-Item (Join-Path $repo 'install.ps1'), (Join-Path $repo 'uninstall.ps1') $package
    $mark = (Get-AppLog).Count
    $code = Invoke-Script (Join-Path $package 'install.ps1')
    Assert-That ($code -eq 0) 'install.ps1 succeeded'
    Assert-That ((Get-ExeVersion $installed) -eq $thisVersion) "Program Files has $thisVersion"
    Wait-For { Test-AppLog $mark "started v$thisVersion, elevated=True" } 30 'the logon task started the app'
    Assert-That (Test-Task $installed 'Highest') 'the logon task starts the installed exe in the background, with admin rights'
    Assert-That (Test-Path $shortcut) 'the Start menu shortcut is there'
    Assert-That ((Get-AppWindow).Count -eq 0) 'started in the tray, no window'

    # What the Start menu shortcut does: a second launch asks the running copy for its window and exits
    Assert-That ((Invoke-Exe $installed '') -eq 0) 'a second launch exits with 0'
    Wait-For { (Get-AppWindow).Count -eq 1 } 20 'the running copy opened its window'
    Assert-That ((Get-App).Count -eq 1) 'still one copy of the app'
    Start-Sleep -Seconds 3
    Write-Host "  window title: $(Get-AppWindow)"
    Save-Screen '3-window'
    $dump = Join-Path $Out 'dump.txt'
    Assert-That ((Invoke-Exe $installed "--dump `"$dump`"" 120) -eq 0 -and (Test-Path $dump)) '--dump wrote the readings'
    Get-Content $dump | Select-Object -First 12 | ForEach-Object { Write-Host "  | $_" }
    # What a copy without admin rights has an elevated copy of itself do (behind a UAC prompt, which a runner lacks)
    Set-Feed $Newer
    Assert-That ((Invoke-Exe $installed '--update' 120) -eq 0) '--update installed the newer release'
    Assert-That ((Get-ExeVersion $installed) -eq $newerVersion) "Program Files has $newerVersion"
    Assert-That ((Invoke-Exe $installed '--update' 120) -eq 2) '--update again says there is nothing newer'
    $code = Invoke-Script (Join-Path $package 'uninstall.ps1')
    Assert-That ($code -eq 0 -and -not (Test-Path (Split-Path $installed))) 'uninstalled again'

    Write-Step "4. A portable copy: its logon task, and an update to $newerVersion"
    $portable = Join-Path $work 'portable\BatteryTray.exe'
    New-Item -ItemType Directory -Force -Path (Split-Path $portable) | Out-Null
    Copy-Item $Exe $portable
    Assert-That ((Invoke-Exe $portable '--register-task') -eq 0) '--register-task succeeded'
    Assert-That (Test-Task $portable 'Limited') 'the task starts the portable exe, without admin rights'
    Assert-That ((Invoke-Exe $portable '--unregister-task') -eq 0) '--unregister-task succeeded'
    Assert-That (-not (Get-ScheduledTask -TaskName 'BatteryTray' -ErrorAction SilentlyContinue)) 'the task is gone'
    Set-Feed $Newer
    $mark = (Get-AppLog).Count
    Start-Process $portable -ArgumentList '--background'
    Wait-For { (Get-ExeVersion $portable) -eq $newerVersion } 120 "the portable exe is $newerVersion"
    Wait-For { Test-AppLog $mark "started v$newerVersion" } 60 "the log says: started v$newerVersion"
    Wait-For { @(Get-ChildItem (Split-Path $portable)).Count -eq 1 } 90 'the old exe is gone from its folder'
    Stop-App

    Assert-That (-not ((Get-AppLog) -match 'panic')) 'no panic in the log'

    if ($Examples) {
        Write-Step '5. The windows with sample data'
        foreach ($language in 'en', 'ru', 'zh') {
            $folder = Join-Path $Out "windows-$language"
            New-Item -ItemType Directory -Force -Path $folder | Out-Null
            & (Join-Path $Examples 'snapshot.exe') $folder $language 1
            Assert-That ($LASTEXITCODE -eq 0) "drawn in $language"
            # At 150%, as the README's pictures are
            $folder = Join-Path $Out "windows-$language-150"
            New-Item -ItemType Directory -Force -Path $folder | Out-Null
            & (Join-Path $Examples 'snapshot.exe') $folder $language 1.5
            Assert-That ($LASTEXITCODE -eq 0) "drawn in $language at 150%"
        }
        $probe = Join-Path $Out 'popups.txt'
        $p = Start-Process (Join-Path $Examples 'popup_probe.exe') -PassThru -RedirectStandardOutput $probe -NoNewWindow
        Start-Sleep -Milliseconds 1000
        Save-Screen '5-popups'
        Assert-That ($p.WaitForExit(30000)) 'the popups were opened and closed'
        Get-Content $probe | Select-Object -First 4 | ForEach-Object { Write-Host "  | $_" }
        Get-Content $probe | Select-Object -Last 3 | ForEach-Object { Write-Host "  | $_" }
    }

    Write-Step 'All of it worked'
}
finally {
    Write-Step 'What the app asked GitHub for'
    if (Test-Path (Join-Path $feed 'requests.log')) { Get-Content (Join-Path $feed 'requests.log') | ForEach-Object { Write-Host "  $_" } }
    Write-Step 'The app log'
    Get-AppLog | ForEach-Object { Write-Host "  $_" }
    if (Test-Path $logFile) { Copy-Item $logFile $Out -ErrorAction SilentlyContinue }
    if (Test-Path (Join-Path $data 'settings.json')) { Copy-Item (Join-Path $data 'settings.json') $Out -ErrorAction SilentlyContinue }
    Save-Screen 'at-the-end'

    Get-App | Stop-Process -Force -ErrorAction SilentlyContinue
    if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    # The rest of the job talks to the real GitHub
    $kept = @(Get-Content $hosts | Where-Object { $_ -notmatch 'api\.github\.com' })
    Set-Content -Path $hosts -Value $kept -Encoding ASCII
    & ipconfig /flushdns | Out-Null
}
