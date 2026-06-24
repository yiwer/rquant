# rquant baostock fetch guard - invoked by a Windows Scheduled Task every 20 min; survives any session.
# Dedup uses a PID lock file (deterministic; NOT WMI CommandLine introspection, which can read $null
# for a just-started process and cause a duplicate launch).
# ASCII-only on purpose: PowerShell 5.1 mis-decodes a UTF-8 (no BOM) .ps1 with non-ASCII chars and fails to parse.
# Logic:
#   1) dataset complete (fetch.log tail contains 'DONE ok=') -> delete the task + clear lock, stop.
#   2) PID in lock file is still alive -> a fetch is running, exit (no duplicate).
#   3) otherwise (no lock / dead PID) -> launch fetch_watchdog.py (supervises fetch_baostock.py:
#      auto kill+resume on crash/stall), record its PID in the lock.
# Runs only while the current user is logged on (no stored password).
# Manual uninstall:  schtasks /Delete /TN rquant-baostock-fetch /F   then delete data\baostock\.fetch.lock

$ErrorActionPreference = 'SilentlyContinue'
$repo = 'E:\rust-app\rquant'
$py   = 'C:\Users\Administrator\AppData\Local\Programs\Python\Python313\python.exe'
$log  = Join-Path $repo 'data\baostock\fetch.log'
$lock = Join-Path $repo 'data\baostock\.fetch.lock'
$task = 'rquant-baostock-fetch'
function Note($m) { Add-Content -LiteralPath $log -Value ("[guard {0}] {1}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $m) }

# 1) complete -> self-remove task
if (Test-Path $log) {
    if ((Get-Content -LiteralPath $log -Tail 80) -match 'DONE ok=') {
        Note 'dataset DONE -> deleting scheduled task (self-cleanup)'
        schtasks /Delete /TN $task /F | Out-Null
        Remove-Item -LiteralPath $lock -Force
        exit 0
    }
}

# 2) lock PID alive -> already running
if (Test-Path $lock) {
    $lockpid = (Get-Content -LiteralPath $lock | Select-Object -First 1)
    if ($lockpid) { $lockpid = $lockpid.Trim() }
    if ($lockpid -and (Get-Process -Id $lockpid -ErrorAction SilentlyContinue)) { exit 0 }
    Note ("stale lock (pid {0} not alive) -> relaunching" -f $lockpid)
}

# 3) launch supervisor, record PID
$p = Start-Process -FilePath $py -ArgumentList 'scripts\fetch_watchdog.py' -WorkingDirectory $repo -WindowStyle Hidden -PassThru
Set-Content -LiteralPath $lock -Value $p.Id
Note ("launched watchdog pid {0}" -f $p.Id)
exit 0
