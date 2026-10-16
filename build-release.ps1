# Builds the release: publish\BatteryTray.exe (self-contained, no .NET install needed) and
# dist\BatteryTray-<version>-win-x64.zip with the exe, the install/uninstall scripts and the docs.
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
[xml]$project = Get-Content (Join-Path $root 'BatteryTray.csproj')
$version = @($project.Project.PropertyGroup.Version)[0]

dotnet publish (Join-Path $root 'BatteryTray.csproj') -p:PublishProfile=release --nologo -v q
if ($LASTEXITCODE -ne 0) { throw 'build failed' }

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist "BatteryTray-$version"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
$files = 'publish\BatteryTray.exe', 'install.cmd', 'install.ps1', 'uninstall.cmd', 'uninstall.ps1', 'README.md',
    'README.ru.md', 'README.zh-CN.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md'
foreach ($f in $files) { Copy-Item (Join-Path $root $f) $stage }

$zip = Join-Path $dist "BatteryTray-$version-win-x64.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Copy-Item (Join-Path $root 'publish\BatteryTray.exe') $dist
Get-ChildItem $dist -File | ForEach-Object { '{0}  {1:N1} MB' -f $_.Name, ($_.Length / 1MB) }
