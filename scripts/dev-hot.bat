@echo off
setlocal EnableExtensions

REM Thin CMD entry for scripts\dev-hot.ps1
REM
REM Usage:
REM   scripts\dev-hot.bat
REM   scripts\dev-hot.bat clean
REM   scripts\dev-hot.bat 5251
REM   scripts\dev-hot.bat maxgb=30
REM Auto-clean when src-tauri\target >= 20 GB (override with maxgb=N).

set "PS_SCRIPT=%~dp0dev-hot.ps1"
set "PS_ARGS="

:parse
if "%~1"=="" goto run
if /I "%~1"=="clean" (
  set "PS_ARGS=%PS_ARGS% -Clean"
  goto next
)
if /I "%~1"=="--clean" (
  set "PS_ARGS=%PS_ARGS% -Clean"
  goto next
)
echo %~1| findstr /I /R "^maxgb=" >nul
if not errorlevel 1 (
  for /f "tokens=2 delims==" %%A in ("%~1") do set "PS_ARGS=%PS_ARGS% -MaxTargetGB %%A"
  goto next
)
echo %~1| findstr /R "^[0-9][0-9]*$" >nul
if not errorlevel 1 (
  set "PS_ARGS=%PS_ARGS% -Port %~1"
  goto next
)
echo [dev-hot] ERROR: Unknown option "%~1". Use: clean  ^|  ^<port^>  ^|  maxgb=N
exit /b 2

:next
shift
goto parse

:run
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %PS_ARGS%
exit /b %ERRORLEVEL%
