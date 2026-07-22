@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0restore-cursor.ps1"
exit /b %ERRORLEVEL%
