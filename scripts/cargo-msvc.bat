@echo off
REM Run a cargo command inside the MSVC build environment.
REM Usage: cargo-msvc.bat <cargo args...>
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
cargo %*
