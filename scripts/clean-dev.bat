@echo off
setlocal EnableExtensions

REM Thin CMD entry for scripts\clean-dev.ps1
REM
REM Usage:
REM   scripts\clean-dev.bat
REM   scripts\clean-dev.bat launch
REM   scripts\clean-dev.bat dryrun

set "PS_SCRIPT=%~dp0clean-dev.ps1"
set "PS_ARGS="

:parse
if "%~1"=="" goto run
if /I "%~1"=="launch" (
  set "PS_ARGS=%PS_ARGS% -Launch"
  goto next
)
if /I "%~1"=="--launch" (
  set "PS_ARGS=%PS_ARGS% -Launch"
  goto next
)
if /I "%~1"=="-Launch" (
  set "PS_ARGS=%PS_ARGS% -Launch"
  goto next
)
if /I "%~1"=="dryrun" (
  set "PS_ARGS=%PS_ARGS% -DryRun"
  goto next
)
if /I "%~1"=="--dry-run" (
  set "PS_ARGS=%PS_ARGS% -DryRun"
  goto next
)
if /I "%~1"=="-DryRun" (
  set "PS_ARGS=%PS_ARGS% -DryRun"
  goto next
)
echo [clean-dev] ERROR: Unknown option "%~1". Use: launch  ^|  dryrun
exit /b 2

:next
shift
goto parse

:run
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %PS_ARGS%
exit /b %ERRORLEVEL%
