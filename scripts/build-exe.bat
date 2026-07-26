@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM Quick local EXE build for testing (skips MSI/NSIS bundling).
REM
REM Usage:
REM   scripts\build-exe.bat          Release exe (recommended for testing)
REM   scripts\build-exe.bat debug    Debug exe (faster compile, larger binary)
REM   scripts\build-exe.bat bundle   Full release + MSI/NSIS installers

cd /d "%~dp0.."

set "MODE=release"
if /I "%~1"=="debug" set "MODE=debug"
if /I "%~1"=="bundle" set "MODE=bundle"

echo [build-exe] Claude Switcher local build (%MODE%)
echo [build-exe] Project: %CD%

set "VCVARS="
for %%P in (
  "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
  "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat"
  "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat"
  "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
) do (
  if exist %%P (
    set "VCVARS=%%~P"
    goto :found_vcvars
  )
)

echo [build-exe] ERROR: vcvars64.bat not found. Install VS 2022 with the Desktop development with C++ workload.
exit /b 1

:found_vcvars
echo [build-exe] MSVC env: !VCVARS!
call "!VCVARS!" >nul
if errorlevel 1 (
  echo [build-exe] ERROR: Failed to initialize MSVC environment.
  exit /b 1
)

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

where pnpm >nul 2>&1
if errorlevel 1 (
  echo [build-exe] ERROR: pnpm not found. Install Node.js 20+ and run: corepack enable
  exit /b 1
)

where cargo >nul 2>&1
if errorlevel 1 (
  echo [build-exe] ERROR: cargo not found. Install Rust stable MSVC target.
  exit /b 1
)

if not exist "node_modules\.bin\tauri.cmd" (
  echo [build-exe] @tauri-apps/cli not found; running pnpm install
  call pnpm install
  if errorlevel 1 (
    echo [build-exe] ERROR: pnpm install failed.
    exit /b 1
  )
  if not exist "node_modules\.bin\tauri.cmd" (
    echo [build-exe] ERROR: tauri CLI still missing after pnpm install.
    exit /b 1
  )
)
set "PATH=%CD%\node_modules\.bin;%PATH%"

set "TAURI_ARGS=build --ci"
if /I "%MODE%"=="debug" (
  set "TAURI_ARGS=build --debug --no-bundle --ci"
) else if /I "%MODE%"=="bundle" (
  set "TAURI_ARGS=build --ci"
) else (
  set "TAURI_ARGS=build --no-bundle --ci"
)

echo [build-exe] Running: pnpm exec tauri %TAURI_ARGS%
echo.

set "START=%TIME%"
call pnpm exec tauri %TAURI_ARGS%
set "BUILD_RC=%ERRORLEVEL%"
set "END=%TIME%"

if not "%BUILD_RC%"=="0" (
  echo.
  echo [build-exe] ERROR: Build failed (exit %BUILD_RC%).
  exit /b %BUILD_RC%
)

if /I "%MODE%"=="debug" (
  set "EXE_SRC=src-tauri\target\debug\claude-switcher.exe"
) else (
  set "EXE_SRC=src-tauri\target\release\claude-switcher.exe"
)

if not exist "%EXE_SRC%" (
  echo [build-exe] ERROR: Expected binary not found: %EXE_SRC%
  exit /b 1
)

if not exist "release" mkdir "release"
set "EXE_DST=release\claude-switcher-%MODE%.exe"
copy /Y "%EXE_SRC%" "%EXE_DST%" >nul

echo.
echo [build-exe] Done (%START% - %END%)
echo [build-exe] Binary : %CD%\%EXE_SRC%
echo [build-exe] Copied  : %CD%\%EXE_DST%
echo.
echo Run for testing:
echo   "%EXE_DST%"
echo.

if /I "%MODE%"=="bundle" (
  echo Installers:
  if exist "src-tauri\target\release\bundle\nsis" dir /b "src-tauri\target\release\bundle\nsis\*.exe" 2>nul
  if exist "src-tauri\target\release\bundle\msi" dir /b "src-tauri\target\release\bundle\msi\*.msi" 2>nul
)

endlocal
exit /b 0
