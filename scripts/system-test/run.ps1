# L2 system-test runner (Windows). Isolates HOME and WebView data, builds debug,
# launches with CDP, runs IPC/DOM scenarios, then clean-dev.
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
$tauriConf = Join-Path $tauriDir "tauri.conf.json"
$artifacts = Join-Path $PSScriptRoot "artifacts"
$cleanDev = Join-Path $root "scripts\clean-dev.ps1"
$vitePortFile = Join-Path $targetDir "debug\.system-test-vite-port"
$viteOutLog = Join-Path $artifacts "vite.stdout.log"
$viteErrLog = Join-Path $artifacts "vite.stderr.log"

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

# 独立脚本避免 PowerShell 与 JavaScript 多层引号转义。
function Test-ViteHttpReady([int]$ListenPort, [int]$TimeoutMs = 5000) {
    & node (Join-Path $PSScriptRoot "vite-ready.mjs") "$ListenPort" "$TimeoutMs"
    return ($LASTEXITCODE -eq 0)
}

function Get-FileSha256([string]$Path) {
    if (-not (Test-Path $Path)) { return "" }
    try {
        return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash
    } catch {
        return ""
    }
}

function Get-CompiledVitePort([string]$ExePath, [string]$PortFilePath) {
    # 不从 EXE 字符串或当前配置猜端口；二者可能属于不同构建。
    if (-not (Test-Path $PortFilePath)) {
        throw "SkipBuild requires verified build metadata. Run once without -SkipBuild."
    }
    $raw = [string](Get-Content -LiteralPath $PortFilePath -Raw -ErrorAction Stop)
    $meta = $raw | ConvertFrom-Json
    $currentHash = Get-FileSha256 $ExePath
    if (-not $meta.exeHash -or -not $currentHash -or $meta.exeHash -ne $currentHash) {
        throw "SkipBuild metadata does not match the executable. Rebuild without -SkipBuild."
    }
    $port = [int]$meta.port
    if ($port -lt 1024 -or $port -gt 65535) {
        throw "SkipBuild metadata contains an invalid Vite port. Rebuild without -SkipBuild."
    }
    return $port
}

function Stop-PidTree([int]$ProcessId) {
    if ($ProcessId -le 4) { return }
    try {
        & taskkill.exe /F /T /PID $ProcessId *> $null
        if ($LASTEXITCODE -ne 0) {
            Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
        }
    } catch {
        Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
    }
}

function Set-DevUrlPort([string]$ConfPath, [int]$ListenPort) {
    $raw = [System.IO.File]::ReadAllText($ConfPath, [System.Text.Encoding]::UTF8)
    $updated = [regex]::Replace(
        $raw,
        '("devUrl"\s*:\s*"https?://)(?:localhost|127\.0\.0\.1):\d+(")',
        "`${1}127.0.0.1:$ListenPort`${2}"
    )
    if ($updated -eq $raw) {
        throw "Could not update devUrl in $ConfPath"
    }
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($ConfPath, $updated, $utf8NoBom)
}

