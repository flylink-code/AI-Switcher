@echo off
setlocal EnableExtensions

REM Thin CMD entry for scripts\dev-hot.ps1
REM
REM Usage:
REM   scripts\dev-hot.bat
REM   scripts\dev-hot.bat clean
REM   scripts\dev-hot.bat 5251

set "PS_SCRIPT=%~dp0dev-hot.ps1"
set "PS_ARGS="

:parse
if "%~1"=="" goto run
if /I "%~1"=="clean" (
  set "PS_ARGS=%PS_ARGS% -Clean"
) else if /I "%~1"=="--clean" (
  set "PS_ARGS=%PS_ARGS% -Clean"
) else (
  echo %~1| findstr /R "^[0-9][0-9]*$" >nul
  if not errorlevel 1 (
    set "PS_ARGS=%PS_ARGS% -Port %~1"
  ) else (
    echo [dev-hot] ERROR: Unknown option "%~1". Use: clean  ^|  ^<port^>
    exit /b 2
  )
)
shift
goto parse

:run
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %PS_ARGS%
exit /b %ERRORLEVEL%
