@echo off
setlocal EnableExtensions

REM Thin CMD entry point for scripts\build-exe.ps1.
REM
REM Usage:
REM   scripts\build-exe.bat
REM   scripts\build-exe.bat release
REM   scripts\build-exe.bat debug
REM   scripts\build-exe.bat bundle
REM   scripts\build-exe.bat release skip-tests
REM   scripts\build-exe.bat debug skip-tests
REM   scripts\build-exe.bat bundle skip-tests

set "PS_SCRIPT=%~dp0build-exe.ps1"
set "PS_ARGS="

if /I "%~1"=="debug" (
  set "PS_ARGS=-Debug"
  shift
) else if /I "%~1"=="bundle" (
  set "PS_ARGS=-Bundle"
  shift
) else if /I "%~1"=="release" (
  shift
) else if not "%~1"=="" (
  echo [build-exe] ERROR: Unknown mode "%~1". Use release, debug, or bundle.
  exit /b 2
)

:parse_options
if "%~1"=="" goto run
if "%~1"=="--" (
  REM pnpm forwards its conventional argument separator to batch scripts.
) else if /I "%~1"=="skip-tests" (
  set "PS_ARGS=%PS_ARGS% -SkipTests"
) else if /I "%~1"=="clean" (
  set "PS_ARGS=%PS_ARGS% -Clean"
) else (
  echo [build-exe] ERROR: Unknown option "%~1". Use skip-tests or clean.
  exit /b 2
)
shift
goto parse_options

:run
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %PS_ARGS%
set "BUILD_RC=%ERRORLEVEL%"
exit /b %BUILD_RC%
