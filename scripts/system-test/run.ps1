# L2 system-test runner (Windows). Isolates HOME, builds debug (dist, not cfg(dev)),
# launches with CDP, runs IPC scenarios, then clean-dev.
#
# Usage:
#   .\scripts\system-test\run.ps1
#   .\scripts\system-test\run.ps1 -SkipBuild
#   .\scripts\system-test\run.ps1 -Scenario SG-regress-no-autobind
#   .\scripts\system-test\run.ps1 -ClaudeCode
#   .\scripts\system-test\run.ps1 -KeepHome   # keep AISW_TEST_HOME even on success
#
# Never writes the user's real ~/.claude or ~/.claude-switcher.

param(
    [switch]$SkipBuild,
    [switch]$SkipClean,
    [switch]$ClaudeCode,
    [switch]$KeepHome,
    [string]$Scenario = "*",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Remaining
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$tauriDir = Join-Path $root "src-tauri"
$targetDir = Join-Path $tauriDir "target"
$exePath = Join-Path $targetDir "debug\claude-switcher.exe"
$distIndex = Join-Path $root "dist\index.html"
$artifacts = Join-Path $PSScriptRoot "artifacts"
$stopDev = Join-Path $root "scripts\stop-dev.ps1"
$cleanDev = Join-Path $root "scripts\clean-dev.ps1"

function Get-FreePort([int]$Start, [int]$Span = 40) {
    for ($port = $Start; $port -lt ($Start + $Span); $port++) {
        try {
            $listener = [System.Net.Sockets.TcpListener]::new(
                [System.Net.IPAddress]::Loopback,
                $port
            )
            $listener.Start()
            $listener.Stop()
            return $port
        } catch { }
    }
    throw "No free TCP port in $Start..$($Start + $Span - 1)"
}

function Test-LoopbackListening([int]$ListenPort) {
    try {
        $rows = @(Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort $ListenPort -State Listen -ErrorAction SilentlyContinue)
        return $rows.Count -gt 0
    } catch {
        return $false
    }
}

function Save-FailedHome([string]$IsolatedHome, [string]$Reason) {
    New-Item -ItemType Directory -Force -Path $artifacts | Out-Null
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dest = Join-Path $artifacts $stamp
    Write-Host "[system-test] Keeping $IsolatedHome -> $dest ($Reason)"
    Copy-Item -LiteralPath $IsolatedHome -Destination $dest -Recurse -Force -ErrorAction SilentlyContinue
    $log = Join-Path $artifacts "$stamp.txt"
    Set-Content -LiteralPath $log -Value $Reason -Encoding UTF8
}

function Get-AutostartCommand {
    try {
        return [string](Get-ItemPropertyValue -LiteralPath "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "AI-Switcher" -ErrorAction Stop)
    } catch {
        return $null
    }
}

if ($Remaining) {
    $leftover = @($Remaining | Where-Object { $_ -and $_ -ne "--" })
    if ($leftover.Count -gt 0) {
        throw "Unexpected arguments: $($leftover -join ' ')"
    }
}

Write-Host "[system-test] Project: $root"
Set-Location $root

if (-not (Test-Path $distIndex)) {
    Write-Host "[system-test] dist/ missing; running pnpm build"
    corepack pnpm build
    if ($LASTEXITCODE -ne 0) { throw "pnpm build failed" }
}

Write-Host "[system-test] Stopping running app so the isolated instance can own single-instance + CDP"
& $stopDev
if ($LASTEXITCODE -ne 0) {
    Write-Host "[system-test] stop-dev returned $LASTEXITCODE; continuing"
}

$testHome = Join-Path $env:TEMP ("aisw-system-test-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $testHome | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".claude") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".claude-switcher") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".config\opencode") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".codex") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".dsh") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $testHome ".pi\agent") | Out-Null

$cdpPort = Get-FreePort 9222
$gatewayPort = Get-FreePort 16828
$proxyBase = Get-FreePort 16821

$env:AISW_TEST_HOME = $testHome
$env:AISW_ALLOW_TEST_HOME = "1"
$env:AISW_SMART_GATEWAY_PORT = "$gatewayPort"
$env:AISW_PROXY_PORT_BASE = "$proxyBase"
$env:AISW_CDP_PORT = "$cdpPort"
$env:AISW_SCENARIO = $Scenario
$env:CODEX_HOME = Join-Path $testHome ".codex"
$env:OPENCODE_CONFIG = Join-Path $testHome ".config\opencode\opencode.json"
$env:OPENCODE_DB = Join-Path $testHome ".local\share\opencode\opencode.db"
$env:DSH_HOME = Join-Path $testHome ".dsh"
$env:PI_CODING_AGENT_DIR = Join-Path $testHome ".pi\agent"
$env:XDG_DATA_HOME = Join-Path $testHome ".local\share"
$env:CARGO_TARGET_DIR = $targetDir
Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue

Write-Host "[system-test] AISW_TEST_HOME=$testHome"
Write-Host "[system-test] CDP=$cdpPort gateway=$gatewayPort proxyBase=$proxyBase"

$failed = $false
$failReason = ""
try {
    if (-not $SkipBuild -or -not (Test-Path $exePath)) {
        Write-Host "[system-test] cargo build (debug, no --cfg dev; loads dist/)"
        Push-Location $tauriDir
        try {
            & cargo build
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
        } finally {
            Pop-Location
        }
    }

    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$cdpPort"
    $proc = Start-Process -FilePath $exePath -WorkingDirectory $root -PassThru
    if (-not $proc) { throw "failed to launch $exePath" }
    Write-Host "[system-test] launched PID $($proc.Id)"

    $ready = $false
    foreach ($i in 1..45) {
        if ($proc.HasExited) {
            throw "debug exe exited early (code $($proc.ExitCode))"
        }
        if (Test-LoopbackListening $cdpPort) {
            $ready = $true
            break
        }
        Start-Sleep -Milliseconds 400
    }
    if (-not $ready) {
        throw "CDP :$cdpPort did not come up"
    }

    Write-Host "[system-test] running L2 scenarios"
    & node (Join-Path $PSScriptRoot "run-scenarios.mjs")
    if ($LASTEXITCODE -ne 0) {
        $failed = $true
        $failReason = "scenarios exited $LASTEXITCODE"
    }

    if ($ClaudeCode -and -not $failed) {
        & (Join-Path $PSScriptRoot "optional-claude-code.ps1")
        if ($LASTEXITCODE -ne 0) {
            $failed = $true
            $failReason = "optional Claude Code exited $LASTEXITCODE"
        }
    }
} catch {
    $failed = $true
    $failReason = "$_"
    Write-Host "[system-test] ERROR: $_"
} finally {
    if ($failed -or $KeepHome) {
        Save-FailedHome $testHome $(if ($failed) { $failReason } else { "KeepHome" })
    }
    if (-not $SkipClean) {
        Write-Host "[system-test] clean-dev"
        & $cleanDev
        $auto = Get-AutostartCommand
        if ($auto -and $auto -match "src-tauri\\target\\debug\\claude-switcher") {
            Write-Host "[system-test] HKCU Run still points at debug exe: $auto"
            if (-not $failed) {
                $failed = $true
                $failReason = "debug autostart left behind"
            }
        }
    }
    if (-not $KeepHome -and -not $failed -and (Test-Path $testHome)) {
        Remove-Item -LiteralPath $testHome -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($failed) {
    Write-Host "[system-test] FAILED: $failReason"
    exit 1
}
Write-Host "[system-test] OK"
exit 0
