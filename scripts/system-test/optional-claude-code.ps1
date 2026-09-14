# Optional live Claude Code probe. Skips unless `claude` is on PATH.
# Does not rewrite the user's ~/.claude; points the CLI at the isolated
# gateway/proxy with ANTHROPIC_BASE_URL for one `claude -p --bare` turn.
#
# Usage (from run.ps1 -ClaudeCode, or):
#   $env:AISW_TEST_HOME = "..."
#   .\scripts\system-test\optional-claude-code.ps1

$ErrorActionPreference = "Stop"
$claude = Get-Command claude -ErrorAction SilentlyContinue
if (-not $claude) {
    Write-Host "[optional-claude-code] skip: claude CLI not on PATH"
    exit 0
}

$cdpInvoke = Join-Path $PSScriptRoot "cdp-invoke.mjs"
if (-not $env:AISW_TEST_HOME) {
    throw "AISW_TEST_HOME is required"
}

$statusJson = & node $cdpInvoke get_smart_gateway_status
$status = $statusJson | ConvertFrom-Json
$currentJson = & node $cdpInvoke get_current_provider '{"target":"claude_code"}'
$current = $currentJson | ConvertFrom-Json

$baseUrl = $null
$apiKey = "sk-system-test"
if ($current.providerKind -eq "smart_gateway") {
    $baseUrl = $status.baseUrl
    if ($status.apiKey) { $apiKey = $status.apiKey }
} else {
    $settingsPath = Join-Path $env:AISW_TEST_HOME ".claude\settings.json"
    if (Test-Path $settingsPath) {
        $settings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
        $baseUrl = $settings.env.ANTHROPIC_BASE_URL
    }
}

if (-not $baseUrl) {
    Write-Host "[optional-claude-code] skip: no isolated Claude Code base URL"
    exit 0
}

Write-Host "[optional-claude-code] claude -p --bare against $baseUrl"
$prevUrl = $env:ANTHROPIC_BASE_URL
$prevKey = $env:ANTHROPIC_API_KEY
try {
    $env:ANTHROPIC_BASE_URL = $baseUrl
    $env:ANTHROPIC_API_KEY = $apiKey
    $output = & claude -p --bare "Reply with the single word pong." 2>&1 | Out-String
    Write-Host $output
} finally {
    if ($null -ne $prevUrl) { $env:ANTHROPIC_BASE_URL = $prevUrl } else { Remove-Item Env:ANTHROPIC_BASE_URL -ErrorAction SilentlyContinue }
    if ($null -ne $prevKey) { $env:ANTHROPIC_API_KEY = $prevKey } else { Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue }
}

Start-Sleep -Seconds 2
$logsJson = & node $cdpInvoke list_proxy_request_logs_cmd '{"input":{"hours":1,"targetApp":"claude_code","page":0,"pageSize":10}}'
$logs = $logsJson | ConvertFrom-Json
$hops = @($logs.data | ForEach-Object { $_.hop })
Write-Host "[optional-claude-code] hops=$($hops -join ',')"
if ($hops.Count -eq 0) {
    Write-Host "[optional-claude-code] warn: no proxy_request_logs yet (upstream may have rejected the probe)"
    exit 0
}
$okHop = $hops | Where-Object { $_ -eq "agent_proxy" -or $_ -eq "smart_gateway" }
if (-not $okHop) {
    throw "expected hop=agent_proxy or smart_gateway, got $($hops -join ',')"
}
Write-Host "[optional-claude-code] OK"
exit 0
