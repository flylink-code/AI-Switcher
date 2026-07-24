@echo off
REM Run a pnpm/tauri command inside the MSVC build environment so cargo (invoked
REM by the tauri CLI) can link against the Windows SDK.
REM
REM Usage:
REM   tauri-msvc.bat dev        -> pnpm tauri dev
REM   tauri-msvc.bat build      -> pnpm tauri build
REM   tauri-msvc.bat <args...>  -> pnpm tauri <args...>
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

set "TAURI_CLI_ARGS=%*"
if "%~1"=="" (
  pnpm tauri dev
) else (
  pnpm tauri %*
)
