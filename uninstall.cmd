@echo off
rem Removes BatteryTray (asks for administrator rights once). Settings and log are kept.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1"
pause
