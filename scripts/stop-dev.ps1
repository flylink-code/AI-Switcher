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
$vitePorts = 5250..5270

Write-Host "[stop-dev] Project: $root"

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
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -match '^(node|nodejs|pnpm|corepack)(\.exe)?$' -and
            (Test-IsOurProcess $_.ProcessId)
        } |
        ForEach-Object { Invoke-StopOnce $_.ProcessId "vite/node" }

    foreach ($listenPort in $vitePorts) {
        foreach ($listenPid in (Get-ListenPids $listenPort)) {
            if (Test-IsOurProcess $listenPid -AllowViteListener) {
                Invoke-StopOnce $listenPid "listen :$listenPort"
            }
        }
    }

    $waited = 0
    while ($waited -lt 8000) {
        $stillHeld = $false
        foreach ($listenPort in $vitePorts) {
            foreach ($listenPid in (Get-ListenPids $listenPort)) {
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
