# rquant gm tail fetch - scheduled launcher. One-shot: breadth -> funnel -> depth.
# PORTABLE: resolves repo root from this script's own location ($PSScriptRoot = scripts/),
#   and Python from $env:RQUANT_PYTHON (else 'python' on PATH). No hard-coded machine paths,
#   so it runs on any instance/clone. Funnel knobs come from data/gm/tail.config.json.
# ASCII-only on purpose: PowerShell 5.1 mis-decodes a UTF-8 (no BOM) .ps1 with non-ASCII chars.
# Why 14:46 default: the 14:45-labelled 15m bar (14:30-14:45) closes at 14:45:00; run after it
#   settles so the 14:45-asof factors see it. Leaves ~14 min to act before the 15:00 close.
# Assumes VPN/TUN is OFF (gm servers are domestic; global-proxy TUN slows the run ~10x).
# Manual run:       powershell -NoProfile -ExecutionPolicy Bypass -File <repo>\scripts\gm_tail_run.ps1
# Manual uninstall: schtasks /Delete /TN rquant-gm-tail /F   (or use the app's gm_tail_remove)

$repo   = Split-Path -Parent $PSScriptRoot                 # scripts/ -> repo root
$py     = if ($env:RQUANT_PYTHON) { $env:RQUANT_PYTHON } else { 'python' }
$script = Join-Path $repo 'scripts\fetch_gm_realtime.py'
$gmDir  = Join-Path $repo 'data\gm'
$cfg    = Join-Path $gmDir 'tail.config.json'
$log    = Join-Path $gmDir 'tail.log'
if (-not (Test-Path $gmDir)) { New-Item -ItemType Directory -Force -Path $gmDir | Out-Null }

# ---- defaults (used if config file missing / field absent) ----
$rank = 'liquidity'; $top = 300; $pool = ''
$minAmt = 30000000; $minPrice = 2.0; $dropLimit = $false
if (Test-Path $cfg) {
    try {
        $c = Get-Content -Raw -LiteralPath $cfg | ConvertFrom-Json
        if ($c.rank)         { $rank = [string]$c.rank }
        if ($c.top)          { $top = [int]$c.top }
        if ($c.pool)         { $pool = [string]$c.pool }
        if ($c.min_amount -ne $null) { $minAmt = [long]$c.min_amount }
        if ($c.min_price -ne $null)  { $minPrice = [string]$c.min_price }
        if ($c.drop_limit_up)        { $dropLimit = [bool]$c.drop_limit_up }
    } catch {
        Add-Content -LiteralPath $log -Encoding utf8 -Value ("[{0}] WARN bad config, using defaults: {1}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $_.Exception.Message)
    }
}
# pool: relative -> resolve against repo root (so config can stay portable)
if ($pool -and -not [System.IO.Path]::IsPathRooted($pool)) { $pool = Join-Path $repo $pool }

$pyArgs = @('--mode','tail','--limit','0','--funnel',
            '--rank', $rank, '--top', "$top",
            '--min-amount', "$minAmt", '--min-price', "$minPrice")
if ($pool)      { $pyArgs += @('--pool', $pool) }
if ($dropLimit) { $pyArgs += '--drop-limit-up' }

Add-Content -LiteralPath $log -Encoding utf8 -Value ("[{0}] === start tail --funnel (rank={1} top={2}) ===" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $rank, $top)
& $py $script @pyArgs *>&1 | Out-File -LiteralPath $log -Encoding utf8 -Append
Add-Content -LiteralPath $log -Encoding utf8 -Value ("[{0}] === done exit={1} ===" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $LASTEXITCODE)
