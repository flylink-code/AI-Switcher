# Hand the machine back to the installed AI-Switcher after a local test session.
#
# Unlike stop-dev (which kills the installed app so hot-reload can start), this
# script only stops this-repo debug / Vite / local release copies. It then
# rewrites HKCU Run\AI-Switcher when that key still points at a test exe.
#
# Does not cargo-clean, does not touch ~/.claude-switcher, does not kill
# cargo/rustc or Cadence cdslmd.
#
# Usage:
#   .\scripts\clean-dev.ps1
#   .\scripts\clean-dev.ps1 -Launch     # start the installed app afterwards
#   .\scripts\clean-dev.ps1 -DryRun     # print actions only

param(
    [switch]$Launch,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$vitePorts = 5250..5270
$runKeyPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$approvedKeyPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
$autostartName = "AI-Switcher"
$approvedEnabled = [byte[]](0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)

Write-Host "[clean-dev] Project: $root"
if ($DryRun) { Write-Host "[clean-dev] Dry run - no processes or registry will change" }

function Stop-PidTree([int]$ProcessId) {
    if ($ProcessId -le 4) { return }
    if ($DryRun) { return }
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

function Get-ProcessExePath([int]$ProcessId) {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($proc -and $proc.Path) { return [string]$proc.Path }
    try {
        $row = Get-CimInstance Win32_Process -Filter "ProcessId=$ProcessId" -ErrorAction Stop
        if ($row.ExecutablePath) { return [string]$row.ExecutablePath }
    } catch { }
    return ""
}

function Get-ListenPids([int[]]$ListenPorts) {
    try {
        Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
            Where-Object { $_.OwningProcess -gt 0 -and $ListenPorts -contains [int]$_.LocalPort } |
            ForEach-Object { $_.OwningProcess } |
            Select-Object -Unique
    } catch {
        @()
    }
}

function ConvertTo-NormalizedPath([string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) { return "" }
    $trimmed = $Value.Trim().TrimStart("\\?\")
    try {
        return [System.IO.Path]::GetFullPath($trimmed)
    } catch {
        return $trimmed
    }
}

function Test-IsUnderRepo([string]$Path) {
    $full = ConvertTo-NormalizedPath $Path
    if (-not $full) { return $false }
    $rootFull = (ConvertTo-NormalizedPath $root).TrimEnd("\") + "\"
    return $full.StartsWith($rootFull, [System.StringComparison]::OrdinalIgnoreCase)
}

function Get-CommandExePath([string]$Command) {
    if ([string]::IsNullOrWhiteSpace($Command)) { return "" }
    $text = $Command.Trim()
    if ($text.StartsWith('"')) {
        $end = $text.IndexOf('"', 1)
        if ($end -gt 1) { return $text.Substring(1, $end - 1) }
    }
    return ($text -split "\s+", 2)[0]
}

function Test-IsTestExePath([string]$Path) {
    $full = ConvertTo-NormalizedPath $Path
    if (-not $full) { return $false }
    if (Test-IsUnderRepo $full) { return $true }
    $name = [System.IO.Path]::GetFileName($full)
    if ($name -match '(?i)^claude-switcher\.exe$') { return $true }
    if ($name -match '(?i)^AISwitcher-debug') { return $true }
    return $false
}

function Test-IsRepoViteProcess([int]$ProcessId, [switch]$AllowViteListener) {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return $false }
    if ($proc.ProcessName -notin @("node", "nodejs", "pnpm", "corepack")) { return $false }
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
    $label = if ($DryRun) { "would stop" } else { "stop" }
    Write-Host "[clean-dev]   $label PID $ProcessId ($name) $Why"
    Stop-PidTree $ProcessId
}

function Get-InstalledAiSwitcherExe {
    $candidates = New-Object System.Collections.Generic.List[string]

    $startMenuRoots = @(
        (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
        (Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs")
    )
    foreach ($menuRoot in $startMenuRoots) {
        if (-not (Test-Path $menuRoot)) { continue }
        $links = @()
        $links += @(Get-ChildItem $menuRoot -Recurse -Filter "*AI-Switcher*.lnk" -ErrorAction SilentlyContinue)
        $links += @(Get-ChildItem $menuRoot -Recurse -Filter "*AISwitcher*.lnk" -ErrorAction SilentlyContinue)
        foreach ($link in ($links | Select-Object -Unique)) {
            try {
                $shell = New-Object -ComObject WScript.Shell
                $target = [string]$shell.CreateShortcut($link.FullName).TargetPath
                if ($target) { $candidates.Add($target.Trim().Trim('"')) }
            } catch { }
        }
    }

    $uninstallRoots = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($hive in $uninstallRoots) {
        if (-not (Test-Path $hive)) { continue }
        Get-ChildItem $hive -ErrorAction SilentlyContinue | ForEach-Object {
            $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
            if (-not $props) { return }
            if ($props.DisplayName -notmatch '(?i)AI-Switcher|AISwitcher') { return }
            if ($props.DisplayIcon) {
                $icon = ($props.DisplayIcon -split ",", 2)[0].Trim().Trim('"')
                if ($icon) { $candidates.Add($icon) }
            }
            if ($props.InstallLocation) {
                $location = $props.InstallLocation.Trim().Trim('"')
                if ($location) {
                    $candidates.Add((Join-Path $location "AISwitcher.exe"))
                }
            }
        }
    }

    foreach ($wellKnown in @(
            (Join-Path $env:LOCALAPPDATA "AI-Switcher\AISwitcher.exe"),
            (Join-Path ${env:ProgramFiles} "AI-Switcher\AISwitcher.exe"),
            (Join-Path ${env:ProgramFiles(x86)} "AI-Switcher\AISwitcher.exe")
        )) {
        $candidates.Add($wellKnown)
    }

    foreach ($path in ($candidates | Select-Object -Unique)) {
        $full = ConvertTo-NormalizedPath $path
        if (-not $full) { continue }
        if (Test-IsTestExePath $full) { continue }
        if (-not (Test-Path -LiteralPath $full)) { continue }
        if ([System.IO.Path]::GetFileName($full) -notmatch '(?i)^AISwitcher\.exe$') { continue }
        return $full
    }
    return $null
}

function Get-AutostartCommand {
    try {
        return [string](Get-ItemPropertyValue -LiteralPath $runKeyPath -Name $autostartName -ErrorAction Stop)
    } catch {
        return $null
    }
}

function Set-AutostartCommand([string]$Command) {
    if ($DryRun) { return }
    if (-not (Test-Path $runKeyPath)) {
        New-Item -Path $runKeyPath -Force | Out-Null
    }
    New-ItemProperty -LiteralPath $runKeyPath -Name $autostartName -Value $Command -PropertyType String -Force | Out-Null
    if (-not (Test-Path $approvedKeyPath)) {
        New-Item -Path $approvedKeyPath -Force | Out-Null
    }
    New-ItemProperty -LiteralPath $approvedKeyPath -Name $autostartName -Value $approvedEnabled -PropertyType Binary -Force | Out-Null
}

function Remove-AutostartRegistration {
    if ($DryRun) { return }
    if (Test-Path $runKeyPath) {
        Remove-ItemProperty -LiteralPath $runKeyPath -Name $autostartName -ErrorAction SilentlyContinue
    }
    if (Test-Path $approvedKeyPath) {
        Remove-ItemProperty -LiteralPath $approvedKeyPath -Name $autostartName -ErrorAction SilentlyContinue
    }
}

Write-Host "[clean-dev] Stopping this-repo debug / Vite (installed AISwitcher is kept)"

Get-Process -Name "claude-switcher" -ErrorAction SilentlyContinue | ForEach-Object {
    Invoke-StopOnce $_.Id "debug cargo binary"
}

Get-Process -Name "AISwitcher" -ErrorAction SilentlyContinue | ForEach-Object {
    $exe = Get-ProcessExePath $_.Id
    if (Test-IsTestExePath $exe) {
        Invoke-StopOnce $_.Id "local test copy $exe"
    } else {
        Write-Host "[clean-dev]   keep PID $($_.Id) (installed) $exe"
    }
}

Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Name -match '^(node|nodejs|pnpm|corepack)(\.exe)?$' -and
        (Test-IsRepoViteProcess $_.ProcessId)
    } |
    ForEach-Object { Invoke-StopOnce $_.ProcessId "vite/node" }

foreach ($listenPid in (Get-ListenPids $vitePorts)) {
    if (Test-IsRepoViteProcess $listenPid -AllowViteListener) {
        Invoke-StopOnce $listenPid "vite listen"
    }
}

if ($script:stopped.Count -eq 0) {
    Write-Host "[clean-dev] no this-repo app/Vite process was running"
} else {
    $verb = if ($DryRun) { "would stop" } else { "stopped" }
    Write-Host "[clean-dev] $verb $($script:stopped.Count) process tree(s)"
}

$installed = Get-InstalledAiSwitcherExe
if ($installed) {
    Write-Host "[clean-dev] Installed app: $installed"
} else {
    Write-Host "[clean-dev] Installed AISwitcher.exe was not found (Start Menu / Uninstall / common paths)"
}

$currentAutostart = Get-AutostartCommand
if ([string]::IsNullOrWhiteSpace($currentAutostart)) {
    Write-Host "[clean-dev] Autostart: not registered"
} else {
    $autoExe = Get-CommandExePath $currentAutostart
    Write-Host "[clean-dev] Autostart: $currentAutostart"
    if (Test-IsTestExePath $autoExe) {
        if ($installed) {
            $restored = "`"$installed`" --autostart"
            $label = if ($DryRun) { "would restore" } else { "restore" }
            Write-Host "[clean-dev]   $label autostart -> $restored"
            Set-AutostartCommand $restored
        } else {
            $label = if ($DryRun) { "would remove" } else { "remove" }
            Write-Host "[clean-dev]   $label test autostart (installed exe not found)"
            Remove-AutostartRegistration
        }
    } else {
        Write-Host "[clean-dev]   leave installed autostart unchanged"
    }
}

if ($Launch) {
    if (-not $installed) {
        Write-Host "[clean-dev] -Launch skipped: installed exe not found"
        exit 2
    }
    $already = @(Get-Process -Name "AISwitcher" -ErrorAction SilentlyContinue | Where-Object {
        -not (Test-IsTestExePath (Get-ProcessExePath $_.Id))
    })
    if ($already.Count -gt 0) {
        Write-Host "[clean-dev] Installed app already running (PID $($already[0].Id))"
    } elseif ($DryRun) {
        Write-Host "[clean-dev] would launch $installed"
    } else {
        Write-Host "[clean-dev] Launch $installed"
        Start-Process -FilePath $installed
    }
}

Write-Host "[clean-dev] Done. Installed data under ~/.claude-switcher was not touched."
