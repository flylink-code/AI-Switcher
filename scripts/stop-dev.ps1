# Stop the running AI-Switcher debug/installed app and this-repo Vite leftover.
#
# Kills both claude-switcher (debug) and AISwitcher (installed) so hot-reload
# can bind the same ports / single-instance mutex. After a test session, use
# scripts\clean-dev.ps1 instead — that keeps the installed app and restores
# autostart away from the debug exe.
#
# Does not kill Cadence cdslmd or unrelated Node. Does not kill cargo/rustc.
#
# Usage:
#   .\scripts\stop-dev.ps1
#   .\scripts\stop-dev.ps1 -AppOnly   # exe only, leave Vite running

param(
    [switch]$AppOnly
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$vitePortMin = 5250
$vitePortMax = 5270

Write-Host "[stop-dev] Project: $root"

function Stop-PidTree([int]$ProcessId) {
    if ($ProcessId -le 4) { return }
    # taskkill can stall on a dying GUI; cap wait so hot-reload is not stuck on "closing".
    $proc = Start-Process -FilePath "taskkill.exe" -ArgumentList @("/F", "/T", "/PID", "$ProcessId") `
        -WindowStyle Hidden -PassThru
    if (-not $proc.WaitForExit(8000)) {
        Write-Host "[stop-dev]   taskkill PID $ProcessId timed out; continuing"
        try { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue } catch { }
    }
}

function Get-ProcessCommandLine([int]$ProcessId) {
    try {
        $row = Get-CimInstance Win32_Process -Filter "ProcessId=$ProcessId" -ErrorAction Stop
        return [string]$row.CommandLine
    } catch {
        return ""
    }
}

# One netstat parse. Per-port Get-NetTCPConnection (21 ports x 4 addresses, then
# again in the wait loop) routinely hangs for minutes on Windows.
function Get-ListenPidMap {
    $map = @{}
    $lines = @()
    try {
        $lines = & netstat.exe -ano -p tcp 2>$null
    } catch {
        return $map
    }
    foreach ($line in $lines) {
        if ($line -notmatch '^\s*TCP\s+(\S+)\s+\S+\s+LISTENING\s+(\d+)\s*$') { continue }
        $local = $Matches[1]
        $procId = 0
        if (-not [int]::TryParse($Matches[2], [ref]$procId) -or $procId -le 0) { continue }
        $colon = $local.LastIndexOf(':')
        if ($colon -lt 0) { continue }
        $port = 0
        if (-not [int]::TryParse($local.Substring($colon + 1), [ref]$port)) { continue }
        if ($port -lt $vitePortMin -or $port -gt $vitePortMax) { continue }
        if (-not $map.ContainsKey($port)) { $map[$port] = @() }
        if ($map[$port] -notcontains $procId) { $map[$port] += $procId }
    }
    return $map
}

function Test-IsOurProcess([int]$ProcessId, [switch]$AllowViteListener) {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return $false }
    $name = $proc.ProcessName
    if ($name -in @("claude-switcher", "AISwitcher")) { return $true }
    if ($AppOnly) { return $false }
    if ($name -notin @("node", "nodejs", "pnpm", "corepack")) { return $false }
    $cmd = Get-ProcessCommandLine $ProcessId
    $rootFwd = $root.Replace("\", "/")
    $cmdFwd = $cmd.Replace("\", "/")
    $inRepo = ($rootFwd.Length -gt 0) -and ($cmdFwd.IndexOf($rootFwd, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
    if ($inRepo) { return $true }
    return [bool]($AllowViteListener -and ($cmd -match '(?i)vite'))
}

$script:stopped = @{}
function Invoke-StopOnce([int]$ProcessId, [string]$Why) {
    if ($ProcessId -le 4 -or $script:stopped.ContainsKey($ProcessId)) { return }
    $script:stopped[$ProcessId] = $true
    $name = "unknown"
    $existing = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($existing) { $name = $existing.ProcessName }
    Write-Host "[stop-dev]   stop PID $ProcessId ($name) $Why"
    Stop-PidTree $ProcessId
}

Write-Host "[stop-dev] Stopping running AI-Switcher / Vite"

foreach ($procName in @("claude-switcher", "AISwitcher")) {
    Get-Process -Name $procName -ErrorAction SilentlyContinue | ForEach-Object {
        Invoke-StopOnce $_.Id "app"
    }
}

if (-not $AppOnly) {
    Write-Host "[stop-dev] Checking leftover Vite / node"
    Get-CimInstance Win32_Process -Filter "Name='node.exe' OR Name='nodejs.exe' OR Name='pnpm.exe' OR Name='corepack.exe'" `
        -ErrorAction SilentlyContinue |
        Where-Object { Test-IsOurProcess $_.ProcessId } |
        ForEach-Object { Invoke-StopOnce $_.ProcessId "vite/node" }

    $listenMap = Get-ListenPidMap
    foreach ($listenPort in $listenMap.Keys) {
        foreach ($listenPid in $listenMap[$listenPort]) {
            if (Test-IsOurProcess $listenPid -AllowViteListener) {
                Invoke-StopOnce $listenPid "listen :$listenPort"
            }
        }
    }

    $waited = 0
    while ($waited -lt 4000) {
        $stillHeld = $false
        $listenMap = Get-ListenPidMap
        foreach ($listenPort in $listenMap.Keys) {
            foreach ($listenPid in $listenMap[$listenPort]) {
                if (Test-IsOurProcess $listenPid -AllowViteListener) {
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
}

if ($script:stopped.Count -eq 0) {
    Write-Host "[stop-dev] nothing running from this repo"
} else {
    Write-Host "[stop-dev] stopped $($script:stopped.Count) process tree(s)"
}
