@echo off
REM Deep-history daily qfq fetch for the 10-symbol research universe.
REM Fetch date: 2026-06-15   Probed Tencent max depth D: 640 bars/window (per-request cap)
REM Actual coverage: 2018-01-02 .. 2026-06-12 (~2110 bars, 4-window merge, qfq anchored 2026-06-15)
REM Strategy: Tencent fqkline API caps at ~640 bars per request regardless of count param.
REM   count>2000 returns "param error"; count<=2000 with large date range returns last ~640 bars.
REM   Multi-window approach: 4 overlapping 30-month windows covering 2018-2026.
REM   Windows: W1=2018-01..2020-06, W2=2019-12..2022-06, W3=2022-01..2024-06, W4=2024-01..2026-06
REM Output: data\<symbol>.csv (gitignored). Re-run overwrites (idempotent).
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -Command "& { $symbols = @('sh600030','sh600036','sh600276','sh600519','sh600900','sh601088','sh601318','sz000333','sz000858','sz300750'); $windows = @(('2018-01-01','2020-06-30'),('2019-12-01','2022-06-30'),('2022-01-01','2024-06-30'),('2024-01-01','2026-06-15')); $base = 'https://web.ifzq.gtimg.cn/appstock/app/fqkline/get'; foreach ($sym in $symbols) { Write-Host \"[fetch] $sym\"; $all = @{}; foreach ($w in $windows) { $url = \"${base}?param=${sym},day,$($w[0]),$($w[1]),2000,qfq\"; try { $r = Invoke-RestMethod -Uri $url -TimeoutSec 30; $rows = $r.data.$sym.qfqday; if ($null -eq $rows) { $rows = $r.data.$sym.day }; if ($null -ne $rows) { foreach ($row in $rows) { $date = $row[0]; if (-not $all.ContainsKey($date)) { $all[$date] = $row } } } } catch { Write-Host \"  WARN: window $($w[0])..$($w[1]) failed: $_\" } }; $sorted = $all.Keys | Sort-Object; $csv = @('time,open,high,low,close,volume'); foreach ($d in $sorted) { $row = $all[$d]; $t = \"$d 15:00:00\"; $csv += \"$t,$($row[1]),$($row[3]),$($row[4]),$($row[2]),$($row[5])\" }; $outpath = \"data\$sym.csv\"; $csv | Set-Content -Path $outpath -Encoding UTF8; Write-Host \"  wrote $($sorted.Count) bars to $outpath\" } }"
echo [done] deep fetch complete
endlocal
