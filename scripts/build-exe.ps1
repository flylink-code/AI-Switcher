# Quick local EXE build for testing (skips MSI/NSIS bundling by default).
#
# Usage:
#   .\scripts\build-exe.ps1           Release exe (recommended)
#   .\scripts\build-exe.ps1 -Debug    Debug exe (faster compile)
#   .\scripts\build-exe.ps1 -Bundle   Full release + installers
#   .\scripts\build-exe.ps1 -SkipTests
#                                      Skip Rust tests for a faster local build

param(
    [switch]$Debug,
    [switch]$Bundle,
    [switch]$Clean,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
# This script is a non-interactive build entry point. pnpm may otherwise abort
# when it needs to refresh node_modules without a TTY.
$env:CI = "true"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

$mode = if ($Debug) { "debug" } elseif ($Bundle) { "bundle" } else { "release" }
Write-Host "[build-exe] Claude Switcher local build ($mode)"
Write-Host "[build-exe] Project: $root"

# vcvars64.bat appends several toolchain directories to PATH.  CMD cannot process
# an environment variable longer than about 8191 characters, so preserve the
# executable locations we need and start vcvars from a compact PATH.
$nodeCommand = Get-Command node -ErrorAction SilentlyContinue
if (-not $nodeCommand) {
    throw "node not found. Install Node.js 20+."
}
$packageManagerPrefix = @()
$corepackCommand = Get-Command corepack -ErrorAction SilentlyContinue
if ($corepackCommand) {
    $packageManagerCommand = $corepackCommand
    $packageManagerPrefix = @("pnpm")
    Write-Host "[build-exe] Using Corepack pnpm from packageManager"
} else {
    $packageManagerCommand = Get-Command pnpm -ErrorAction SilentlyContinue
    if (-not $packageManagerCommand) {
        throw "Neither pnpm nor corepack was found. Install Node.js 20+ with Corepack."
    }
    Write-Host "[build-exe] Corepack not found; using pnpm from PATH"
}
$packageManagerPath = $packageManagerCommand.Path
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCommand) {
    throw "cargo not found. Install Rust stable MSVC target."
}

$toolDirs = @($packageManagerPath, $nodeCommand.Path, $cargoCommand.Path) |
    ForEach-Object { Split-Path -Parent $_ } |
    Select-Object -Unique
$system32 = Join-Path $env:SystemRoot "System32"
$cargoBin = Join-Path $env:USERPROFILE ".cargo\\bin"
$env:PATH = (@($system32, $env:SystemRoot) + $toolDirs) -join ";"

$vcvarsCandidates = @(
    "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
    "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat",
    "${env:ProgramFiles}\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat",
    "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
)

$vcvars = $vcvarsCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vcvars) {
    throw "vcvars64.bat not found. Install VS 2022 with Desktop development with C++."
}

Write-Host "[build-exe] MSVC env: $vcvars"
$vcvarsOutput = cmd /c "`"$vcvars`" >nul && set"
$vcvarsExitCode = $LASTEXITCODE
if ($vcvarsExitCode -ne 0) {
    throw "MSVC environment initialization failed (exit $vcvarsExitCode). PATH was shortened for vcvars64.bat; check the Visual Studio C++ workload installation."
}
$vcvarsOutput | ForEach-Object {
    if ($_ -match "^(.*?)=(.*)$") {
        Set-Item -Path "env:$($matches[1])" -Value $matches[2]
    }
}

$env:PATH = (($toolDirs + $cargoBin | Select-Object -Unique) -join ";") + ";$env:PATH"

# MSVC prints a localized "creating import library" status line while linking
# the cdylib target. Rust reports that harmless stdout as `linker_messages`.
# Preserve caller-provided flags and suppress only that specific lint.
$hasLinkerMessageFlag =
    $env:RUSTFLAGS -match "(^|\s)-A\s+linker-messages(\s|$)" -or
    $env:RUSTFLAGS -match "(^|\s)-Alinker-messages(\s|$)"
if (-not $hasLinkerMessageFlag) {
    $env:RUSTFLAGS = (@($env:RUSTFLAGS, "-A linker-messages") |
        Where-Object { $_ } |
        ForEach-Object { $_.Trim() }) -join " "
}

$nodeBin = Join-Path $root "node_modules\.bin"
$tauriCli = Join-Path $nodeBin "tauri.cmd"
if (-not (Test-Path $tauriCli)) {
    Write-Host "[build-exe] @tauri-apps/cli not found; running pnpm install"
    & $packageManagerPath @packageManagerPrefix install
    if ($LASTEXITCODE -ne 0) {
        throw "pnpm install failed (exit $LASTEXITCODE)."
    }
    if (-not (Test-Path $tauriCli)) {
        throw "tauri CLI still missing after pnpm install: $tauriCli`nRun: pnpm install"
    }
}
$env:PATH = "$nodeBin;$env:PATH"

$tauriDir = Join-Path $root "src-tauri"
$profile = if ($Debug) { "debug" } else { "release" }
$targetDir = Join-Path $tauriDir "target"

