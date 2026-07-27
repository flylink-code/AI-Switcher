@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM Run the local Tauri CLI inside an available Visual Studio 2022 x64 environment.
REM
REM Usage:
REM   tauri-msvc.bat             -> tauri dev
REM   tauri-msvc.bat dev
REM   tauri-msvc.bat build --no-bundle --ci

set "VCVARS="
if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat"
if not defined VCVARS if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" set "VCVARS=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

if not defined VCVARS (
  echo [tauri-msvc] ERROR: vcvars64.bat not found. Install VS 2022 with Desktop development with C++.
  exit /b 1
)

call "%VCVARS%" >nul
if errorlevel 1 (
  echo [tauri-msvc] ERROR: Failed to initialize the MSVC environment.
  exit /b 1
)

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>&1
if errorlevel 1 (
  echo [tauri-msvc] ERROR: cargo not found. Install the Rust stable MSVC toolchain.
  exit /b 1
)

if not defined RUSTFLAGS (
  set "RUSTFLAGS=-A linker-messages"
) else if "!RUSTFLAGS:linker-messages=!"=="!RUSTFLAGS!" (
  set "RUSTFLAGS=!RUSTFLAGS! -A linker-messages"
)

set "TAURI_ARGS=%*"
if "%~1"=="" set "TAURI_ARGS=dev"

where pnpm >nul 2>&1
if not errorlevel 1 (
  call pnpm exec tauri %TAURI_ARGS%
  set "TAURI_RC=%ERRORLEVEL%"
  exit /b %TAURI_RC%
)

where corepack >nul 2>&1
if errorlevel 1 (
  echo [tauri-msvc] ERROR: neither pnpm nor corepack was found. Install Node.js 20+ with Corepack.
  exit /b 1
)

call corepack pnpm exec tauri %TAURI_ARGS%
set "TAURI_RC=%ERRORLEVEL%"
exit /b %TAURI_RC%
