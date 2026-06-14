@echo off
REM Deep-history daily qfq fetch for the 10-symbol research universe.
REM Fetch date: <FILL by Task 4>   Probed Tencent max depth D: <FILL by Task 4>
REM Output: data\<symbol>.csv (gitignored). Re-run overwrites (idempotent).
setlocal
set RQ=target\release\rquant.exe
set DATALEN=3000
for %%S in (sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750) do (
  echo [fetch] %%S
  %RQ% fetch --symbol %%S --scale 240 --datalen %DATALEN% --adjust qfq --out data\%%S.csv
)
echo [done] deep fetch complete
endlocal
