# One-click debug shell + Vite hot reload (does NOT use `tauri dev`).
#
# Why not `pnpm tauri:dev`: Cursor/agent shells often redirect CARGO_TARGET_DIR
# into a sandbox cache ( inflates disk / panics ). This script always builds
# into <repo>\src-tauri\target (absolute) and starts the cfg(dev) exe itself.
#
# Usage:
#   .\scripts\dev-hot.ps1
#   .\scripts\dev-hot.ps1 -Clean          # always cargo clean first
#   .\scripts\dev-hot.ps1 -MaxTargetGB 30 # auto-clean when target exceeds N GB (default 20)
#   .\scripts\dev-hot.ps1 -Port 5251      # pin Vite / baked-in devUrl port
#
# Stops leftover debug exe + this-repo Vite (ports 5250-5270) first, then starts
# Vite, cargo-builds cfg(dev), launches debug exe (CDP :9222). Ctrl+C stops Vite.

param(
    [switch]$Clean,
    [double]$MaxTargetGB = 20,
    [int]$Port = 0,
    [int]$CdpPort = 9222,
    [switch]$NoLaunch
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $root

$tauriDir = Join-Path $root "src-tauri"
$targetDir = Join-Path $tauriDir "target"
$nestedTarget = Join-Path $tauriDir "src-tauri"
$tauriConf = Join-Path $tauriDir "tauri.conf.json"
$exePath = Join-Path $targetDir "debug\claude-switcher.exe"
$defaultPort = 5250

Write-Host "[dev-hot] Project: $root"

function Get-DirSizeGB([string]$Path) {
    if (-not (Test-Path $Path)) { return 0 }
    try {
        $fso = New-Object -ComObject Scripting.FileSystemObject
        $bytes = [int64]$fso.GetFolder($Path).Size
        return [math]::Round($bytes / 1GB, 2)
    } catch {
        $sum = 0L
        Get-ChildItem $Path -Recurse -File -ErrorAction SilentlyContinue |
            ForEach-Object { $sum += $_.Length }
        return [math]::Round($sum / 1GB, 2)
    }
}

function Test-TcpPortAvailable([int]$ListenPort) {
    try {
        $listener = [System.Net.Sockets.TcpListener]::new(
            [System.Net.IPAddress]::Loopback,
            $ListenPort
        )
        $listener.Start()
        $listener.Stop()
        return $true
    } catch {
        return $false
    }
}

function Stop-PidTree([int]$ProcessId) {
    if ($ProcessId -le 4) { return }
    & taskkill.exe /F /T /PID $ProcessId 2>$null | Out-Null
}

function Get-ProcessCommandLine([int]$ProcessId) {
    try {
        $row = Get-CimInstance Win32_Process -Filter "ProcessId=$ProcessId" -ErrorAction Stop
        return [string]$row.CommandLine
    } catch {
        return ""
    }
}

function Get-ListenPids([int]$ListenPort) {
    $pids = @()
    foreach ($addr in @("127.0.0.1", "0.0.0.0", "::1", "::")) {
        try {
            $pids += @(
                Get-NetTCPConnection -LocalAddress $addr -LocalPort $ListenPort -State Listen -ErrorAction SilentlyContinue |
                    ForEach-Object { $_.OwningProcess }
            )
        } catch { }
    }
    $pids | Where-Object { $_ -and $_ -gt 0 } | Select-Object -Unique
}

function Test-IsDevHotOccupant([int]$ProcessId, [switch]$AllowViteListener) {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return $false }
    $name = $proc.ProcessName
    if ($name -in @("claude-switcher", "AISwitcher")) { return $true }
    if ($name -notin @("node", "nodejs", "pnpm", "corepack")) { return $false }
    $cmd = Get-ProcessCommandLine $ProcessId
    $rootFwd = $root.Replace("\", "/")
    $cmdFwd = $cmd.Replace("\", "/")
    $inRepo = ($rootFwd.Length -gt 0) -and ($cmdFwd.IndexOf($rootFwd, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
    if ($inRepo) { return $true }
    return [bool]($AllowViteListener -and ($cmd -match '(?i)vite'))
}

function Stop-PreviousDevHot {
    Write-Host "[dev-hot] Stopping previous debug app / Vite"

    $script:devHotKilled = @{}
    $stopOnce = {
        param([int]$ProcessId, [string]$Why)
        if ($ProcessId -le 4 -or $script:devHotKilled.ContainsKey($ProcessId)) { return }
        $script:devHotKilled[$ProcessId] = $true
        $name = "unknown"
        $existing = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if ($existing) { $name = $existing.ProcessName }
        Write-Host "[dev-hot]   stop PID $ProcessId ($name) $Why"
        Stop-PidTree $ProcessId
    }

    foreach ($procName in @("claude-switcher", "AISwitcher")) {
        Get-Process -Name $procName -ErrorAction SilentlyContinue | ForEach-Object {
            & $stopOnce $_.Id "app"
        }
    }

    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -match '^(node|nodejs|pnpm|corepack)(\.exe)?$' -and
            (Test-IsDevHotOccupant $_.ProcessId)
        } |
        ForEach-Object { & $stopOnce $_.ProcessId "vite/node" }

    foreach ($listenPort in 5250..5270) {
        foreach ($pid in (Get-ListenPids $listenPort)) {
            if (Test-IsDevHotOccupant $pid -AllowViteListener) {
                & $stopOnce $pid "listen :$listenPort"
            }
        }
    }

    $waited = 0
    while ($waited -lt 8000) {
        $stillHeld = $false
        foreach ($listenPort in 5250..5270) {
            foreach ($pid in (Get-ListenPids $listenPort)) {
                if (Test-IsDevHotOccupant $pid -AllowViteListener) {
                    $stillHeld = $true
                    break
                }
            }
            if ($stillHeld) { break }
        }
        if (-not $stillHeld) { break }
        Start-Sleep -Milliseconds 250
        $waited += 250
    }

    if ($script:devHotKilled.Count -eq 0) {
        Write-Host "[dev-hot]   no leftover app/Vite from this repo"
    }
}

function Get-DevUrlPortFromConf([string]$ConfPath) {
    $raw = Get-Content $ConfPath -Raw -Encoding UTF8
    if ($raw -match '"devUrl"\s*:\s*"https?://(?:localhost|127\.0\.0\.1):(\d+)"') {
        return [int]$Matches[1]
    }
    return $defaultPort
}

function Set-DevUrlPort([string]$ConfPath, [int]$ListenPort) {
    $raw = Get-Content $ConfPath -Raw -Encoding UTF8
    $updated = [regex]::Replace(
        $raw,
        '("devUrl"\s*:\s*"https?://)(?:localhost|127\.0\.0\.1):\d+(")',
        "`${1}localhost:$ListenPort`${2}"
    )
    if ($updated -eq $raw) {
        throw "Could not update devUrl in $ConfPath"
    }
    [System.IO.File]::WriteAllText($ConfPath, $updated)
}

# --- toolchains ---
$corepack = Get-Command corepack -ErrorAction SilentlyContinue
$pnpm = Get-Command pnpm -ErrorAction SilentlyContinue
if (-not $corepack -and -not $pnpm) {
    throw "corepack/pnpm not found. Install Node.js 22+ with Corepack."
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found. Install Rust stable (MSVC)."
}

# --- compile volume: never use relative/sandbox CARGO_TARGET_DIR ---
if ($env:CARGO_TARGET_DIR -and ($env:CARGO_TARGET_DIR -ne $targetDir)) {
    Write-Host "[dev-hot] Overriding CARGO_TARGET_DIR=$($env:CARGO_TARGET_DIR)"
}
$env:CARGO_TARGET_DIR = $targetDir

if (Test-Path $nestedTarget) {
    Write-Host "[dev-hot] Removing nested $nestedTarget (relative CARGO_TARGET_DIR leftover)"
    Remove-Item -LiteralPath $nestedTarget -Recurse -Force
}

$targetGb = Get-DirSizeGB $targetDir
Write-Host ("[dev-hot] src-tauri\target size: {0:N1} GB (auto-clean >= {1} GB)" -f $targetGb, $MaxTargetGB)

# Stop last session before cargo clean / port pick so the debug exe is not locked
# and leftover Vite does not occupy 5250.
Stop-PreviousDevHot

$needClean = [bool]$Clean
if (-not $needClean -and $MaxTargetGB -gt 0 -and $targetGb -ge $MaxTargetGB) {
    Write-Host "[dev-hot] target exceeds limit; cargo clean"
    $needClean = $true
}
if ($needClean -and (Test-Path $targetDir)) {
    Push-Location $tauriDir
    try {
        & cargo clean
        if ($LASTEXITCODE -ne 0) { throw "cargo clean failed (exit $LASTEXITCODE)." }
    } finally {
        Pop-Location
    }
}

$cargoBusy = @(Get-Process -Name "cargo", "rustc" -ErrorAction SilentlyContinue)
if ($cargoBusy.Count -gt 0) {
    $pids = ($cargoBusy | ForEach-Object { $_.Id }) -join ", "
    throw "Another cargo/rustc is running (PIDs: $pids). Stop it and retry."
}

# --- port: prefer tauri.conf / 5250; skip Hyper-V range 5141-5240 ---
$confPort = Get-DevUrlPortFromConf $tauriConf
$preferred = if ($Port -gt 0) { $Port } else { $confPort }
$vitePort = $preferred
if (-not (Test-TcpPortAvailable $vitePort)) {
    if ($Port -gt 0) {
        throw "Port $vitePort is in use. Stop the occupant or omit -Port to auto-pick."
    }
    $found = $null
    foreach ($candidate in 5251..5270) {
        if (Test-TcpPortAvailable $candidate) {
            $found = $candidate
            break
        }
    }
    if (-not $found) {
        throw "No free port in 5251-5270 (and $preferred is busy, often Cadence cdslmd on 5250)."
    }
    Write-Host "[dev-hot] Port $preferred is busy; using $found for this session (not committed)"
    $vitePort = $found
}

$originalConf = Get-Content $tauriConf -Raw -Encoding UTF8
$patchedConf = $false
if ($vitePort -ne $confPort) {
    Set-DevUrlPort $tauriConf $vitePort
    $patchedConf = $true
    Write-Host "[dev-hot] Temporarily set tauri.conf.json devUrl -> :$vitePort (restored after build)"
}

function Restore-TauriConf {
    if ($script:patchedConf) {
        [System.IO.File]::WriteAllText($script:tauriConf, $script:originalConf)
        $script:patchedConf = $false
        Write-Host "[dev-hot] Restored tauri.conf.json devUrl"
    }
}

# Bind script-scope for Restore
$script:patchedConf = $patchedConf
$script:tauriConf = $tauriConf
$script:originalConf = $originalConf

$viteProc = $null
try {
    Write-Host "[dev-hot] Starting Vite on 127.0.0.1:$vitePort"
    $viteArgs = if ($vitePort -eq $confPort -and $Port -eq 0) {
        @("dev")
    } else {
        @("exec", "vite", "--port", "$vitePort", "--strictPort", "--host", "127.0.0.1")
    }
    if ($corepack) {
        $viteProc = Start-Process -FilePath $corepack.Path -ArgumentList (@("pnpm") + $viteArgs) `
            -WorkingDirectory $root -PassThru -WindowStyle Hidden
    } else {
        $viteProc = Start-Process -FilePath $pnpm.Path -ArgumentList $viteArgs `
            -WorkingDirectory $root -PassThru -WindowStyle Hidden
    }

    $ready = $false
    foreach ($i in 1..60) {
        if ($viteProc.HasExited) {
            throw "Vite exited early (code $($viteProc.ExitCode)). Is port $vitePort blocked?"
        }
        try {
            $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$vitePort" -UseBasicParsing -TimeoutSec 1
            if ($resp.StatusCode -ge 200) {
                $ready = $true
                break
            }
        } catch { }
        Start-Sleep -Milliseconds 500
    }
    if (-not $ready) {
        throw "Vite did not become ready on http://127.0.0.1:$vitePort"
    }
    Write-Host "[dev-hot] Vite ready: http://127.0.0.1:$vitePort/"

    Write-Host "[dev-hot] cargo build --cfg dev  (CARGO_TARGET_DIR=$targetDir)"
    Push-Location $tauriDir
    $previousEncodedRustflags = $env:CARGO_ENCODED_RUSTFLAGS
    try {
        # Avoid PowerShell/native argument quote loss in Cargo's --config parser.
        $env:CARGO_ENCODED_RUSTFLAGS = "--cfg$([char]0x1f)dev"
        & cargo build
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)." }
    } finally {
        $env:CARGO_ENCODED_RUSTFLAGS = $previousEncodedRustflags
        Pop-Location
    }

    Restore-TauriConf

    if (-not (Test-Path $exePath)) {
        throw "Missing $exePath"
    }

    if ($NoLaunch) {
        Write-Host "[dev-hot] Built. Skip launch (-NoLaunch). Vite still running (PID $($viteProc.Id))."
        return
    }

    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort"
    Write-Host "[dev-hot] Launch $exePath  (CDP :$CdpPort)"
    Start-Process -FilePath $exePath

    Write-Host ""
    Write-Host "[dev-hot] Hot reload is up. Edit frontend -> Vite HMR. Edit Rust -> re-run this script."
    Write-Host "[dev-hot] Ctrl+C stops Vite. The desktop window keeps running."
    Wait-Process -Id $viteProc.Id
} finally {
    Restore-TauriConf
    if ($viteProc -and -not $viteProc.HasExited) {
        Stop-PidTree $viteProc.Id
    }
}
