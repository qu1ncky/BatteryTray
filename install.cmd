@echo off
rem Installs or updates BatteryTray (asks for administrator rights once).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1"
pause