function Save-FailedHome([string]$IsolatedHome, [string]$Reason) {
    New-Item -ItemType Directory -Force -Path $artifacts | Out-Null
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dest = Join-Path $artifacts $stamp
    Write-Host "[system-test] Keeping $IsolatedHome -> $dest ($Reason)"
    Copy-Item -LiteralPath $IsolatedHome -Destination $dest -Recurse -Force -ErrorAction SilentlyContinue
    $log = Join-Path $artifacts "$stamp.txt"
    Set-Content -LiteralPath $log -Value $Reason -Encoding UTF8
    if (Test-Path $viteOutLog) {
        Copy-Item -LiteralPath $viteOutLog -Destination (Join-Path $dest "vite.stdout.log") -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path $viteErrLog) {
        Copy-Item -LiteralPath $viteErrLog -Destination (Join-Path $dest "vite.stderr.log") -Force -ErrorAction SilentlyContinue
    }
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
Write-Host "[system-test] Keeping installed app running; the test instance uses isolated storage and ports"

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

if ($SkipBuild) {
    $vitePort = Get-CompiledVitePort $exePath $vitePortFile
    Write-Host "[system-test] -SkipBuild: using verified Vite port :$vitePort"
} else {
    $vitePort = Get-FreePort 5251 20
}

$originalConfBytes = $null
if (Test-Path $tauriConf) {
    $originalConfBytes = [System.IO.File]::ReadAllBytes($tauriConf)
}
$confPatched = $false
$viteProc = $null
$vitePid = 0
$proc = $null

$env:AISW_TEST_HOME = $testHome
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $testHome "webview2"
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
Remove-Item Env:TAURI_CONFIG -ErrorAction SilentlyContinue

Write-Host "[system-test] AISW_TEST_HOME=$testHome"
Write-Host "[system-test] CDP=$cdpPort gateway=$gatewayPort proxyBase=$proxyBase vite=$vitePort"

$failed = $false
$failReason = ""
try {
    $viteJs = Join-Path $root "node_modules\vite\bin\vite.js"
    if (-not (Test-Path $viteJs)) {
        throw "Vite is missing ($viteJs). Run: corepack pnpm install"
    }
    if ($SkipBuild -and -not (Test-Path $exePath)) {
        throw "-SkipBuild requires an existing debug executable: $exePath"
    }

    New-Item -ItemType Directory -Force -Path $artifacts | Out-Null
    if (-not (Test-LoopbackListening $vitePort)) {
        $node = Get-Command node -ErrorAction Stop
        Remove-Item -LiteralPath $viteOutLog -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $viteErrLog -Force -ErrorAction SilentlyContinue
        Write-Host "[system-test] Starting Vite on 127.0.0.1:$vitePort (stdout -> $viteOutLog, stderr -> $viteErrLog)"
        $viteProc = Start-Process -FilePath $node.Path -ArgumentList @(
            $viteJs, "--port", "$vitePort", "--strictPort", "--host", "127.0.0.1"
        ) -WorkingDirectory $root -PassThru -WindowStyle Hidden `
          -RedirectStandardOutput $viteOutLog -RedirectStandardError $viteErrLog -ErrorAction Stop
        if ($null -eq $viteProc -or $viteProc.Id -le 0) {
            throw "Vite process did not start"
        }
        $vitePid = [int]$viteProc.Id

        $viteReady = $false
        for ($i = 1; $i -le 60; $i++) {
            $viteState = Get-Process -Id $vitePid -ErrorAction SilentlyContinue
            if ($null -eq $viteState) {
                $errContent = ""
                if (Test-Path $viteErrLog) { $errContent = ([string](Get-Content $viteErrLog -Raw -ErrorAction SilentlyContinue)).Trim() }
                if (-not $errContent -and (Test-Path $viteOutLog)) { $errContent = ([string](Get-Content $viteOutLog -Raw -ErrorAction SilentlyContinue)).Trim() }
                $msg = "Vite process exited before readiness (PID $vitePid)"
                if ($errContent) { $msg += ":`n$errContent" }
                throw $msg
            }
            if (Test-ViteHttpReady $vitePort) {
                $viteReady = $true
                break
            }
            if ($i -eq 60) {
                $errContent = ""
                if (Test-Path $viteErrLog) { $errContent = ([string](Get-Content $viteErrLog -Raw -ErrorAction SilentlyContinue)).Trim() }
                if (-not $errContent -and (Test-Path $viteOutLog)) { $errContent = ([string](Get-Content $viteOutLog -Raw -ErrorAction SilentlyContinue)).Trim() }
                $msg = "Vite did not become ready on :$vitePort"
                if ($errContent) { $msg += ":`n$errContent" }
                throw $msg
            }
            Start-Sleep -Milliseconds 500
        }
        Write-Host "[system-test] Vite ready: http://127.0.0.1:$vitePort/"
    } elseif (-not (Test-ViteHttpReady $vitePort)) {
        throw "Port $vitePort is occupied but is not serving Vite (Test-ViteHttpReady failed)"
    } else {
        Write-Host "[system-test] Vite already running and ready on http://127.0.0.1:$vitePort/"
    }

    if (-not $SkipBuild) {
        # target >= 20GB clean protection
        $nestedTarget = Join-Path $tauriDir "src-tauri"
        if (Test-Path $nestedTarget) {
            Write-Host "[system-test] Removing nested $nestedTarget (relative CARGO_TARGET_DIR leftover)"
            Remove-Item -LiteralPath $nestedTarget -Recurse -Force -ErrorAction SilentlyContinue
        }

        $targetGb = Get-DirSizeGB $targetDir
        Write-Host ("[system-test] src-tauri\target size: {0:N1} GB (auto-clean >= 20 GB)" -f $targetGb)
        if ($targetGb -ge 20 -and (Test-Path $targetDir)) {
            $cargoBusy = @(Get-Process -Name "cargo", "rustc" -ErrorAction SilentlyContinue)
            if ($cargoBusy.Count -gt 0) {
                $pids = ($cargoBusy | ForEach-Object { $_.Id }) -join ", "
                throw "Another cargo/rustc process is running (PIDs: $pids). Stop it before cargo clean."
            }
            Write-Host "[system-test] target exceeds 20 GB; performing cargo clean before build"
            Push-Location $tauriDir
            try {
                & cargo clean
                if ($LASTEXITCODE -ne 0) { throw "cargo clean failed (exit $LASTEXITCODE)" }
            } finally {
                Pop-Location
            }
        }

        Set-DevUrlPort $tauriConf $vitePort
        $confPatched = $true
        $env:TAURI_CONFIG = ('{"build":{"devUrl":"http://127.0.0.1:' + $vitePort + '"}}')

        Write-Host "[system-test] cargo build --cfg dev (Vite :$vitePort, TAURI_CONFIG override)"
        Push-Location $tauriDir
        try {
            $previousRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
            $env:CARGO_ENCODED_RUSTFLAGS = "--cfg$([char]0x1f)dev"
            & cargo build
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
        } finally {
            $env:CARGO_ENCODED_RUSTFLAGS = $previousRustFlags
            Pop-Location
            Remove-Item Env:TAURI_CONFIG -ErrorAction SilentlyContinue
            if ($confPatched -and $originalConfBytes) {
                [System.IO.File]::WriteAllBytes($tauriConf, $originalConfBytes)
                $confPatched = $false
                Write-Host "[system-test] Restored tauri.conf.json byte-for-byte"
            }
        }

        # Remember compiled port and EXE hash for subsequent -SkipBuild runs
        $debugDir = Join-Path $targetDir "debug"
        if (-not (Test-Path $debugDir)) {
            New-Item -ItemType Directory -Force -Path $debugDir | Out-Null
        }
        $exeHash = Get-FileSha256 $exePath
        $metaObj = @{
            port = $vitePort
            exeHash = $exeHash
            updatedAt = (Get-Date -Format "o")
        }
        $metaJson = $metaObj | ConvertTo-Json -Compress
        Set-Content -LiteralPath $vitePortFile -Value $metaJson -Encoding UTF8
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
    if (-not $ready) { throw "CDP :$cdpPort did not come up" }

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
    $failReason = "$_`n$($_.ScriptStackTrace)"
    Write-Host "[system-test] ERROR: $failReason"
} finally {
    if ($proc -and -not $proc.HasExited) {
        Stop-PidTree $proc.Id
    }
    Remove-Item Env:TAURI_CONFIG -ErrorAction SilentlyContinue
    if ($confPatched -and $originalConfBytes) {
        [System.IO.File]::WriteAllBytes($tauriConf, $originalConfBytes)
        $confPatched = $false
        Write-Host "[system-test] Restored tauri.conf.json byte-for-byte in finally"
    }
    if ($vitePid -gt 0 -and (Get-Process -Id $vitePid -ErrorAction SilentlyContinue)) {
        Stop-PidTree $vitePid
    }
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