# Cursor/agent shells may set CARGO_TARGET_DIR to a sandbox cache. Always build into the project.
if ($env:CARGO_TARGET_DIR -and ($env:CARGO_TARGET_DIR -ne $targetDir)) {
    Write-Host "[build-exe] Overriding CARGO_TARGET_DIR=$($env:CARGO_TARGET_DIR)"
}
$env:CARGO_TARGET_DIR = $targetDir

$cargoProcs = @(Get-Process -Name "cargo", "rustc" -ErrorAction SilentlyContinue)
if ($cargoProcs.Count -gt 0) {
    $pids = ($cargoProcs | ForEach-Object { $_.Id }) -join ", "
    throw @"
Another cargo/rustc build is already running (PIDs: $pids).
Stop it (or close other terminals running tauri dev / build) and retry.
Concurrent builds can corrupt src-tauri\target and cause os error 3.
"@
}

if ($Clean -or (-not (Test-Path $targetDir))) {
    Write-Host "[build-exe] cargo clean (target missing or -Clean requested)"
    Push-Location $tauriDir
    try {
        & cargo clean
        if ($LASTEXITCODE -ne 0) {
            throw "cargo clean failed (exit $LASTEXITCODE)."
        }
    } finally {
        Pop-Location
    }
}

if (-not $SkipTests) {
    Write-Host "[build-exe] Running: cargo test"
    Push-Location $tauriDir
    try {
        & $cargoCommand.Path test
        if ($LASTEXITCODE -ne 0) {
            throw "Rust tests failed (exit $LASTEXITCODE)."
        }
    } finally {
        Pop-Location
    }
    Write-Host ""
} else {
    Write-Host "[build-exe] Rust tests skipped by -SkipTests"
    Write-Host ""
}

$tauriArgs = if ($Debug) {
    @("build", "--debug", "--no-bundle", "--ci")
} elseif ($Bundle) {
    @("build", "--ci")
} else {
    @("build", "--no-bundle", "--ci")
}

Write-Host "[build-exe] Running: pnpm exec tauri $($tauriArgs -join ' ')"
Write-Host ""

$sw = [System.Diagnostics.Stopwatch]::StartNew()
& $packageManagerPath @packageManagerPrefix exec tauri @tauriArgs
if ($LASTEXITCODE -ne 0) {
    throw "Build failed (exit $LASTEXITCODE)."
}
$sw.Stop()

$exeSrc = Join-Path $targetDir "$profile\ClaudeSwitch.exe"
if (-not (Test-Path $exeSrc)) {
    throw "Expected binary not found: $exeSrc"
}

$releaseDir = Join-Path $root "release"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
$outputName = if ($mode -eq "debug") { "ClaudeSwitch-debug.exe" } else { "ClaudeSwitch.exe" }
$exeDst = Join-Path $releaseDir $outputName
$copiedPath = $exeDst
$targetProcesses = @()

if (Test-Path -LiteralPath $exeDst) {
    $resolvedDestination = [System.IO.Path]::GetFullPath($exeDst)
    $targetProcesses = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try {
            $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -eq $resolvedDestination)
        } catch {
            $false
        }
    })
}

if ($targetProcesses.Count -gt 0) {
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $copiedPath = Join-Path $releaseDir "ClaudeSwitch-$mode-$timestamp.exe"
    Copy-Item -LiteralPath $exeSrc -Destination $copiedPath

    $processSummary = ($targetProcesses | ForEach-Object {
        "$($_.ProcessName) (PID $($_.Id))"
    }) -join ", "
    Write-Warning "The existing output is running and cannot be replaced: $exeDst"
    Write-Warning "Locking process: $processSummary"
    Write-Warning "The new build was copied to: $copiedPath"
    Write-Warning "Close the running app and rebuild to refresh the standard release file."
} else {
    try {
        Copy-Item -LiteralPath $exeSrc -Destination $exeDst -Force
    } catch {
        $destinationLocked = $false
        if (Test-Path -LiteralPath $exeDst) {
            try {
                $probe = [System.IO.File]::Open(
                    $exeDst,
                    [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::ReadWrite,
                    [System.IO.FileShare]::None
                )
                $probe.Dispose()
            } catch [System.IO.IOException] {
                $destinationLocked = $true
            }
        }

        if (-not $destinationLocked) {
            throw
        }

        $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
        $copiedPath = Join-Path $releaseDir "ClaudeSwitch-$mode-$timestamp.exe"
        Copy-Item -LiteralPath $exeSrc -Destination $copiedPath
        Write-Warning "The existing output is locked by another process: $exeDst"
        Write-Warning "The new build was copied to: $copiedPath"
        Write-Warning "Close the process using the standard release file and rebuild to replace it."
    }
}

Write-Host ""
Write-Host "[build-exe] Done in $($sw.Elapsed.ToString('mm\:ss'))"
Write-Host "[build-exe] Binary : $exeSrc"
Write-Host "[build-exe] Copied  : $copiedPath"
Write-Host ""
Write-Host "Run for testing:"
Write-Host "  `"$copiedPath`""

if ($Bundle) {
    Write-Host ""
    Write-Host "Installers:"
    Get-ChildItem -Path (Join-Path $root "src-tauri\target\release\bundle") -Recurse -Include *.exe, *.msi -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Host "  $($_.FullName)" }
}
