@echo off
setlocal EnableExtensions

REM Thin CMD entry for scripts\stop-dev.ps1
REM
REM Usage:
REM   scripts\stop-dev.bat
REM   scripts\stop-dev.bat apponly

set "PS_SCRIPT=%~dp0stop-dev.ps1"
set "PS_ARGS="

if /I "%~1"=="apponly" set "PS_ARGS=-AppOnly"
if /I "%~1"=="--app-only" set "PS_ARGS=-AppOnly"
if /I "%~1"=="-AppOnly" set "PS_ARGS=-AppOnly"

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %PS_ARGS%
exit /b %ERRORLEVEL%
